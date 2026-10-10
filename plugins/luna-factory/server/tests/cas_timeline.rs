//! App-only agent timeline against a loopback fake CAS. Never starts native workers
//! and never contacts a real CAS or Codex daemon.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};
use luna_factoryd::{
    config::Config,
    http::McpServer,
    lifecycle::Factory,
    store::{StartRequest, Store},
    timeline::AgentTimelines,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;

const SNAPSHOT: &str = "/api/actions/codex-action-server/get-thread-snapshot/run";
const ITEMS: &str = "/api/actions/codex-action-server/list-thread-items/run";
const READS: [&str; 7] = [
    "/api/actions/codex-action-server/inspect-target/run",
    "/api/actions/codex-action-server/read-dispatch-receipt/run",
    "/api/actions/codex-action-server/read-thread/run",
    SNAPSHOT,
    ITEMS,
    "/api/actions/codex-action-server/list-thread-turns/run",
    "/api/actions/codex-action-server/list-thread-timeline/run",
];
const TARGET: &str = "trusted-target";
const CWD: &str = "/operator/approved/repository";

#[derive(Clone, Debug)]
struct Call {
    method: String,
    path: String,
    body: Value,
}
#[derive(Clone)]
struct Reply {
    status: StatusCode,
    body: String,
}
#[derive(Clone, Default)]
struct HttpState {
    calls: Arc<Mutex<Vec<Call>>>,
    replies: Arc<Mutex<BTreeMap<String, Reply>>>,
}
struct CasFixture {
    endpoint: String,
    state: HttpState,
    server: JoinHandle<()>,
}
impl Drop for CasFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl CasFixture {
    async fn start() -> Self {
        let state = HttpState::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(respond).with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            endpoint,
            state,
            server,
        }
    }
    fn reply(&self, path: &str, status: StatusCode, body: impl Into<String>) {
        self.state.replies.lock().unwrap().insert(
            path.into(),
            Reply {
                status,
                body: body.into(),
            },
        );
    }
    fn json(&self, path: &str, body: Value) {
        self.reply(path, StatusCode::OK, body.to_string());
    }
    fn calls(&self) -> Vec<Call> {
        self.state.calls.lock().unwrap().clone()
    }
    fn assert_read_only(&self) {
        for call in self.calls() {
            assert_eq!(call.method, "POST");
            assert!(
                READS.contains(&call.path.as_str()),
                "unexpected action: {}",
                call.path
            );
            let payload = &call.body["payload"];
            if call.path == SNAPSHOT || call.path == ITEMS {
                assert_eq!(payload["target"], TARGET, "binding must come from config");
                assert_eq!(payload["cwd"], CWD, "binding must come from config");
            }
        }
    }
}
async fn respond(State(state): State<HttpState>, request: Request<Body>) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.calls.lock().unwrap().push(Call {
        method,
        path: path.clone(),
        body,
    });
    let reply = state
        .replies
        .lock()
        .unwrap()
        .get(&path)
        .cloned()
        .unwrap_or(Reply {
            status: StatusCode::NOT_FOUND,
            body: "unconfigured fixture action".into(),
        });
    Response::builder()
        .status(reply.status)
        .header("content-type", "application/json")
        .body(Body::from(reply.body))
        .unwrap()
}

