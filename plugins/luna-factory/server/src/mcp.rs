//! Typed rmcp definitions. OpenAI metadata stays inside supported MCP extension maps.
use rmcp::model::{ReadResourceResult, ServerCapabilities, Tool};
use serde_json::{Value, json};

pub const APP_URI: &str = "ui://luna-factory/workbench.html";

pub fn capabilities() -> ServerCapabilities {
    serde_json::from_value(json!({
        "tools": {}, "resources": {},
        "extensions": {
            "io.modelcontextprotocol/ui": {"mimeTypes":["text/html;profile=mcp-app"]},
            "openai/settings": {"readTool":"read_factory_settings","updateTool":"update_factory_settings"}
        }
    })).expect("static MCP capabilities")
}

fn definition(name: &str, title: &str, description: &str, schema: Value, read_only: bool) -> Tool {
    serde_json::from_value(json!({
        "name":name,"title":title,"description":description,"inputSchema":schema,
        "annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"idempotentHint":read_only,"openWorldHint":!read_only}
    })).expect("static tool definition")
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub fn tool_definitions() -> Vec<Tool> {
    let id = json!({"type":"string","minLength":1,"maxLength":64});
    let mut tools = vec![
        definition(
            "start_factory",
            "Start Luna Factory",
            "Start one bounded objective in an operator-approved repository. Explicit finish authority and finite limits are required.",
            object(
                json!({
                    "repository":{"type":"string","maxLength":64},"objective":{"type":"string","minLength":1,"maxLength":8000},
                    "acceptance":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","maxLength":1000}},
                    "non_goals":{"type":"array","maxItems":32,"items":{"type":"string","maxLength":1000}},
                    "finish":{"type":"string","enum":["local_candidate","push","pr"]},
                    "profile":{"type":"string","maxLength":64},"capacity":{"type":"integer","minimum":1,"maximum":8},
                    "repair_attempts":{"type":"integer","minimum":0,"maximum":10},"wall_seconds":{"type":"integer","minimum":30,"maximum":86400},
                    "idempotency_key":{"type":"string","minLength":1,"maxLength":128}
                }),
                &[
                    "repository",
                    "objective",
                    "acceptance",
                    "non_goals",
                    "finish",
                    "profile",
                    "capacity",
                    "repair_attempts",
                    "wall_seconds",
                    "idempotency_key",
                ],
            ),
            false,
        ),
        definition(
            "list_factory_runs",
            "Factory runs",
            "Read recent runs and operator attention without inference.",
            object(
                json!({"limit":{"type":"integer","minimum":1,"maximum":100}}),
                &[],
            ),
            true,
        ),
        definition(
            "get_factory_run",
            "Factory run",
            "Read one run, remaining gap and bounded evidence without inference.",
            object(json!({"run_id":id}), &["run_id"]),
            true,
        ),
        definition(
            "get_factory_capabilities",
            "Factory capabilities",
            "Read trusted repository aliases, runtime capability and routing evidence. This never starts a model.",
            object(json!({}), &[]),
            true,
        ),
        definition(
            "steer_factory_run",
            "Steer factory run",
            "Send an in-scope correction to the current owner, fenced to the expected turn. Does not expand authority.",
            object(
                json!({"run_id":id,"expected_turn_id":id,"message":{"type":"string","minLength":1,"maxLength":4000}}),
                &["run_id", "expected_turn_id", "message"],
            ),
            false,
        ),
        definition(
            "cancel_factory_run",
            "Stop factory run",
            "Interrupt only owned execution and verify stopped descendants before releasing its repository claim.",
            object(json!({"run_id":id}), &["run_id"]),
            false,
        ),
        definition(
            "resume_factory_run",
            "Resume factory run",
            "Reconcile the same native thread with preserved authority and limits. Optionally include an in-scope operator answer.",
            object(
                json!({"run_id":id,"message":{"type":"string","minLength":1,"maxLength":4000}}),
                &["run_id"],
            ),
            false,
        ),
        definition(
            "open_factory",
            "Luna Factory",
            "Open the Luna Factory workbench.",
            object(json!({}), &[]),
            true,
        ),
        definition(
            "open_factory_panel",
            "Factory run panel",
            "Open the currently selected factory run beside this conversation.",
            object(json!({}), &[]),
            true,
        ),
        definition(
            "refresh_factory",
            "Refresh workbench",
            "Read bounded workbench data without inference.",
            object(json!({"run_id":id}), &[]),
            true,
        ),
        definition(
            "read_factory_settings",
            "Factory settings",
            "Read safe UI defaults. Trusted server configuration remains authoritative.",
            object(json!({}), &[]),
            true,
        ),
        definition(
            "update_factory_settings",
            "Update factory defaults",
            "Change safe UI defaults within trusted limits. Does not change repository access or runtime permissions.",
            object(
                json!({"capacity":{"type":"integer","minimum":1,"maximum":8},"finish":{"type":"string","enum":["local_candidate","push","pr"]},"profile":{"type":"string","maxLength":64}}),
                &[],
            ),
            false,
        ),
    ];
    for tool in &mut tools {
        let name = tool.name.as_ref();
        if matches!(
            name,
            "open_factory" | "open_factory_panel" | "refresh_factory"
        ) {
            let mut meta = json!({"ui":{"resourceUri":APP_URI,"visibility":["app"]}});
            if name != "refresh_factory" {
                meta["openai/ui"] = json!({"entrypoints":[{"type":if name=="open_factory" {"global"} else {"thread"}}]});
            }
            tool.meta = Some(serde_json::from_value(meta).expect("static UI metadata"));
        } else if matches!(name, "read_factory_settings" | "update_factory_settings") {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"visibility":["app"]}}))
                    .expect("static settings metadata"),
            );
            if name == "read_factory_settings" {
                tool.output_schema = Some(serde_json::from_value(json!({"type":"object","properties":{"schema":{"type":"object"},"values":{"type":"object"},"layout":{"type":"array"}},"required":["schema","values"]})).expect("settings schema"));
            }
        } else if matches!(name, "start_factory" | "get_factory_run") {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"resourceUri":APP_URI}}))
                    .expect("static result metadata"),
            );
        }
    }
    tools
}

pub fn app_resource(html: &str) -> ReadResourceResult {
    serde_json::from_value(json!({
        "resultType":"complete","ttlMs":0,"cacheScope":"private",
        "contents":[{"uri":APP_URI,"mimeType":"text/html;profile=mcp-app","text":html,
            "_meta":{"ui":{"csp":{"connectDomains":[],"resourceDomains":[]}}}}]
    }))
    .expect("static resource metadata")
}
