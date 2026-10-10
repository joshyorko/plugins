use crate::{
    lifecycle::Factory,
    mcp::{APP_URI, app_resource, capabilities, tool_definitions},
    store::StartRequest,
};
use anyhow::Context;
use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use base64::Engine;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::*,
    service::{PeerRequestOptions, RequestContext},
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
    #[serde(default)]
    expected_revision: Option<u64>,
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
struct MentionSearch {
    query: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryRequestInput {
    candidate_id: Option<String>,
    alias: Option<String>,
    max_finish: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resume {
    run_id: String,
    #[serde(default)]
    expected_revision: Option<u64>,
    message: Option<String>,
    expected_decision_id: Option<String>,
    #[serde(default)]
    diagnosis: Option<crate::control::Diagnosis>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsPatch {
    set: Value,
}
fn default_limit() -> u32 {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Steer {
    run_id: String,
    #[serde(default)]
    expected_revision: Option<u64>,
    expected_turn_id: String,
    message: String,
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(value)?)
}
impl McpServer {
    async fn call_repository_request(
        &self,
        arguments: Value,
        input_responses: Option<InputResponses>,
        request_state: Option<String>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let input: RepositoryRequestInput = serde_json::from_value(arguments).map_err(|_| {
            ErrorData::invalid_params("Repository request fields are invalid", None)
        })?;
        if input_responses.is_none()
            && request_state.is_none()
            && let (Some(candidate_id), Some(alias), Some(max_finish)) =
                (input.candidate_id, input.alias, input.max_finish)
        {
            return self
                .record_repository_request(crate::repositories::RegistrationRequest {
                    candidate_id,
                    alias,
                    max_finish,
                })
                .await;
        }

        let discovery =
            self.factory.discover_repositories().await.map_err(|_| {
                ErrorData::invalid_params("Repository discovery is unavailable", None)
            })?;
        let candidates = discovery
            .get("candidates")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if candidates.as_array().is_none_or(Vec::is_empty) {
            return Err(ErrorData::invalid_params(
                "No discovered repositories are available. Configure an operator-approved discovery root first.",
                None,
            ));
        }

        let form_result = if let Some(responses) = input_responses {
            let state = request_state.as_deref().unwrap_or_default();
            if !crate::extensions::is_repository_form_state(state)
                || responses.len() != 1
                || !responses.contains_key("repository")
            {
                return Err(ErrorData::invalid_params(
                    "This repository form response is stale or does not match the active request.",
                    None,
                ));
            }
            let form_candidates = self
                .factory
                .consume_repository_form_state(state, &candidates)
                .await
                .ok_or_else(|| {
                    ErrorData::invalid_params(
                        "This repository form response is stale or belongs to another request.",
                        None,
                    )
                })?;
            let result = &responses["repository"];
            Some(
                crate::extensions::parse_repository_form_result(result, &form_candidates).map_err(
                    |_| {
                        ErrorData::invalid_params(
                            "Repository form response is invalid or stale",
                            None,
                        )
                    },
                )?,
            )
        } else if request_state.is_some() {
            return Err(ErrorData::invalid_params(
                "This repository form response is stale or incomplete.",
                None,
            ));
        } else {
            None
        };

        let decision = if let Some(result) = form_result {
            result
        } else {
            let capabilities = context.client_capabilities().unwrap_or_default();
            let protocol = context
                .protocol_version()
                .map(|version| version.to_string())
                .unwrap_or_default();
            if !crate::extensions::supports_openai_form(
                &protocol,
                &serde_json::to_value(&capabilities).unwrap_or_default(),
            ) {
                if protocol.as_str() < "2026-07-28"
                    && capabilities
                        .elicitation
                        .as_ref()
                        .is_some_and(|cap| cap.form.is_some())
                    && capabilities.extensions.as_ref().is_some_and(|items| {
                        items
                            .get("openai/elicitation")
                            .and_then(|value| value.get("form"))
                            .is_some()
                    })
                {
                    let params = crate::extensions::legacy_repository_form_params(&candidates)
                        .map_err(|_| {
                            ErrorData::invalid_params("Repository form is unavailable", None)
                        })?;
                    let request = ServerRequest::CustomRequest(CustomRequest::new(
                        crate::extensions::OPENAI_LEGACY_FORM_METHOD,
                        Some(params),
                    ));
                    let response = context
                        .peer
                        .send_request_with_option(
                            request,
                            PeerRequestOptions::with_timeout(std::time::Duration::from_secs(300)),
                        )
                        .await
                        .map_err(|_| {
                            ErrorData::invalid_request("Repository form was unavailable", None)
                        })?;
                    let response = response.await_response().await.map_err(|_| {
                        ErrorData::invalid_request("Repository form response was unavailable", None)
                    })?;
                    let response = match response {
                        ClientResult::CustomResult(result) => result.0,
                        ClientResult::ElicitResult(result) => {
                            serde_json::to_value(result).unwrap_or(Value::Null)
                        }
                        _ => {
                            return Err(ErrorData::invalid_request(
                                "Repository form response was unavailable",
                                None,
                            ));
                        }
                    };
                    crate::extensions::parse_repository_form_result(&response, &candidates)
                        .map_err(|_| {
                            ErrorData::invalid_params(
                                "Repository form response is invalid or stale",
                                None,
                            )
                        })?
                } else {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        "Open Luna Factory and use the accessible Add repository form. This host does not support the repository form request; no access request was created.",
                    )]).into());
                }
            } else {
                let state = self
                    .factory
                    .issue_repository_form_state(&candidates)
                    .await
                    .map_err(|_| {
                        ErrorData::invalid_params("Repository form is unavailable", None)
                    })?;
                let result =
                    crate::extensions::make_repository_form(&candidates, &state).map_err(|_| {
                        ErrorData::invalid_params("Repository form is unavailable", None)
                    })?;
                return Ok(CallToolResponse::InputRequired(result));
            }
        };

