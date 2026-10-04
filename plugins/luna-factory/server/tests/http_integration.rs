//! Exercises real rmcp Streamable HTTP handlers without any native runtime.
use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use luna_factoryd::{config::Config, lifecycle::Factory};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn setup() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let config:Config=serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/intentionally/not/installed","skill_path":dir.path().join("SKILL.md"),"repositories":{},"profiles":{},"limits":{"capacity":1,"repair_attempts":1,"wall_seconds":60}})).unwrap();
    let factory = Factory::new(config).unwrap();
    (
        dir,
        luna_factoryd::http::router(
            factory,
            "<!doctype html><title>Luna fixture</title>".into(),
            CancellationToken::new(),
        ),
    )
}
async fn rpc(app: &Router, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"luna-tests","version":"1"},"io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}}}});
    let mut request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "127.0.0.1:8787")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method);
    if let Some(name) = params
        .get("name")
        .or_else(|| params.get("uri"))
        .and_then(Value::as_str)
    {
        request = request.header("mcp-name", name);
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        status.is_success(),
        "{}: {}",
        status,
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
async fn mcp_2026_discovery_tools_resource_and_zero_inference_reads_work() {
    let (_dir, app) = setup();
    let discovery = rpc(&app, "server/discover", json!({})).await;
    assert!(
        discovery["result"]["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
    assert!(
        discovery["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/ui"].is_object()
    );
    let tools = rpc(&app, "tools/list", json!({})).await;
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "open_factory"
                && t["_meta"]["openai/ui"]["entrypoints"][0]["type"] == "global")
    );
    let read = rpc(
        &app,
        "resources/read",
        json!({"uri":"ui://luna-factory/workbench.html"}),
    )
    .await;
    assert_eq!(
        read["result"]["contents"][0]["mimeType"],
        "text/html;profile=mcp-app"
    );
    for name in [
        "get_factory_capabilities",
        "list_factory_runs",
        "open_factory",
        "read_factory_settings",
    ] {
        let result = rpc(&app, "tools/call", json!({"name":name,"arguments":{}})).await;
        assert_ne!(result["result"]["isError"], true, "{name}: {result}");
        assert!(
            result["result"]["structuredContent"].is_object(),
            "{name}: non-object structured content"
        );
    }
}
