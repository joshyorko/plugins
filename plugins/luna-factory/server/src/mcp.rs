//! Typed rmcp definitions. OpenAI metadata stays inside supported MCP extension maps.
use rmcp::model::{ReadResourceResult, ServerCapabilities, Tool};
use serde_json::{Value, json};

pub const APP_URI: &str = "ui://luna-factory/workbench.html";

pub fn capabilities() -> ServerCapabilities {
    serde_json::from_value(json!({
        "tools": {}, "resources": {},
        "extensions": {
            "io.modelcontextprotocol/ui": {"mimeTypes":["text/html;profile=mcp-app"]},
            "openai/mentions": {"searchTool":"search_factory_mentions"},
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
            "search_factory_mentions",
            "Search Factory runs and tasks",
            "Search the currently accessible Factory ledger for exact run or task mentions. This read-only search performs no inference or execution and returns revision-fenced resource links without filesystem paths or private evidence.",
            object(
                json!({"query":{"type":"string","maxLength":160}}),
                &["query"],
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
                json!({"run_id":id,"expected_revision":{"type":"integer","minimum":0},"expected_turn_id":id,"message":{"type":"string","minLength":1,"maxLength":4000}}),
                &["run_id", "expected_turn_id", "message"],
            ),
            false,
        ),
        definition(
            "cancel_factory_run",
            "Stop factory run",
            "Interrupt only owned execution and verify stopped descendants before releasing its repository claim.",
            object(
                json!({"run_id":id,"expected_revision":{"type":"integer","minimum":0}}),
                &["run_id"],
            ),
            false,
        ),
        definition(
            "resume_factory_run",
            "Resume factory run",
            "Reconcile the same native thread with preserved authority and limits. To answer a completed decision, include message and the current pending_decision.id as expected_decision_id. Identical answer retries never dispatch twice.",
            object(
                json!({"run_id":id,"expected_revision":{"type":"integer","minimum":0},"message":{"type":"string","minLength":1,"maxLength":4000},"expected_decision_id":{"type":"string","minLength":1,"maxLength":128},"diagnosis":{"type":"object","additionalProperties":false,"properties":{"summary":{"type":"string","minLength":1,"maxLength":1000},"basis":{"type":"string","enum":["operator_semantic","observed_checks"]},"check_refs":{"type":"array","maxItems":32,"items":{"type":"string"}},"same_goal_replan":{"type":"boolean"}},"required":["summary","basis","check_refs"]}}),
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
                json!({"set":{"type":"object","minProperties":1,"additionalProperties":false,"properties":{"capacity":{"type":"integer","minimum":1,"maximum":8},"finish":{"type":"string","enum":["local_candidate","push","pr"]},"profile":{"type":"string","maxLength":64}}}}),
                &["set"],
            ),
            false,
        ),
    ];
    tools.extend([
        definition("reconcile_factory_run","Reconcile native ownership","Observe existing owned native work and current checks without starting, resuming or stopping inference or retrying effects.",object(json!({"run_id":id,"expected_revision":{"type":"integer","minimum":0}}),&["run_id"]),true),
        definition("discover_factory_repositories", "Discover local repositories", "Read a bounded catalog under operator-approved local roots. Returns opaque candidate IDs, never absolute paths or file contents. No inference.", object(json!({}), &[]), true),
        definition("request_factory_repository", "Request repository access", "Request one discovered repository alias and explicit finish cap. With no complete selection, supported ChatGPT hosts may ask through an OpenAI form; otherwise use the accessible Luna Factory Add repository form. This only queues a request; local operator approval is still required. Never accepts filesystem paths or remote approval.", object(json!({"candidate_id":{"type":"string","pattern":"^[a-f0-9]{64}$"},"alias":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,64}$"},"max_finish":{"type":"string","enum":["local_candidate","push","pr"]}}), &[]), false),
    ]);
    let create_schema = serde_json::to_value(&tools[0].input_schema).expect("start schema");
    let node_id = json!({"type":"string","minLength":1,"maxLength":256});
    let deps = json!({"type":"array","maxItems":128,"uniqueItems":true,"items":node_id});
    let source = object(
        json!({"provider":node_id,"repository_id":node_id,"item_id":node_id,"revision":node_id}),
        &["provider", "repository_id", "item_id", "revision"],
    );
    let candidate = object(
        json!({"id":node_id,"title":{"type":"string","minLength":1,"maxLength":4000},"criterion_ids":{"type":"array","minItems":1,"maxItems":32,"items":node_id},"dependencies":deps,"source":source}),
        &["id", "title", "criterion_ids", "dependencies", "source"],
    );
    let change = json!({"oneOf":[
        object(json!({"kind":{"const":"import_candidates"},"nodes":{"type":"array","minItems":1,"maxItems":32,"items":candidate}}), &["kind","nodes"]),
        object(json!({"kind":{"const":"set_dependencies"},"node_id":node_id,"dependencies":deps}), &["kind","node_id","dependencies"]),
        object(json!({"kind":{"const":"set_target"},"node_id":node_id,"target_id":node_id}), &["kind","node_id","target_id"])
    ]});
    let revision = json!({"type":"integer","minimum":0,"maximum":9_007_199_254_740_991_u64});
    tools.extend([
        definition("create_factory_graph", "Create planning graph", "Create a repository-bound planning graph in the existing Factory ledger. No execution claim, worker or inference is started. This planning-only record cannot be resumed as execution.", create_schema, false),
        definition("get_factory_graph", "Inspect engineering graph", "Read authoritative repository-bound nodes, dependencies, source bindings, claims, proof and proposed changes. Selection/context is not authorization.", object(json!({"run_id":id}), &["run_id"]), true),
        definition("get_factory_backends", "Inspect execution capabilities", "Read configuration-only target capabilities and unknown entitlement evidence. Starts no daemon or authentication. A planning preference does not qualify subscription-backed execution.", object(json!({}), &[]), true),
        definition("inspect_factory_cas", "Inspect configured CAS target", "Opt-in read-only CAS inspection. Use an operator-configured target alias. Optionally supply both run_id and an existing durable planning request_id to read its receipt and exact thread. Does not dispatch, retry, authenticate, release claims or certify execution. Missing receipts and subscription entitlement remain unverified.", object(json!({"target_alias":{"type":"string","minLength":1,"maxLength":64},"run_id":id,"request_id":{"type":"string","minLength":1,"maxLength":128}}), &["target_alias"]), true),
        definition("propose_factory_change", "Propose graph change", "Record a source-bound, revision-fenced planning proposal under local operator authority. Imports remain candidates without execution authority. Inspect its returned ID/revision before applying; duplicate keys require identical payloads.", object(json!({"run_id":id,"expected_revision":revision,"idempotency_key":node_id,"change":change}), &["run_id","expected_revision","idempotency_key","change"]), false),
        definition("apply_factory_change", "Apply graph change", "Apply one inspected proposal at its current revision. Rechecks source, authority and backend preference. Does not dispatch or reassign active/unknown execution. Identical recorded retries do not apply twice.", object(json!({"run_id":id,"change_id":node_id,"expected_revision":revision}), &["run_id","change_id","expected_revision"]), false),
    ]);
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
        } else if name == "search_factory_mentions" {
            tool.meta = Some(
                serde_json::from_value(json!({
                    "ui":{"visibility":["app"]},
                    "openai/extensions":{"mentions/search":{}}
                }))
                .expect("static mention metadata"),
            );
        } else if name == "request_factory_repository" {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"resourceUri":APP_URI,"visibility":["app"]}}))
                    .expect("static repository fallback UI metadata"),
            );
        } else if matches!(
            name,
            "read_factory_settings" | "update_factory_settings" | "discover_factory_repositories"
        ) {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"visibility":["app"]}}))
                    .expect("static settings metadata"),
            );
        } else if matches!(
            name,
            "reconcile_factory_run"
                | "create_factory_graph"
                | "get_factory_graph"
                | "get_factory_backends"
                | "inspect_factory_cas"
                | "propose_factory_change"
                | "apply_factory_change"
        ) {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"visibility":["model","app"]}}))
                    .expect("static recovery visibility"),
            );
        } else if matches!(name, "start_factory" | "get_factory_run") {
            tool.meta = Some(
                serde_json::from_value(json!({"ui":{"resourceUri":APP_URI}}))
                    .expect("static result metadata"),
            );
        }
        if matches!(
            name,
            "create_factory_graph" | "propose_factory_change" | "apply_factory_change"
        ) {
            tool.annotations = Some(serde_json::from_value(json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false})).expect("planning annotations"));
        }
        tool.output_schema = Some(
            serde_json::from_value(crate::schemas::output_schema(name))
                .expect("structured output schema"),
        );
    }
    tools
}

pub fn app_resource(html: &str) -> ReadResourceResult {
    serde_json::from_value(json!({
        "resultType":"complete","ttlMs":0,"cacheScope":"private",
        "contents":[{"uri":APP_URI,"mimeType":"text/html;profile=mcp-app","text":html,
            "_meta":{
                "ui":{"csp":{"connectDomains":[],"resourceDomains":[]}},
                "openai/ui":{"availableDisplayModes":["inline","fullscreen"],"preferredDisplayMode":"inline"}
            }}]
    }))
    .expect("static resource metadata")
}