        let registration = match decision {
            crate::extensions::FormDecision::Accepted(request) => request,
            crate::extensions::FormDecision::Declined => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    "Repository selection was declined. No request was created.",
                )])
                .into());
            }
            crate::extensions::FormDecision::Cancelled => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    "Repository selection was cancelled. No request was created.",
                )])
                .into());
            }
        };
        self.record_repository_request(registration).await
    }

    async fn record_repository_request(
        &self,
        request: crate::repositories::RegistrationRequest,
    ) -> Result<CallToolResponse, ErrorData> {
        let value = self
            .factory
            .request_repository(request)
            .await
            .map_err(|_| {
                ErrorData::invalid_params("Repository request could not be recorded", None)
            })?;
        let mut result = CallToolResult::structured(value);
        result.content = vec![ContentBlock::text(
            "Access was requested. A local operator must approve the repository before Factory can use it.",
        )];
        Ok(result.into())
    }

    pub async fn invoke(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        match name {
            "start_factory" => self.factory.start(parse::<StartRequest>(args)?).await,
            "create_factory_graph" => {
                self.factory
                    .create_graph(parse::<StartRequest>(args)?)
                    .await
            }
            "get_factory_graph" => self.factory.graph(&parse::<RunId>(args)?.run_id).await,
            "propose_factory_change" => self.factory.propose_graph_change(parse(args)?).await,
            "apply_factory_change" => self.factory.apply_graph_change(parse(args)?).await,
            "get_factory_backends" => {
                ensure_empty(&args)?;
                Ok(crate::backends::capabilities(&self.factory.config))
            }
            "inspect_factory_cas" => self.factory.inspect_cas(parse(args)?).await,
            "read_factory_agent_timeline" => self.factory.read_agent_timeline(parse(args)?).await,
            "list_factory_runs" => {
                Ok(json!({"runs":self.factory.list(parse::<List>(args)?.limit).await?}))
            }
            "get_factory_run" => self.factory.get(&parse::<RunId>(args)?.run_id).await,
            "search_factory_mentions" => {
                let query = parse::<MentionSearch>(args)?.query;
                let runs = self.factory.list(50).await?;
                let runs = runs.as_array().context("mention_run_list_unavailable")?;
                crate::mentions::search_items(runs, &query)
            }
            "get_factory_capabilities" => {
                ensure_empty(&args)?;
                self.factory.capabilities().await
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
                    .steer_at_revision(
                        &p.run_id,
                        &p.expected_turn_id,
                        &p.message,
                        p.expected_revision,
                    )
                    .await
            }
            "cancel_factory_run" => {
                let p: RunId = parse(args)?;
                self.factory
                    .cancel_at_revision(&p.run_id, p.expected_revision)
                    .await
            }
            "reconcile_factory_run" => {
                let p: RunId = parse(args)?;
                self.factory.reconcile(&p.run_id, p.expected_revision).await
            }
            "resume_factory_run" => {
                let params: Resume = parse(args)?;
                self.factory
                    .resume_with_diagnosis(
                        &params.run_id,
                        params.message.as_deref(),
                        params.expected_decision_id.as_deref(),
                        params.expected_revision,
                        params.diagnosis,
                    )
                    .await
            }
            "read_factory_settings" => {
                ensure_empty(&args)?;
                self.factory.settings().await
            }
            "discover_factory_repositories" => {
                ensure_empty(&args)?;
                self.factory.discover_repositories().await
            }
            "request_factory_repository" => self.factory.request_repository(parse(args)?).await,
            "update_factory_settings" => {
                self.factory
                    .update_settings(parse::<SettingsPatch>(args)?.set)
                    .await
            }
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
        let mut config: ServerConfig = serde_json::from_value(json!({"protocolVersion":"2025-11-25","capabilities":capabilities(),"serverInfo":{"name":"luna-factory","title":"Luna Factory","version":env!("CARGO_PKG_VERSION")},"instructions":"One canonical Luna Factory runtime. Status and UI reads make no inference calls. Start requests require an approved repository alias and explicit bounded authority. Never treat a tool response as live model-route or tunnel/UI proof."})).expect("static server metadata");
        config.server_info.icons = Some(vec![
            Icon::new(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD
                    .encode(include_bytes!("../../assets/logo.png"))
            ))
            .with_mime_type("image/png")
            .with_sizes(vec!["512x512".into()]),
        ]);
        config
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
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        if serde_json::to_vec(&args).map_or(true, |v| v.len() > 65536) {
            return Ok(
                CallToolResult::error(vec![ContentBlock::text("Request exceeds 64 KiB.")]).into(),
            );
        }
        if request.name == "request_factory_repository" {
            return self
                .call_repository_request(
                    args,
                    request.input_responses,
                    request.request_state,
                    context,
                )
                .await;
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
        if request.uri == APP_URI {
            return Ok(app_resource(&self.html).into());
        }
        let mention = crate::mentions::parse_mention_uri(&request.uri)
            .map_err(|_| ErrorData::invalid_params("Mention URI is invalid", None))?;
        let run = self
            .factory
            .get(&mention.run_id)
            .await
            .map_err(|_| ErrorData::invalid_params("Mentioned run is unavailable", None))?;
        let payload = crate::mentions::resource_payload(&run, &mention).map_err(|error| {
            let message = match error.to_string().as_str() {
                "stale_mention_revision" => {
                    "This mention is stale. Search again for current state."
                }
                "mention_task_unavailable" => "This task is no longer available in the run.",
                _ => "Mentioned run state is unavailable.",
            };
            ErrorData::invalid_params(message, None)
        })?;
        let result: ReadResourceResult = serde_json::from_value(json!({
            "resultType":"complete","ttlMs":0,"cacheScope":"private",
            "contents":[{"uri":request.uri,"mimeType":"application/json","text":payload.to_string()}]
        }))
        .map_err(|_| ErrorData::internal_error("Mention resource could not be serialized", None))?;
        Ok(result.into())
    }
}

