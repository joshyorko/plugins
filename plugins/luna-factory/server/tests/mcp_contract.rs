use luna_factoryd::mcp::{app_resource, capabilities, tool_definitions};
use serde_json::{Value, json};

#[test]
fn every_structured_tool_declares_an_object_output_contract() {
    for tool in tool_definitions() {
        let value = serde_json::to_value(&tool).unwrap();
        assert_eq!(value["outputSchema"]["type"], "object", "{}", tool.name);
        assert!(
            value["outputSchema"]["required"]
                .as_array()
                .is_some_and(|a| !a.is_empty()),
            "{}",
            tool.name
        );
    }
}

#[test]
fn graph_catalog_requires_revision_and_is_callable_by_both_audiences() {
    let tools = tool_definitions();
    assert_eq!(tools.len(), 24);
    for name in [
        "create_factory_graph",
        "get_factory_graph",
        "get_factory_backends",
        "propose_factory_change",
        "apply_factory_change",
    ] {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let value = serde_json::to_value(tool).unwrap();
        assert_eq!(value["_meta"]["ui"]["visibility"], json!(["model", "app"]));
        if matches!(
            name,
            "create_factory_graph" | "propose_factory_change" | "apply_factory_change"
        ) {
            assert_eq!(value["annotations"]["idempotentHint"], true);
            assert_eq!(value["annotations"]["openWorldHint"], false);
        }
        if name == "propose_factory_change" || name == "apply_factory_change" {
            assert!(
                value["inputSchema"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("expected_revision"))
            );
            assert_eq!(
                value["inputSchema"]["properties"]["expected_revision"]["maximum"],
                9_007_199_254_740_991_u64
            );
        }
    }
    let settings = tools
        .iter()
        .find(|t| t.name == "update_factory_settings")
        .unwrap();
    let input = serde_json::to_value(&settings.input_schema).unwrap();
    let validator = jsonschema::validator_for(&input).unwrap();
    assert!(validator.is_valid(&json!({"set":{"capacity":1}})));
    assert!(!validator.is_valid(&json!({"capacity":1})));
    assert!(!validator.is_valid(&json!({"set":{}})));
    let workbench = app_resource("<!doctype html>");
    let resource = serde_json::to_value(workbench).unwrap();
    assert_eq!(
        resource["contents"][0]["_meta"]["openai/ui"]["availableDisplayModes"],
        json!(["inline", "fullscreen"])
    );
    assert_eq!(
        resource["contents"][0]["_meta"]["openai/ui"]["preferredDisplayMode"],
        "inline"
    );
}

#[test]
fn github_reads_are_read_only_open_world_and_callable_by_both_audiences() {
    let tools = tool_definitions();
    for name in ["inspect_factory_issue_graph", "read_factory_delivery"] {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let value = serde_json::to_value(tool).unwrap();
        assert_eq!(value["_meta"]["ui"]["visibility"], json!(["model", "app"]));
        assert_eq!(
            value["annotations"],
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true})
        );
        assert_eq!(value["inputSchema"]["additionalProperties"], false);
        let output = &value["outputSchema"]["properties"];
        assert_eq!(output["reported_by"]["const"], "github");
    }
    let inspect = tools
        .iter()
        .find(|t| t.name == "inspect_factory_issue_graph")
        .unwrap();
    let input = serde_json::to_value(&inspect.input_schema).unwrap();
    assert_eq!(input["required"], json!(["repository", "parent"]));
    let validator = jsonschema::validator_for(&input).unwrap();
    assert!(validator.is_valid(&json!({"repository":"joshyorko/plugins","parent":67})));
    for bad in [
        json!({"repository":"joshyorko/plugins#67","parent":67}),
        json!({"repository":"../plugins","parent":67}),
        json!({"repository":"joshyorko/plugins","parent":0}),
        json!({"repository":"joshyorko/plugins","parent":67,"token":"x"}),
    ] {
        assert!(!validator.is_valid(&bad), "{bad}");
    }
    let model_visible = tools
        .iter()
        .filter(|tool| {
            serde_json::to_value(tool).unwrap()["_meta"]["ui"]["visibility"]
                .as_array()
                .is_none_or(|v| v.iter().any(|c| c == "model"))
        })
        .count();
    assert_eq!(model_visible, 16);
    let delivery = tools
        .iter()
        .find(|t| t.name == "read_factory_delivery")
        .unwrap();
    let output = serde_json::to_value(delivery.output_schema.as_ref().unwrap()).unwrap();
    assert_eq!(output["properties"]["proof"]["const"], "none");
    assert_eq!(output["properties"]["merge_capability"]["const"], "none");
    assert_eq!(output["properties"]["min_interval_seconds"]["const"], 60);
}