fn config(cas: &CasFixture, temp: &tempfile::TempDir, bound: bool) -> Config {
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let mut value = json!({
        "listen":"127.0.0.1:8787","database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/absent-cas-timeline-native","skill_path":temp.path().join("SKILL.md"),
        "repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},"limits":{"capacity":2,"repair_attempts":1,"wall_seconds":60},
        "cas_targets":{"fixture":{"endpoint":cas.endpoint,"package":"codex-action-server","target":TARGET,"cwd":CWD}}
    });
    if bound {
        value["cas_timelines"] = json!({"fixture":"fixture"});
    }
    let config: Config = serde_json::from_value(value).unwrap();
    config.validate().unwrap();
    config
}
/// An admitted run with an owner and one owned, active worker. No native process exists.
fn seed_run(config: &Config, key: &str) -> luna_factoryd::store::Run {
    let request: StartRequest = serde_json::from_value(json!({"repository":"fixture","objective":"Observe agents without proof","acceptance":["Independently observed proof"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":2,"repair_attempts":1,"wall_seconds":60,"idempotency_key":key})).unwrap();
    let mut store = Store::open(config).unwrap();
    let mut run = store.admit(config, &request).unwrap().run;
    run.thread_id = Some("owner-thread".into());
    run.turn_id = Some("turn-2".into());
    run.owned_threads = vec!["worker-1".into()];
    run.active_threads = vec!["worker-1".into()];
    store.save(&mut run).unwrap();
    store.get(&run.id).unwrap()
}
fn server(config: &Config) -> McpServer {
    McpServer {
        factory: Factory::new(config.clone()).unwrap(),
        html: Arc::new(String::new()),
    }
}
fn connection() -> Value {
    json!({"target":TARGET,"codex_bin":"/private/operator/bin/codex","socket":"/private/operator/native.sock"})
}
fn snapshot(thread: &str, revision: char, waiting: bool) -> Value {
    json!({"result":{"operation":"get_thread_snapshot","connection":connection(),"result":{
        "target":TARGET,"cwd":CWD,"thread_id":thread,"source":"native-app-server","thread_status":"active",
        "active_flags":if waiting {json!(["waitingOnApproval"])} else {json!([])},"unknown_active_flags":false,
        "native_updated_at":1_760_000_900,"native_updated_at_unknown":false,
        "latest_turn":{"id":"turn-2","status":"failed","error_code":"rateLimitExceeded"},
        "latest_item":{"id":"m","kind":"agentMessage","phase":null,"status":null,"text":"private-latest-item-sentinel","text_truncated":false},
        "continuation":{"turn_id":"turn-2","item_cursor":"x","item_cursor_omitted":false,"item_cursor_digest":null},
        "observed_at":"2026-10-10T00:00:00.000Z","revision":revision.to_string().repeat(64),"changed":true,"native_state_authoritative":true
    },"effective_configuration":null,"receipts":[],"events":[]},"error":null})
}
fn unchanged(thread: &str, revision: char) -> Value {
    json!({"result":{"operation":"get_thread_snapshot","connection":connection(),"result":{
        "target":TARGET,"cwd":CWD,"thread_id":thread,"observed_at":"2026-10-10T00:00:30.000Z","revision":revision.to_string().repeat(64),
        "changed":false,"native_state_authoritative":true,"thread_status":"active","active_flags":["waitingOnApproval"],
        "latest_turn":{"id":"turn-2","status":"failed","error_code":"rateLimitExceeded"}
    },"effective_configuration":null,"receipts":[],"events":[]},"error":null})
}
fn entry(seconds: u64, item: Value) -> Value {
    json!({"turnId":"turn-2","startedAtMs":(seconds - 5) * 1000,"completedAtMs":seconds * 1000,"item":item})
}
/// Newest first, as requested with sort_direction=desc. Every private field carries a sentinel.
fn items(cursor: Option<&str>) -> Value {
    let data = vec![
        entry(
            1_760_000_880,
            json!({"id":"i10","type":"agentMessage","text":"Pushing with ghp_privatetokensentinel now"}),
        ),
        entry(
            1_760_000_860,
            json!({"id":"i9","type":"agentMessage","phase":"commentary","text":"Tests pass; opening the pull request next.\nprivate-second-line-sentinel"}),
        ),
        entry(
            1_760_000_840,
            json!({"id":"i8","type":"commandExecution","command":"/bin/bash -lc 'gh pr create --title private-title-sentinel'","cwd":"/private/cwd-sentinel","status":"completed","exitCode":0,"aggregatedOutput":"https://github.invalid/private-output-sentinel/pull/1"}),
        ),
        entry(
            1_760_000_820,
            json!({"id":"i7","type":"commandExecution","command":"git commit -m private-commit-message-sentinel","status":"completed","exitCode":0,"aggregatedOutput":"[branch abc] private-output-sentinel"}),
        ),
        entry(
            1_760_000_800,
            json!({"id":"i6","type":"commandExecution","command":"cd /private/path-sentinel && API_KEY=private-env-sentinel cargo test --locked","status":"completed","exitCode":101,"aggregatedOutput":"-----BEGIN PRIVATE KEY----- private-output-sentinel"}),
        ),
        entry(
            1_760_000_780,
            json!({"id":"i5","type":"fileChange","status":"completed","changes":[{"path":"/private/path-sentinel/a.rs","kind":{"type":"update"},"diff":"private-diff-sentinel"},{"path":"/private/path-sentinel/b.rs","kind":{"type":"add"},"diff":"private-file-contents-sentinel"}]}),
        ),
        entry(
            1_760_000_760,
            json!({"id":"i4","type":"collabAgentToolCall","tool":"spawnAgent","status":"completed","senderThreadId":"owner-thread","receiverThreadIds":["worker-1"],"prompt":"private-prompt-sentinel","model":"gpt-6-luna","agentsStates":{"worker-1":{"status":"running","message":"private-state-sentinel"}}}),
        ),
        entry(
            1_760_000_740,
            json!({"id":"i3","type":"reasoning","summary":["private-reasoning-sentinel"],"content":[]}),
        ),
        entry(
            1_760_000_730,
            json!({"id":"i2","type":"futureNativeItem","payload":"private-unknown-sentinel"}),
        ),
        entry(
            1_760_000_720,
            json!({"id":"i1","type":"userMessage","content":[{"type":"text","text":"private-instructions-sentinel"}]}),
        ),
    ];
    let next = if cursor.is_some() {
        Value::Null
    } else {
        json!("older-page-1")
    };
    json!({"result":{"operation":"thread/items/list","connection":connection(),"result":{"data":data,"nextCursor":next,"backwardsCursor":null},
        "effective_configuration":null,"receipts":[{"private":"private-receipt-sentinel"}],"events":[{"method":"item/agentMessage/delta","params":{"delta":"private-event-sentinel"}}]},"error":null})
}
fn validate(output: &Value) {
    let tool = luna_factoryd::mcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "read_factory_agent_timeline")
        .expect("timeline tool must be advertised");
    let schema = serde_json::to_value(tool.output_schema.expect("output schema")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(output)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema errors: {}", errors.join("; "));
    assert_eq!(output["binding_verified"], false);
    assert_eq!(output["execution_eligible"], false);
    assert_eq!(output["source"], "codex-action-server");
    let text = output.to_string();
    assert!(
        text.len() < 64 * 1024,
        "timeline projection must stay bounded"
    );
    for private in ["sentinel", "/private/", "BEGIN PRIVATE", "ghp_"] {
        assert!(!text.contains(private), "timeline leaked {private}: {text}");
    }
}
/// Every table of the durable ledger, row for row.
fn ledger(config: &Config) -> Vec<(String, Vec<Vec<String>>)> {
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    [
        "runs",
        "claims",
        "receipts",
        "control_events",
        "settings",
        "repository_registrations",
    ]
    .into_iter()
    .map(|table| {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get_ref(index).map(|value| format!("{value:?}")))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (table.to_owned(), rows)
    })
    .collect()
}

#[tokio::test]
async fn unconfigured_repository_returns_a_structured_unavailable_result_without_http() {
    let cas = CasFixture::start().await;
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, false);
    let server = server(&config);
    let run = seed_run(&config, "unbound");
    let output = server
        .invoke(
            "read_factory_agent_timeline",
            json!({"run_id":run.id,"thread_id":"owner-thread"}),
        )
        .await
        .expect("unconfigured CAS is a normal result, not an error");
    validate(&output);
    assert_eq!(output["observed"], false);
    assert_eq!(output["status"], "unavailable");
    assert_eq!(output["reason"], "cas_timeline_not_configured");
    assert!(
        output["detail"]
            .as_str()
            .unwrap()
            .contains("not configured")
    );
    let capabilities = server
        .invoke("get_factory_capabilities", json!({}))
        .await
        .unwrap();
    assert_eq!(capabilities["agent_timeline"]["enabled"], false);
    assert!(cas.calls().is_empty());
}

#[tokio::test]
async fn only_factory_known_threads_resolve_and_callers_cannot_choose_scope() {
    let cas = CasFixture::start().await;
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, true);
    let server = server(&config);
    let run = seed_run(&config, "scope");
    let planning: StartRequest = serde_json::from_value(json!({"repository":"fixture","objective":"Plan only","acceptance":["Later"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"plan"})).unwrap();
    let plan = Store::open(&config)
        .unwrap()
        .admit_planning(&config, &planning)
        .unwrap()
        .run;
    for args in [
        json!({"run_id":run.id,"thread_id":"foreign-thread"}),
        json!({"run_id":plan.id,"thread_id":"owner-thread"}),
        json!({"run_id":"missing-run","thread_id":"owner-thread"}),
        json!({"run_id":run.id,"thread_id":"owner-thread","target":"other-target"}),
        json!({"run_id":run.id,"thread_id":"owner-thread","cwd":"/other"}),
        json!({"run_id":run.id,"thread_id":"owner-thread","endpoint":"http://192.0.2.1"}),
        json!({"run_id":run.id,"thread_id":"owner-thread","cursor":"never-issued"}),
        json!({"run_id":run.id,"thread_id":"owner-thread","cursor":"has space"}),
        json!({"run_id":run.id,"thread_id":""}),
    ] {
        assert!(
            server
                .invoke("read_factory_agent_timeline", args.clone())
                .await
                .is_err(),
            "accepted {args}"
        );
    }
    assert!(cas.calls().is_empty(), "rejected scope reached CAS");
    let tool = luna_factoryd::mcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "read_factory_agent_timeline")
        .unwrap();
    let value = serde_json::to_value(tool).unwrap();
    assert_eq!(value["_meta"]["ui"]["visibility"], json!(["app"]));
    assert_eq!(value["annotations"]["readOnlyHint"], true);
    assert_eq!(value["annotations"]["destructiveHint"], false);
    assert_eq!(
        value["inputSchema"]["required"],
        json!(["run_id", "thread_id"])
    );
    assert_eq!(value["inputSchema"]["additionalProperties"], false);
}

#[tokio::test]
async fn native_items_normalize_to_a_redacted_vocabulary_with_children() {
    let cas = CasFixture::start().await;
    cas.json(SNAPSHOT, snapshot("owner-thread", 'a', true));
    cas.json(ITEMS, items(None));
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, true);
    let server = server(&config);
    let run = seed_run(&config, "normalize");
    let output = server
        .invoke(
            "read_factory_agent_timeline",
            json!({"run_id":run.id,"thread_id":"owner-thread"}),
        )
        .await
        .unwrap();
    validate(&output);
    assert_eq!(output["observed"], true);
    assert_eq!(output["status"], "observed");
    let events: Vec<_> = output["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            assert_eq!(event["thread_id"], "owner-thread");
            (
                event["at"].as_u64().unwrap(),
                event["kind"].as_str().unwrap().to_owned(),
                event["summary"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected: Vec<(u64, &str, &str)> = vec![
        (1_760_000_720, "assigned", "Received instructions"),
        (1_760_000_760, "subagent_spawned", "Spawned a helper agent"),
        (1_760_000_780, "file_change", "Changed 2 files"),
        (
            1_760_000_800,
            "check_result",
            "Check failed: cargo test (exit 101)",
        ),
        (1_760_000_820, "commit", "Committed changes with git commit"),
        (
            1_760_000_840,
            "pr_opened",
            "Opened a pull request with gh pr create",
        ),
        (
            1_760_000_860,
            "message",
            "Tests pass; opening the pull request next.",
        ),
        (
            1_760_000_880,
            "message",
            "Sensitive details withheld. Review the native owner thread locally.",
        ),
        (
            1_760_000_900,
            "waiting",
            "Waiting on a native approval in Codex",
        ),
        (
            1_760_000_900,
            "error",
            "Latest turn failed: rate limit exceeded",
        ),
    ];
    assert_eq!(
        events,
        expected
            .into_iter()
            .map(|(at, kind, summary)| (at, kind.to_owned(), summary.to_owned()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        output["omitted"], 2,
        "reasoning and unknown items are dropped"
    );
    assert_eq!(
        output["children"],
        json!([{"parent_thread":"owner-thread","receiver_thread":"worker-1"}])
    );
    assert_eq!(output["next_cursor"], "older-page-1");
    assert_eq!(output["freshness"]["cache"], "miss");
    assert_eq!(output["freshness"]["native_updated_at"], 1_760_000_900);
    assert_eq!(output["freshness"]["thread_status"], "active");
    let calls = cas.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        vec![SNAPSHOT, ITEMS]
    );
    assert_eq!(
        calls[0].body,
        json!({"payload":{"target":TARGET,"cwd":CWD,"thread_id":"owner-thread"}})
    );
    assert_eq!(
        calls[1].body,
        json!({"payload":{"target":TARGET,"cwd":CWD,"thread_id":"owner-thread","limit":100,"sort_direction":"desc"}})
    );
    // A worker thread resolves through the same exact binding.
    cas.json(SNAPSHOT, snapshot("worker-1", 'b', false));
    let worker = server
        .invoke(
            "read_factory_agent_timeline",
            json!({"run_id":run.id,"thread_id":"worker-1"}),
        )
        .await
        .unwrap();
    validate(&worker);
    assert_eq!(worker["thread_id"], "worker-1");
    cas.assert_read_only();
}

#[tokio::test]
async fn polling_is_served_from_the_per_thread_cache_and_revalidated_by_revision() {
    let cas = CasFixture::start().await;
    cas.json(SNAPSHOT, snapshot("owner-thread", 'a', true));
    cas.json(ITEMS, items(None));
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, true);
    let server = server(&config);
    let run = seed_run(&config, "cache");
    let args = json!({"run_id":run.id,"thread_id":"owner-thread"});
    let first = server
        .invoke("read_factory_agent_timeline", args.clone())
        .await
        .unwrap();
    for _ in 0..5 {
        let again = server
            .invoke("read_factory_agent_timeline", args.clone())
            .await
            .unwrap();
        assert_eq!(again["freshness"]["cache"], "hit");
        assert_eq!(again["events"], first["events"]);
    }
    assert_eq!(cas.calls().len(), 2, "polling inside the window called CAS");

    // With an expired window, an unchanged revision costs exactly one snapshot read.
    let timelines = AgentTimelines::new(Duration::ZERO);
    let target = &config.cas_targets["fixture"];
    let baseline = cas.calls().len();
    let fresh = timelines
        .read("fixture", target, &run.id, "owner-thread", None)
        .await
        .unwrap();
    validate(&fresh);
    assert_eq!(cas.calls().len(), baseline + 2);
    cas.json(SNAPSHOT, unchanged("owner-thread", 'a'));
    let revalidated = timelines
        .read("fixture", target, &run.id, "owner-thread", None)
        .await
        .unwrap();
    validate(&revalidated);
    assert_eq!(revalidated["freshness"]["cache"], "revalidated");
    assert_eq!(revalidated["events"], fresh["events"]);
    let calls = cas.calls();
    assert_eq!(calls.len(), baseline + 3);
    assert_eq!(calls.last().unwrap().path, SNAPSHOT);
    assert_eq!(
        calls.last().unwrap().body["payload"]["revision"],
        "a".repeat(64)
    );
    // An issued cursor pages older items; an unissued one never reaches CAS.
    cas.json(SNAPSHOT, snapshot("owner-thread", 'c', false));
    cas.json(ITEMS, items(Some("older-page-1")));
    let older = timelines
        .read(
            "fixture",
            target,
            &run.id,
            "owner-thread",
            Some("older-page-1"),
        )
        .await
        .unwrap();
    validate(&older);
    assert!(older["next_cursor"].is_null());
    assert_eq!(
        cas.calls().last().unwrap().body["payload"]["cursor"],
        "older-page-1"
    );
    let before = cas.calls().len();
    assert!(
        timelines
            .read("fixture", target, &run.id, "owner-thread", Some("forged"))
            .await
            .is_err()
    );
    assert_eq!(cas.calls().len(), before);
    cas.assert_read_only();
}

#[tokio::test]
async fn remote_failures_are_bounded_unavailable_results_and_are_not_retried_in_the_window() {
    let cas = CasFixture::start().await;
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, true);
    let run = seed_run(&config, "failures");
    let target = &config.cas_targets["fixture"];
    let mut foreign_cwd = snapshot("owner-thread", 'a', false);
    foreign_cwd["result"]["result"]["cwd"] = json!("/private/foreign-sentinel");
    let mut foreign_thread = snapshot("owner-thread", 'a', false);
    foreign_thread["result"]["result"]["thread_id"] = json!("private-thread-sentinel");
    let mut foreign_target = snapshot("owner-thread", 'a', false);
    foreign_target["result"]["connection"]["target"] = json!("private-target-sentinel");
    let cases = [
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "private-remote-failure-sentinel".to_owned(),
            "cas_unavailable",
        ),
        (
            StatusCode::OK,
            json!({"result":null,"error":"private-remote-error-sentinel"}).to_string(),
            "cas_action_failed",
        ),
        (
            StatusCode::OK,
            "not-json-private-sentinel".to_owned(),
            "cas_invalid_response",
        ),
        (
            StatusCode::OK,
            foreign_cwd.to_string(),
            "cas_binding_mismatch",
        ),
        (
            StatusCode::OK,
            foreign_thread.to_string(),
            "cas_binding_mismatch",
        ),
        (
            StatusCode::OK,
            foreign_target.to_string(),
            "cas_binding_mismatch",
        ),
        (
            StatusCode::OK,
            "private-sentinel".repeat(128 * 1024),
            "cas_response_too_large",
        ),
    ];
    for (status, body, expected) in cases {
        cas.reply(SNAPSHOT, status, body);
        let timelines = AgentTimelines::new(Duration::from_secs(60));
        let before = cas.calls().len();
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            timelines.read("fixture", target, &run.id, "owner-thread", None),
        )
        .await
        .expect("CAS reads must be time-bounded")
        .expect("remote failure is an unavailable result");
        validate(&output);
        assert_eq!(output["observed"], false);
        assert_eq!(output["reason"], expected);
        assert_eq!(
            cas.calls().len(),
            before + 1,
            "failed snapshot must stop the read"
        );
        let again = timelines
            .read("fixture", target, &run.id, "owner-thread", None)
            .await
            .unwrap();
        assert_eq!(again["reason"], expected);
        assert_eq!(
            cas.calls().len(),
            before + 1,
            "failure retried inside the window"
        );
    }
    // A malformed item page after a valid snapshot is withheld too.
    cas.json(SNAPSHOT, snapshot("owner-thread", 'a', false));
    let mut bad_items = items(None);
    bad_items["result"]["operation"] = json!("thread/turns/list");
    cas.json(ITEMS, bad_items);
    let output = AgentTimelines::new(Duration::ZERO)
        .read("fixture", target, &run.id, "owner-thread", None)
        .await
        .unwrap();
    validate(&output);
    assert_eq!(output["reason"], "cas_binding_mismatch");
    cas.assert_read_only();
}