pub fn allowed_http(
    host: Option<&str>,
    origin: Option<&str>,
    listen: &str,
    published_origin: Option<&str>,
) -> bool {
    let Some(host) = host else {
        return false;
    };
    let Ok(address) = listen.parse::<std::net::SocketAddr>() else {
        return false;
    };
    if let Some(published_origin) = published_origin {
        if !address.ip().is_unspecified()
            || !crate::config::valid_published_origin(published_origin)
        {
            return false;
        }
        let Ok(uri) = published_origin.parse::<axum::http::Uri>() else {
            return false;
        };
        let Some(authority) = uri.authority() else {
            return false;
        };
        return host == authority.as_str()
            && origin.is_none_or(|request_origin| request_origin == published_origin);
    }
    if !address.ip().is_loopback() {
        return false;
    }
    let localhost = format!("localhost:{}", address.port());
    if host != listen && host != localhost {
        return false;
    }
    origin.is_none_or(|origin| {
        origin == format!("http://{listen}") || origin == format!("http://{localhost}")
    })
}
async fn guard(
    State((listen, published_origin)): State<(String, Option<String>)>,
    request: Request,
    next: Next,
) -> Response {
    let host = request.headers().get("host").and_then(|v| v.to_str().ok());
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if !allowed_http(host, origin, &listen, published_origin.as_deref()) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(axum::body::Body::from("Host or Origin rejected"))
            .unwrap();
    }
    next.run(request).await
}
pub fn router(factory: Factory, html: String, cancel: CancellationToken) -> Router {
    let listen = factory.config.listen.to_string();
    let published_origin = factory.config.published_origin.clone();
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
        .layer(middleware::from_fn_with_state(
            (listen, published_origin),
            guard,
        ))
}