#[test]
fn composer_mentions_use_read_only_app_visible_search_and_current_capability() {
    let server_capabilities = serde_json::to_value(capabilities()).unwrap();
    assert_eq!(
        server_capabilities["extensions"]["openai/mentions"]["searchTool"],
        "search_factory_mentions"
    );

    let tool = tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "search_factory_mentions")
        .expect("composer mention search is registered");
    let value = serde_json::to_value(tool).unwrap();
    assert_eq!(value["annotations"]["readOnlyHint"], true);
    assert_eq!(value["_meta"]["ui"]["visibility"], json!(["app"]));
    assert_eq!(
        value["_meta"]["openai/extensions"]["mentions/search"],
        json!({})
    );
    assert_eq!(value["inputSchema"]["required"], json!(["query"]));
    assert_eq!(
        value["inputSchema"]["properties"]["query"]["type"],
        "string"
    );
    assert_eq!(
        value["outputSchema"]["properties"]["items"]["type"],
        "array"
    );
    assert_eq!(
        value["outputSchema"]["properties"]["items"]["items"]["$ref"],
        "#/$defs/resource_link"
    );
}

#[test]
fn repository_request_tool_is_app_visible_for_html_fallback_and_allows_host_elicitation() {
    let tool = tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "request_factory_repository")
        .expect("repository request tool is registered");
    let value = serde_json::to_value(tool).unwrap();
    assert_eq!(
        value["_meta"]["ui"]["resourceUri"],
        "ui://luna-factory/workbench.html"
    );
    assert_eq!(value["_meta"]["ui"]["visibility"], json!(["app"]));
    assert_eq!(value["inputSchema"]["required"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_partial_settings_updates_retain_both_fields_and_revalidate_defaults() {
    use luna_factoryd::{config::Config, lifecycle::Factory, store::Store};
    let dir = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/absent","skill_path":dir.path().join("SKILL.md"),"repositories":{},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":2,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    Store::open(&config)
        .unwrap()
        .save_settings(&json!({"capacity":8,"profile":"removed","finish":"deploy"}))
        .unwrap();
    let factory = Factory::new(config).unwrap();
    assert_eq!(
        factory.settings().await.unwrap()["values"],
        json!({"capacity":1,"profile":"default","finish":"local_candidate"})
    );
    for _ in 0..16 {
        factory
            .update_settings(json!({"capacity":1,"finish":"local_candidate"}))
            .await
            .unwrap();
        let a = factory.clone();
        let b = factory.clone();
        let (left, right) = tokio::join!(
            tokio::spawn(async move { a.update_settings(json!({"capacity":2})).await }),
            tokio::spawn(async move { b.update_settings(json!({"finish":"push"})).await })
        );
        left.unwrap().unwrap();
        right.unwrap().unwrap();
        assert_eq!(
            factory.settings().await.unwrap()["values"],
            json!({"capacity":2,"profile":"default","finish":"push"})
        );
    }
}

#[tokio::test]
async fn native_settings_patch_is_strict_partial_and_returns_effective_values() {
    use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/absent","skill_path":dir.path().join("SKILL.md"),"repositories":{},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":2,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new(String::new()),
    };
    let first = server
        .invoke("update_factory_settings", json!({"set":{"capacity":2}}))
        .await
        .unwrap();
    assert_eq!(
        first["values"],
        json!({"capacity":2,"finish":"local_candidate","profile":"default"})
    );
    let second = server
        .invoke("update_factory_settings", json!({"set":{"finish":"push"}}))
        .await
        .unwrap();
    assert_eq!(second["values"]["capacity"], 2);
    for bad in [
        json!({"capacity":1}),
        json!({"set":{}}),
        json!({"set":{"capacity":3}}),
        json!({"set":{"capacity":true}}),
        json!({"set":{"profile":"foreign"}}),
        json!({"set":{"root":"/tmp"}}),
        json!({"set":{"capacity":1},"actor":"admin"}),
    ] {
        assert!(
            server
                .invoke("update_factory_settings", bad.clone())
                .await
                .is_err(),
            "{bad}"
        );
    }
    assert_eq!(
        server
            .invoke("read_factory_settings", json!({}))
            .await
            .unwrap()["values"],
        second["values"]
    );
}

#[tokio::test]
async fn settings_without_profiles_does_not_invent_an_allowed_profile() {
    use luna_factoryd::{config::Config, lifecycle::Factory};
    let dir = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/absent","skill_path":dir.path().join("SKILL.md"),"repositories":{},"profiles":{},"limits":{"capacity":1,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    let factory = Factory::new(config).unwrap();
    let settings = factory.settings().await.unwrap();
    assert!(settings["schema"]["properties"].get("profile").is_none());
    assert!(settings["values"].get("profile").is_none());
}

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
fn reconciliation_is_callable_in_the_model_filtered_catalog() {
    let models: Vec<_> = tool_definitions()
        .into_iter()
        .filter(|tool| {
            let value = serde_json::to_value(tool).unwrap();
            value["_meta"]["ui"]["visibility"]
                .as_array()
                .is_none_or(|visibility| visibility.iter().any(|context| context == "model"))
        })
        .collect();
    let reconcile = models
        .iter()
        .find(|tool| tool.name == "reconcile_factory_run")
        .expect("the advertised recovery action must exist in ChatGPT's callable catalog");
    let value = serde_json::to_value(reconcile).unwrap();
    assert_eq!(value["annotations"]["readOnlyHint"], true);
    assert_eq!(value["inputSchema"]["required"], json!(["run_id"]));
    assert!(value["inputSchema"]["properties"]["expected_revision"].is_object());
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