#[tokio::test]
async fn observations_never_write_run_state_criteria_claims_attempts_budgets_or_receipts() {
    let cas = CasFixture::start().await;
    cas.json(SNAPSHOT, snapshot("owner-thread", 'a', true));
    cas.json(ITEMS, items(None));
    let temp = tempfile::tempdir().unwrap();
    let config = config(&cas, &temp, true);
    let server = server(&config);
    let run = seed_run(&config, "observe-only");
    let read_run = |id: String| {
        let server = &server;
        async move {
            server
                .invoke("get_factory_run", json!({"run_id":id}))
                .await
                .unwrap()
        }
    };
    let run_before = read_run(run.id.clone()).await;
    let ledger_before = ledger(&config);
    let receipts_before = Store::open(&config).unwrap().receipts(&run.id).unwrap();
    // Observed commits, PR creation, check results, approvals and failures are
    // all present in these pages; none of them may become Factory state.
    for thread in ["owner-thread", "worker-1"] {
        cas.json(SNAPSHOT, snapshot(thread, 'a', true));
        let output = AgentTimelines::new(Duration::ZERO)
            .read(
                "fixture",
                &config.cas_targets["fixture"],
                &run.id,
                thread,
                None,
            )
            .await
            .unwrap();
        assert_eq!(output["observed"], true);
        server
            .invoke(
                "read_factory_agent_timeline",
                json!({"run_id":run.id,"thread_id":thread}),
            )
            .await
            .unwrap();
    }
    cas.reply(SNAPSHOT, StatusCode::BAD_GATEWAY, "private-sentinel");
    AgentTimelines::new(Duration::ZERO)
        .read(
            "fixture",
            &config.cas_targets["fixture"],
            &run.id,
            "owner-thread",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        ledger(&config),
        ledger_before,
        "a timeline observation wrote the durable ledger"
    );
    assert_eq!(
        Store::open(&config).unwrap().receipts(&run.id).unwrap(),
        receipts_before
    );
    let run_after = read_run(run.id.clone()).await;
    assert_eq!(
        run_after, run_before,
        "observation changed the run projection"
    );
    for field in [
        "state",
        "claim_held",
        "repairs_used",
        "deadline_at",
        "blocker",
    ] {
        assert_eq!(run_after[field], run_before[field], "{field}");
    }
    assert_eq!(run_after["control"], run_before["control"]);
    assert_eq!(run_after["presentation"], run_before["presentation"]);
    cas.assert_read_only();
}
