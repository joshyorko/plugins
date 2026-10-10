//! Exercises real rmcp Streamable HTTP handlers without any native runtime.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
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
async fn rpc_raw(app: &Router, method: &str, mut params: Value) -> (StatusCode, Value) {
    let client_capabilities = params
        .get("_meta")
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .cloned()
        .unwrap_or_else(|| json!({"extensions":{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}}}));
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"luna-tests","version":"1"},"io.modelcontextprotocol/clientCapabilities":client_capabilities});
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
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn rpc(app: &Router, method: &str, params: Value) -> Value {
    let (status, value) = rpc_raw(app, method, params).await;
    assert!(status.is_success(), "{}: {}", status, value);
    value
}

fn setup_with_discovery() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let discovery = dir.path().join("discovery");
    let repo = discovery.join("sample-service");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/intentionally/not/installed","skill_path":dir.path().join("SKILL.md"),"repositories":{},"discovery_roots":{"sandbox":{"root":discovery,"max_finish":"local_candidate"}},"profiles":{},"limits":{"capacity":1,"repair_attempts":1,"wall_seconds":60}})).unwrap();
    (
        dir,
        luna_factoryd::http::router(
            Factory::new(config).unwrap(),
            "<!doctype html><title>Luna fixture</title>".into(),
            CancellationToken::new(),
        ),
    )
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
    assert_eq!(
        discovery["result"]["capabilities"]["extensions"]["openai/mentions"]["searchTool"],
        "search_factory_mentions"
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
    let mention = rpc(
        &app,
        "tools/call",
        json!({"name":"search_factory_mentions","arguments":{"query":"no-such-run"}}),
    )
    .await;
    assert_ne!(mention["result"]["isError"], true);
    assert_eq!(mention["result"]["structuredContent"]["items"], json!([]));
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

#[tokio::test]
async fn repository_choice_uses_mrtr_and_only_records_operator_pending_request() {
    let (_dir, app) = setup_with_discovery();
    let capabilities = json!({
        "elicitation":{"form":{"schemaValidation":true}},
        "extensions":{
            "openai/elicitation":{"form":{}},
            "io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}
        }
    });
    let first = rpc(
        &app,
        "tools/call",
        json!({
            "name":"request_factory_repository","arguments":{},
            "_meta":{"io.modelcontextprotocol/clientCapabilities":capabilities}
        }),
    )
    .await;
    let required = &first["result"];
    assert_eq!(required["resultType"], "input_required");
    let form = &required["inputRequests"]["repository"]["params"];
    assert_eq!(
        form["requestedSchema"],
        json!({"type":"object","properties":{}})
    );
    let rich_schema = &form["_meta"]["openai/elicitation"]["requestedSchema"];
    let candidate_id = rich_schema["properties"]["candidate_id"]["oneOf"][0]["const"]
        .as_str()
        .unwrap();
    assert_eq!(
        rich_schema["properties"]["request_for_local_approval"]["default"],
        false
    );
    assert!(
        form["message"]
            .as_str()
            .unwrap()
            .contains("local operator must approve")
    );
    assert!(!required.to_string().contains("/discovery/"));

    let retry = rpc(&app, "tools/call", json!({
        "name":"request_factory_repository","arguments":{},
        "requestState":required["requestState"],
        "inputResponses":{"repository":{"action":"accept","content":{
            "candidate_id":candidate_id,"alias":"sample","max_finish":"local_candidate","request_for_local_approval":true
        }}},
        "_meta":{"io.modelcontextprotocol/clientCapabilities":capabilities}
    })).await;
    assert_ne!(retry["result"]["isError"], true);
    assert_eq!(retry["result"]["structuredContent"]["status"], "pending");
    assert_eq!(retry["result"]["structuredContent"]["alias"], "sample");

    let fresh = rpc(
        &app,
        "tools/call",
        json!({
            "name":"request_factory_repository","arguments":{},
            "_meta":{"io.modelcontextprotocol/clientCapabilities":capabilities}
        }),
    )
    .await;
    let cancelled = rpc(
        &app,
        "tools/call",
        json!({
            "name":"request_factory_repository","arguments":{},
            "requestState":fresh["result"]["requestState"],
            "inputResponses":{"repository":{"action":"cancel"}},
            "_meta":{"io.modelcontextprotocol/clientCapabilities":capabilities}
        }),
    )
    .await;
    assert_eq!(cancelled["result"]["isError"], true);
    assert!(
        cancelled["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("cancelled")
    );
    let fallback = rpc(
        &app,
        "tools/call",
        json!({
            "name":"request_factory_repository","arguments":{}
        }),
    )
    .await;
    assert_eq!(fallback["result"]["isError"], true);
    assert!(
        fallback["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("accessible Add repository form")
    );
    let raced = rpc_raw(&app, "tools/call", json!({
        "name":"request_factory_repository","arguments":{},
        "requestState":required["requestState"],
        "inputResponses":{"repository":{"action":"accept","content":{
            "candidate_id":candidate_id,"alias":"raced","max_finish":"local_candidate","request_for_local_approval":true
        }}},
        "_meta":{"io.modelcontextprotocol/clientCapabilities":capabilities}
    })).await;
    assert!(
        raced.1["error"].is_object() || raced.1["result"]["isError"] == true,
        "race response: {}",
        raced.1
    );
    let requests = rpc(
        &app,
        "tools/call",
        json!({"name":"discover_factory_repositories","arguments":{}}),
    )
    .await;
    assert_eq!(
        requests["result"]["structuredContent"]["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
