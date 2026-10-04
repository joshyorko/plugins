use luna_factoryd::mcp::{app_resource, capabilities, tool_definitions};
use serde_json::{Value, json};

#[test]
fn rust_sdk_preserves_standard_and_openai_metadata() {
    let tools = serde_json::to_value(tool_definitions()).unwrap();
    let global = tools.as_array().unwrap().iter().find(|t| t["name"] == "open_factory").unwrap();
    assert_eq!(global["_meta"]["ui"]["resourceUri"], "ui://luna-factory/workbench.html");
    assert_eq!(global["_meta"]["ui"]["visibility"], json!(["app"]));
    assert_eq!(global["_meta"]["openai/ui"]["entrypoints"], json!([{"type":"global"}]));
    let panel = tools.as_array().unwrap().iter().find(|t| t["name"] == "open_factory_panel").unwrap();
    assert_eq!(panel["_meta"]["openai/ui"]["entrypoints"], json!([{"type":"thread"}]));
    let caps: Value = serde_json::to_value(capabilities()).unwrap();
    assert!(caps["extensions"]["io.modelcontextprotocol/ui"].is_object());
    let resource = serde_json::to_value(app_resource("<h1>Luna Factory</h1>")).unwrap();
    assert_eq!(resource["contents"][0]["mimeType"], "text/html;profile=mcp-app");
}

#[test]
fn status_and_ui_reads_are_truthfully_read_only() {
    for tool in tool_definitions() {
        let value = serde_json::to_value(&tool).unwrap();
        if ["list_factory_runs", "get_factory_run", "get_factory_capabilities", "open_factory", "open_factory_panel", "refresh_factory"].contains(&tool.name.as_ref()) {
            assert_eq!(value["annotations"]["readOnlyHint"], true, "{}", tool.name);
        }
        assert!(value["inputSchema"]["additionalProperties"] == false);
    }
}
