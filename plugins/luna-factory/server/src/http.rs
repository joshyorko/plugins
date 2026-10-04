use crate::{
    lifecycle::Factory,
    mcp::{APP_URI, app_resource, capabilities, tool_definitions},
    store::StartRequest,
};
use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct McpServer {
    pub factory: Factory,
    pub html: Arc<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunId {
    run_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    run_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    #[serde(default = "default_limit")]
    limit: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resume {
    run_id: String,
    message: Option<String>,
}
fn default_limit() -> u32 {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Steer {
    run_id: String,
    expected_turn_id: String,
    message: String,
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(value)?)
}
impl McpServer {
    pub async fn invoke(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        match name {
            "start_factory" => self.factory.start(parse::<StartRequest>(args)?).await,
            "list_factory_runs" => {
                Ok(json!({"runs":self.factory.list(parse::<List>(args)?.limit).await?}))
            }
            "get_factory_run" => self.factory.get(&parse::<RunId>(args)?.run_id).await,
            "get_factory_capabilities" => {
                ensure_empty(&args)?;
                Ok(self.factory.capabilities())
            }
            "open_factory" | "open_factory_panel" => {
                ensure_empty(&args)?;
                self.factory.workbench(None).await
            }
            "refresh_factory" => {
                self.factory
                    .workbench(parse::<Refresh>(args)?.run_id.as_deref())
                    .await
            }
            "steer_factory_run" => {
                let p: Steer = parse(args)?;
                self.factory
                    .steer(&p.run_id, &p.expected_turn_id, &p.message)
                    .await
            }
            "cancel_factory_run" => self.factory.cancel(&parse::<RunId>(args)?.run_id).await,
            "resume_factory_run" => {
                let params: Resume = parse(args)?;
                self.factory
                    .resume_with_input(&params.run_id, params.message.as_deref())
                    .await
            }
            "read_factory_settings" => {
                ensure_empty(&args)?;
                self.factory.settings().await
            }
            "update_factory_settings" => self.factory.update_settings(args).await,
            _ => anyhow::bail!("unknown_tool"),
        }
    }
}
fn ensure_empty(value: &Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.as_object().is_some_and(|o| o.is_empty()),
        "unexpected_arguments"
    );
    Ok(())
}
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        serde_json::from_value(json!({"protocolVersion":"2025-11-25","capabilities":capabilities(),"serverInfo":{"name":"luna-factory","title":"Luna Factory","version":env!("CARGO_PKG_VERSION")},"instructions":"One canonical Luna Factory runtime. Status and UI reads make no inference calls. Start requests require an approved repository alias and explicit bounded authority. Never treat a tool response as live model-route or tunnel/UI proof."})).expect("static server metadata")
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tool_definitions()))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool_definitions().into_iter().find(|t| t.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        if serde_json::to_vec(&args).map_or(true, |v| v.len() > 65536) {
            return Ok(
                CallToolResult::error(vec![ContentBlock::text("Request exceeds 64 KiB.")]).into(),
            );
        }
        let result = match self.invoke(&request.name, args).await {
            Ok(value) => {
                let text = if let Some(state) = value.get("state").and_then(Value::as_str) {
                    format!(
                        "Luna Factory {}: {}. {}",
                        value["id"].as_str().unwrap_or("run"),
                        state,
                        value["delta"].as_str().unwrap_or("")
                    )
                } else {
                    "Luna Factory status. See structured data for bounded runs, capabilities or settings.".into()
                };
                let mut result = CallToolResult::structured(value);
                result.content = vec![ContentBlock::text(text)];
                result
            }
            Err(error) => {
                // Errors from our boundaries have a stable leading context; never
                // include the source chain, native stderr, filesystem paths or logs.
                let reason = error.to_string();
                let safe = if reason.len() <= 160 && !reason.contains('/') && !reason.contains('\\')
                {
                    reason
                } else {
                    "Request could not be completed. Inspect the local operator diagnostics; no automatic retry was performed.".into()
                };
                CallToolResult::error(vec![ContentBlock::text(safe)])
            }
        };
        Ok(result.into())
    }
    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(APP_URI, "luna-factory-workbench")
                .with_mime_type("text/html;profile=mcp-app"),
        ]))
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != APP_URI {
            return Err(ErrorData::invalid_params("Unknown resource", None));
        }
        Ok(app_resource(&self.html).into())
    }
}

pub fn allowed_http(host: Option<&str>, origin: Option<&str>, listen: &str) -> bool {
    let Some(host) = host else {
        return false;
    };
    let Ok(address) = listen.parse::<std::net::SocketAddr>() else {
        return false;
    };
    let localhost = format!("localhost:{}", address.port());
    if host != listen && host != localhost {
        return false;
    }
    origin.is_none_or(|origin| {
        origin == format!("http://{listen}") || origin == format!("http://{localhost}")
    })
}
async fn guard(State(listen): State<String>, request: Request, next: Next) -> Response {
    let host = request.headers().get("host").and_then(|v| v.to_str().ok());
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if !allowed_http(host, origin, &listen) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(axum::body::Body::from("Host or Origin rejected"))
            .unwrap();
    }
    next.run(request).await
}
pub fn router(factory: Factory, html: String, cancel: CancellationToken) -> Router {
    let listen = factory.config.listen.to_string();
    let html = Arc::new(html);
    let service = StreamableHttpService::new(
        move || {
            Ok(McpServer {
                factory: factory.clone(),
                html: html.clone(),
            })
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_cancellation_token(cancel),
    );
    Router::new()
        .nest_service("/mcp", service)
        .layer(tower_http::limit::RequestBodyLimitLayer::new(65536))
        .layer(middleware::from_fn_with_state(listen, guard))
}
