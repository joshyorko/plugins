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
    let start = create_schema["properties"].clone();
    let node_id = json!({"type":"string","minLength":1,"maxLength":256});
    let deps = json!({"type":"array","maxItems":128,"uniqueItems":true,"items":node_id});
    // Optional display hints (for example a GitHub issue number and link). They are never
    // proof, authority or source identity, and do not participate in duplicate detection.
    let mut display = object(
        json!({"number":{"type":"integer","minimum":1,"maximum":crate::graph::DISPLAY_NUMBER_MAX},
            "url":{"type":"string","maxLength":crate::graph::DISPLAY_URL_MAX,"pattern":"^https://github\\.com/[A-Za-z0-9._~%/-]+$"}}),
        &[],
    );
    display["minProperties"] = json!(1);
    let source = object(
        json!({"provider":node_id,"repository_id":node_id,"item_id":node_id,"revision":node_id,"display":display}),
        &["provider", "repository_id", "item_id", "revision"],
    );
    let candidate = object(
        json!({"id":node_id,"title":{"type":"string","minLength":1,"maxLength":4000},"criterion_ids":{"type":"array","minItems":1,"maxItems":32,"items":node_id},"dependencies":deps,"source":source}),
        &["id", "title", "criterion_ids", "dependencies", "source"],
    );
    let change = json!({"oneOf":[
        object(json!({"kind":{"const":"import_candidates"},"nodes":{"type":"array","minItems":1,"maxItems":32,"items":candidate}}), &["kind","nodes"]),
        object(json!({"kind":{"const":"set_dependencies"},"node_id":node_id,"dependencies":deps}), &["kind","node_id","dependencies"]),
        object(json!({"kind":{"const":"set_target"},"node_id":node_id,"target_id":node_id}), &["kind","node_id","target_id"]),
        object(json!({"kind":{"const":"update_task"},"node_id":node_id,"title":{"type":["string","null"],"minLength":1,"maxLength":8000},"dependencies":deps,"target_id":{"type":["string","null"],"minLength":1,"maxLength":256}}), &["kind","node_id","title","dependencies","target_id"])
    ]});
    let revision = json!({"type":"integer","minimum":0,"maximum":9_007_199_254_740_991_u64});
    tools.extend([
        definition("create_factory_graph", "Create planning graph", "Create a repository-bound planning graph in the existing Factory ledger. No execution claim, worker or inference is started. This planning-only record cannot be resumed as execution.", create_schema, false),
        definition("get_factory_graph", "Inspect engineering graph", "Read authoritative repository-bound nodes, dependencies, source bindings, claims, proof and proposed changes. Selection/context is not authorization.", object(json!({"run_id":id}), &["run_id"]), true),
        definition("get_factory_backends", "Inspect execution capabilities", "Read configuration-only target capabilities and unknown entitlement evidence. Starts no daemon or authentication. A planning preference does not qualify subscription-backed execution.", object(json!({}), &[]), true),
        definition("inspect_factory_cas", "Inspect configured CAS target", "Opt-in read-only CAS inspection. Use an operator-configured target alias. Optionally supply both run_id and an existing durable planning request_id to read its receipt and exact thread. Does not dispatch, retry, authenticate, release claims or certify execution. Missing receipts and subscription entitlement remain unverified.", object(json!({"target_alias":{"type":"string","minLength":1,"maxLength":64},"run_id":id,"request_id":{"type":"string","minLength":1,"maxLength":128}}), &["target_alias"]), true),
        definition("read_factory_agent_timeline", "Read observed agent timeline", "App-only Lanes read. For the run's owner or one of its worker threads, read a bounded, redacted timeline observed in Codex through the operator-configured loopback CAS binding. Observations are not Factory proof, never change run state, criteria, claims, budgets or attention, and never certify execution. Unconfigured or unreachable CAS returns an unavailable result with a reason.", object(json!({"run_id":id,"thread_id":{"type":"string","minLength":1,"maxLength":256},"cursor":{"type":"string","minLength":1,"maxLength":1024}}), &["run_id","thread_id"]), true),
        definition("propose_factory_change", "Propose graph change", "Record a source-bound, revision-fenced planning proposal under local operator authority. Imports remain candidates without execution authority. Inspect its returned ID/revision before applying; duplicate keys require identical payloads.", object(json!({"run_id":id,"expected_revision":revision,"idempotency_key":node_id,"change":change}), &["run_id","expected_revision","idempotency_key","change"]), false),
        definition("inspect_factory_issue_graph", "Inspect GitHub issue graph", "Opt-in read-only intake through Luna's least-privileged GitHub App. Reads one parent issue, its sub-issues, blocked-by relations, same-repository task-list references and documented dependencies in an allowlisted, Luna-approved repository. Returns candidate nodes with a forge identity kept apart from the source bound to the graph repository identity, native versus parsed derivation, dependencies documented but not native, acceptance candidates labelled derived_from_issue, rejected cycles, unimported cross-repository references, eligibility and complete or incomplete status. Writes nothing: importing still requires create_factory_graph, then propose_factory_change and apply_factory_change with explicit user confirmation. GitHub data is reported, never proof.", object(json!({"repository":{"type":"string","minLength":3,"maxLength":140,"pattern":"^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$"},"parent":{"type":"integer","minimum":1,"maximum":2_147_483_647_u64},"repository_alias":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]{1,64}$"}}), &["repository","parent"]), true),
        definition("read_factory_delivery", "Read GitHub delivery telemetry", "Opt-in read-only telemetry reported by GitHub for graph nodes with GitHub sources: linked pull requests, state, draft, head SHA, check runs, combined status, review decision and diff stats, each with observed_at. Cached per node for at least 60 seconds. Never satisfies criteria, never changes run state, actions or attention, and grants no merge or integration authority.", object(json!({"run_id":id}), &["run_id"]), true),
        definition("apply_factory_change", "Apply graph change", "Apply one inspected proposal at its current revision. Rechecks source, authority and backend preference. Does not dispatch or reassign active/unknown execution. Identical recorded retries do not apply twice.", object(json!({"run_id":id,"change_id":node_id,"expected_revision":revision}), &["run_id","change_id","expected_revision"]), false),
    ]);
    let token = json!({"type":"string","minLength":1,"maxLength":256,"pattern":"^\\S+$"});
    // Forge identity shared with #76: provider:owner/repo#number. Physical identity is the
    // approved repository alias, never caller-supplied.
    let forge = object(
        json!({"provider":{"type":"string","enum":["github","local","fixture"]},
            "repository":{"type":"string","minLength":3,"maxLength":201,"pattern":"^[A-Za-z0-9_-][A-Za-z0-9._-]*/[A-Za-z0-9_-][A-Za-z0-9._-]*$"},
            "number":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_991_u64},
            "node_id":token}),
        &["provider", "repository", "number"],
    );
    let parent = object(
        json!({"forge":forge,"revision":token,
            "display":{"type":"object","additionalProperties":false,"properties":{
                "url":{"type":"string","minLength":9,"maxLength":2048,"pattern":"^https://[^/@\\s]+/"}}}}),
        &["forge", "revision"],
    );
    let campaign_id = json!({"type":"string","minLength":1,"maxLength":64});
    tools.extend([
        definition("create_factory_campaign", "Create planning campaign", "Group one parent work item with a new planning-only graph in an approved repository. parent.forge is the item's forge identity (provider github|local|fixture, owner/repo, number); it and any display URL are supplied, not verified, and never evidence. A GitHub display URL must be https://github.com/{owner}/{repo}/issues/{number}. One campaign per forge identity and local repository; duplicate keys require identical payloads. Starts no claim, worker, inference or execution; continue planning with propose_factory_change on the returned graph.", object(json!({
            "repository":start["repository"],"parent":parent,"title":{"type":"string","minLength":1,"maxLength":1000},
            "objective":start["objective"],"acceptance":start["acceptance"],"non_goals":start["non_goals"],
            "finish":start["finish"],"profile":start["profile"],"idempotency_key":start["idempotency_key"]
        }), &["repository","parent","title","objective","acceptance","non_goals","finish","profile","idempotency_key"]), false),
        definition("list_factory_campaigns", "Factory campaigns", "Read recent campaigns with their supplied parent item, planning graph summary and linked runs, without inference. A planned campaign has no execution. Unreadable records are listed in skipped, never returned.", object(json!({"limit":{"type":"integer","minimum":1,"maximum":100}}), &[]), true),
        definition("get_factory_campaign", "Factory campaign", "Read one campaign, its planning graph summary (task counts by state and revision) and its linked runs, without inference.", object(json!({"campaign_id":campaign_id}), &["campaign_id"]), true),
        definition("promote_factory_campaign", "Promote campaign to execution", "Request promotion of a campaign's reviewed plan into execution, fenced to the planning graph revision. Input is validated first; a valid request then fails closed with execution_not_qualified while capabilities.execution.eligible is false. It never upgrades the planning graph and records nothing.", object(json!({"campaign_id":campaign_id,"expected_revision":revision,"idempotency_key":token}), &["campaign_id","expected_revision","idempotency_key"]), false),
    ]);
    let idempotency_key = json!({"type":"string","minLength":1,"maxLength":128});
    tools.extend([
        definition("list_factory_github_repositories", "List GitHub repositories", "Opt-in read-only list of the repositories Luna's GitHub App installation can see, one page at a time. Each entry says whether it is already connected (its approved alias) and whether it can be connected (allowlisted and the app has contents: read). Off unless the operator enabled workspaces.connect_github. GitHub data is reported, never proof.", object(json!({"cursor":{"type":"string","minLength":1,"maxLength":512}}), &[]), true),
        definition("connect_factory_github_repository", "Connect GitHub repository", "Clone one allowlisted repository from Luna's GitHub App installation into the operator's workspace root with a read-only token narrowed to that repository, and register it as an approved repository alias so it can be planned immediately. Operator opt-in (workspaces.connect_github). No push, branch, pull request, run, dispatch or execution. Retrying the same idempotency_key returns the same result.", object(json!({"repository":{"type":"string","minLength":3,"maxLength":140,"pattern":"^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$"},"idempotency_key":idempotency_key}), &["repository","idempotency_key"]), false),
        definition("create_factory_local_repository", "Create local repository", "Start a new local repository in the operator's workspace root: git init with one empty initial commit, registered as an approved repository alias. Operator opt-in (workspaces.create_local). No GitHub call, run, dispatch or execution. Retrying the same idempotency_key returns the same result.", object(json!({"name":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[a-z0-9][a-z0-9._-]{0,63}$"},"idempotency_key":idempotency_key}), &["name","idempotency_key"]), false),
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
            "read_factory_settings"
                | "update_factory_settings"
                | "discover_factory_repositories"
                | "read_factory_agent_timeline"
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
                | "inspect_factory_issue_graph"
                | "read_factory_delivery"
                | "propose_factory_change"
                | "apply_factory_change"
                | "create_factory_campaign"
                | "list_factory_campaigns"
                | "get_factory_campaign"
                | "promote_factory_campaign"
                | "list_factory_github_repositories"
                | "connect_factory_github_repository"
                | "create_factory_local_repository"
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
            "inspect_factory_issue_graph"
                | "read_factory_delivery"
                | "list_factory_github_repositories"
        ) {
            // Read-only, but it reads an external system (GitHub).
            tool.annotations = Some(serde_json::from_value(json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true})).expect("github read annotations"));
        }
        if matches!(
            name,
            "create_factory_graph"
                | "propose_factory_change"
                | "apply_factory_change"
                | "create_factory_campaign"
        ) {
            tool.annotations = Some(serde_json::from_value(json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false})).expect("planning annotations"));
        }
        if matches!(
            name,
            "connect_factory_github_repository" | "create_factory_local_repository"
        ) {
            // Writes only under the operator's workspace root; connecting reads GitHub.
            let open_world = name == "connect_factory_github_repository";
            tool.annotations = Some(serde_json::from_value(json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":open_world})).expect("workspace annotations"));
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
