use luna_factoryd::mcp::{app_resource, capabilities, tool_definitions};
use serde_json::{Value, json};

#[test]
fn rust_sdk_preserves_standard_and_openai_metadata() {
    let tools = serde_json::to_value(tool_definitions()).unwrap();
    let global = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "open_factory")
        .unwrap();
    assert_eq!(
        global["_meta"]["ui"]["resourceUri"],
        "ui://luna-factory/workbench.html"
    );
    assert_eq!(global["_meta"]["ui"]["visibility"], json!(["app"]));
    assert_eq!(
        global["_meta"]["openai/ui"]["entrypoints"],
        json!([{"type":"global"}])
    );
    let panel = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "open_factory_panel")
        .unwrap();
    assert_eq!(
        panel["_meta"]["openai/ui"]["entrypoints"],
        json!([{"type":"thread"}])
    );
    let caps: Value = serde_json::to_value(capabilities()).unwrap();
    assert!(caps["extensions"]["io.modelcontextprotocol/ui"].is_object());
    let resource = serde_json::to_value(app_resource("<h1>Luna Factory</h1>")).unwrap();
    assert_eq!(
        resource["contents"][0]["mimeType"],
        "text/html;profile=mcp-app"
    );
}

#[test]
fn status_and_ui_reads_are_truthfully_read_only() {
    for tool in tool_definitions() {
        let value = serde_json::to_value(&tool).unwrap();
        if [
            "list_factory_runs",
            "get_factory_run",
            "get_factory_capabilities",
            "open_factory",
            "open_factory_panel",
            "refresh_factory",
        ]
        .contains(&tool.name.as_ref())
        {
            assert_eq!(value["annotations"]["readOnlyHint"], true, "{}", tool.name);
        }
        assert!(value["inputSchema"]["additionalProperties"] == false);
    }
}

#[test]
fn server_identity_contains_a_portable_png_icon() {
    use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory};
    use rmcp::ServerHandler;
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/absent","skill_path":dir.path().join("SKILL.md"),"repositories":{},"profiles":{},"limits":{"capacity":1,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new("html".into()),
    };
    let info: Value = serde_json::to_value(server.get_info()).unwrap();
    assert_eq!(info["serverInfo"]["icons"][0]["mimeType"], "image/png");
    assert_eq!(info["serverInfo"]["icons"][0]["sizes"], json!(["512x512"]));
    assert!(
        info["serverInfo"]["icons"][0]["src"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,iVBORw0KGgo")
    );
}
