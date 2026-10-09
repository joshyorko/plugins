//! Read-only CAS boundary against a loopback HTTP fixture. Never starts native workers.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};
use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::task::JoinHandle;

const INSPECT: &str = "/api/actions/codex-action-server/inspect-target/run";
const RECEIPT: &str = "/api/actions/codex-action-server/read-dispatch-receipt/run";
const THREAD: &str = "/api/actions/codex-action-server/read-thread/run";

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
                [INSPECT, RECEIPT, THREAD].contains(&call.path.as_str()),
                "unexpected action: {}",
                call.path
            );
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
fn setup(cas: &CasFixture) -> (tempfile::TempDir, Config, McpServer) {
    let temp = tempfile::tempdir().unwrap();
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
    let config: Config=serde_json::from_value(json!({
        "listen":"127.0.0.1:8787","database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/absent-cas-boundary-native","skill_path":temp.path().join("SKILL.md"),
        "repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},"limits":{"capacity":1,"repair_attempts":1,"wall_seconds":60},
        "cas_targets":{"fixture":{"endpoint":cas.endpoint,"package":"codex-action-server","target":"trusted-target","cwd":"/operator/approved/repository"}}
    })).unwrap();
    let server = McpServer {
        factory: Factory::new(config.clone()).unwrap(),
        html: Arc::new(String::new()),
    };
    (temp, config, server)
}

#[tokio::test]
async fn unapproved_target_and_incomplete_binding_fail_before_http() {
    let cas = CasFixture::start().await;
    let (_temp, _config, server) = setup(&cas);
    for args in [
        json!({"target_alias":"unknown"}),
        json!({"target_alias":"fixture","run_id":"run"}),
        json!({"target_alias":"fixture","request_id":"request"}),
        json!({"target_alias":"fixture","run_id":"run","request_id":"request"}),
        json!({"target_alias":"fixture","endpoint":"http://untrusted.invalid"}),
        json!({"target_alias":"fixture","cwd":"/untrusted"}),
    ] {
        assert!(
            server
                .invoke("inspect_factory_cas", args.clone())
                .await
                .is_err(),
            "unexpected acceptance: {args}"
        );
    }
    assert!(cas.calls().is_empty());
}

fn resolved_target() -> Value {
    json!({"result":{"target":"trusted-target","status":"resolved","destination":"operator-secret-host","codex_bin":"/private/operator/bin/codex","socket":"/private/operator/native.sock","connectivity":"not_probed"},"error":null})
}
fn validate_output(output: &Value) {
    let tool = luna_factoryd::mcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "inspect_factory_cas")
        .expect("CAS model tool must be advertised");
    let schema =
        serde_json::to_value(tool.output_schema.expect("CAS output schema required")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(output)
        .map(|error| error.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "CAS output schema errors: {}",
        errors.join("; ")
    );
    assert!(
        serde_json::to_vec(output).unwrap().len() < 16 * 1024,
        "public CAS projection should remain bounded"
    );
}
#[tokio::test]
async fn target_inspection_uses_only_exact_read_action_and_redacts_operator_transport_details() {
    let cas = CasFixture::start().await;
    cas.json(INSPECT, resolved_target());
    let (_temp, _config, server) = setup(&cas);
    let output = server
        .invoke("inspect_factory_cas", json!({"target_alias":"fixture"}))
        .await
        .unwrap();
    validate_output(&output);
    let text = output.to_string();
    for private in [
        "operator-secret-host",
        "/private/operator/bin/codex",
        "/private/operator/native.sock",
    ] {
        assert!(
            !text.contains(private),
            "inspection exposed trusted transport details"
        );
    }
    let calls = cas.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, INSPECT);
    assert_eq!(
        calls[0].body,
        json!({"payload":{"target":"trusted-target"}})
    );
    cas.assert_read_only();
}

#[tokio::test]
async fn malformed_remote_errors_oversized_bodies_and_target_mismatches_fail_bounded() {
    let cas = CasFixture::start().await;
    let (_temp, _config, server) = setup(&cas);
    let mut wrong_target = resolved_target();
    wrong_target["result"]["target"] = json!("unapproved-target");
    let cases = [
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "secret-sentinel remote failure".to_owned(),
        ),
        (StatusCode::OK, "not-json-secret-sentinel".to_owned()),
        (
            StatusCode::OK,
            json!({"result":null,"error":"secret-sentinel remote error"}).to_string(),
        ),
        (
            StatusCode::OK,
            json!({"result":["secret-sentinel"],"error":null}).to_string(),
        ),
        (StatusCode::OK, wrong_target.to_string()),
        (StatusCode::OK, "secret-sentinel".repeat(512 * 1024)),
    ];
    for (status, body) in cases {
        cas.reply(INSPECT, status, body);
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.invoke("inspect_factory_cas", json!({"target_alias":"fixture"})),
        )
        .await
        .expect("CAS read must be time-bounded")
        .expect_err("malformed CAS response accepted");
        let message = error.to_string();
        assert!(message.len() < 1000, "unbounded remote error");
        assert!(
            !message.contains("secret-sentinel"),
            "remote error body leaked"
        );
    }
    assert_eq!(
        cas.calls().len(),
        6,
        "reads must not retry or redispatch after errors"
    );
    cas.assert_read_only();
}

fn seed_plan(
    config: &Config,
) -> (
    luna_factoryd::store::Run,
    luna_factoryd::cas_boundary::CasRequest,
) {
    use luna_factoryd::{
        cas::PreparedDispatch,
        cas_boundary::CasRequest,
        control::{Event, EventEnvelope},
        store::{StartRequest, Store},
    };
    let request:StartRequest=serde_json::from_value(json!({"repository":"fixture","objective":"Record bounded CAS planning identity","acceptance":["Current independently observed proof"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"cas-plan"})).unwrap();
    let mut store = Store::open(config).unwrap();
    let mut run = store.admit_planning(config, &request).unwrap().run;
    let planned=CasRequest{
        prepared:PreparedDispatch::new(config.cas_targets["fixture"].clone(),json!({"text":"private-prompt-sentinel","target":"trusted-target","cwd":"/operator/approved/repository"})).unwrap(),
        target_alias:"fixture".into(),task_id:"objective".into(),subject:run.current_subject.clone(),repository_stamp:run.graph_repository_stamp.clone().unwrap(),
    };
    let event = EventEnvelope {
        id: planned.prepared.request_id.clone(),
        expected_revision: run.control.as_ref().unwrap().revision,
        event: Event::CasPlanned {
            request: planned.clone(),
        },
    };
    assert!(store.apply_event(&mut run, &event).unwrap());
    assert!(
        !store.apply_event(&mut run, &event).unwrap(),
        "exact planning retry must be idempotent"
    );
    (run, planned)
}
fn receipt(request_id: &str, state: &str) -> Value {
    if state == "not_found" {
        return json!({"result":{"request_id":request_id,"state":state},"error":null});
    }
    let has_thread = matches!(
        state,
        "accepted" | "completed" | "failed" | "interrupted" | "created_not_materialized"
    );
    let has_turn = has_thread && state != "created_not_materialized";
    json!({"result":{"request_id":request_id,"state":state,"thread_id":if has_thread{Some("remote-thread")}else{None},"turn_id":if has_turn{Some("remote-turn")}else{None},"replayed":false,"retry_same_request_id":"returns_receipt_without_dispatch","native_state_authoritative":true,"wait_mode":"dispatch"},"error":null})
}
fn thread_snapshot() -> Value {
    json!({"result":{"operation":"thread/read","connection":{"target":"trusted-target","codex_bin":"/private/operator/bin/codex"},"result":{"thread":{"id":"remote-thread","cwd":"/operator/approved/repository","status":{"type":"idle"},"turns":[{"id":"remote-turn","status":"completed","items":[{"type":"agentMessage","text":"private-native-text-sentinel"}]}]}},"events":[],"receipts":[]},"error":null})
}
#[tokio::test]
async fn persisted_request_survives_reopen_and_all_receipt_states_remain_observations() {
    use luna_factoryd::store::Store;
    let cas = CasFixture::start().await;
    cas.json(INSPECT, resolved_target());
    cas.json(THREAD, thread_snapshot());
    let (_temp, config, server) = setup(&cas);
    let (run, planned) = seed_plan(&config);
    let before = serde_json::to_value(&run).unwrap();
    drop(server);
    let reopened = Store::open(&config).unwrap().get(&run.id).unwrap();
    assert_eq!(
        reopened.control.as_ref().unwrap().cas_requests[&planned.prepared.request_id],
        planned
    );
    let server = McpServer {
        factory: Factory::new(config.clone()).unwrap(),
        html: Arc::new(String::new()),
    };
    // An unrelated admitted run owns the same repository. CAS reads must not
    // settle its UNKNOWN execution or release/steal its claim.
    let mut store = Store::open(&config).unwrap();
    let mut request = run.request.clone();
    request.idempotency_key = "unrelated-held-run".into();
    let mut held = store.admit(&config, &request).unwrap().run;
    held.set_state(luna_factoryd::control::RunControl::Unknown);
    store.save(&mut held).unwrap();
    let held_before = serde_json::to_value(&held).unwrap();
    drop(store);
    for state in [
        "not_found",
        "unknown",
        "in_progress",
        "created_not_materialized",
        "accepted",
        "completed",
        "failed",
        "interrupted",
    ] {
        cas.json(RECEIPT, receipt(&planned.prepared.request_id, state));
        let output=server.invoke("inspect_factory_cas",json!({"target_alias":"fixture","run_id":run.id,"request_id":planned.prepared.request_id})).await.unwrap();
        validate_output(&output);
        assert_eq!(output["receipt"]["state"], state);
        assert_eq!(output["receipt"]["binding_verified"], false);
        assert_eq!(output["receipt"]["execution_eligible"], false);
        assert_eq!(output["qualification"]["execution_eligible"], false);
        if output["thread"].is_object() {
            assert_eq!(output["thread"]["liveness"], "idle");
            assert_eq!(output["thread"]["execution_eligible"], false);
        }
        for secret in [
            "private-prompt-sentinel",
            "private-native-text-sentinel",
            "/private/operator/bin/codex",
        ] {
            assert!(!output.to_string().contains(secret));
        }
        let after = Store::open(&config).unwrap().get(&run.id).unwrap();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            before,
            "receipt state {state} mutated run/claim/evidence"
        );
        let held_after = Store::open(&config).unwrap().get(&held.id).unwrap();
        assert_eq!(
            held_after.observed_claim,
            luna_factoryd::store::ObservedClaim::Owned
        );
        assert_eq!(
            serde_json::to_value(held_after).unwrap(),
            held_before,
            "CAS read mutated unrelated claimed run"
        );
    }
    let calls = cas.calls();
    assert_eq!(calls.iter().filter(|call| call.path == INSPECT).count(), 8);
    assert_eq!(calls.iter().filter(|call| call.path == RECEIPT).count(), 8);
    assert_eq!(calls.iter().filter(|call| call.path == THREAD).count(), 5);
    for call in calls {
        match call.path.as_str() {
            RECEIPT => assert_eq!(
                call.body,
                json!({"payload":{"request_id":planned.prepared.request_id}})
            ),
            THREAD => assert_eq!(
                call.body,
                json!({"payload":{"target":"trusted-target","cwd":"/operator/approved/repository","thread_id":"remote-thread","include_turns":false}})
            ),
            _ => {}
        }
    }
    cas.assert_read_only();
}
#[tokio::test]
async fn configured_binding_drift_and_source_changes_reject_before_any_http() {
    let cas = CasFixture::start().await;
    let (_temp, config, server) = setup(&cas);
    let (run, planned) = seed_plan(&config);
    drop(server);
    for field in ["endpoint", "package", "target", "cwd"] {
        let mut changed = config.clone();
        let target = changed.cas_targets.get_mut("fixture").unwrap();
        match field {
            "endpoint" => target.endpoint = "http://127.0.0.1:9".into(),
            "package" => target.package = "codex-observe".into(),
            "target" => target.target = "different-approved-target".into(),
            "cwd" => target.cwd = "/different/approved/repository".into(),
            _ => unreachable!(),
        }
        let server = McpServer {
            factory: Factory::new(changed).unwrap(),
            html: Arc::new(String::new()),
        };
        assert!(server.invoke("inspect_factory_cas",json!({"target_alias":"fixture","run_id":run.id,"request_id":planned.prepared.request_id})).await.is_err(),"binding drift accepted: {field}");
        drop(server);
    }
    std::fs::write(
        config.repositories["fixture"].root.join("changed.txt"),
        "changed source",
    )
    .unwrap();
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new(String::new()),
    };
    assert!(server.invoke("inspect_factory_cas",json!({"target_alias":"fixture","run_id":run.id,"request_id":planned.prepared.request_id})).await.is_err());
    assert!(cas.calls().is_empty());
}
#[tokio::test]
async fn receipt_and_thread_identity_mismatches_never_modify_the_durable_plan() {
    use luna_factoryd::store::Store;
    let cas = CasFixture::start().await;
    cas.json(INSPECT, resolved_target());
    let (_temp, config, server) = setup(&cas);
    let (run, planned) = seed_plan(&config);
    let before = serde_json::to_value(&run).unwrap();
    for changed_field in ["receipt_id", "thread_id", "target", "cwd"] {
        let mut observed = receipt(&planned.prepared.request_id, "accepted");
        let mut thread = thread_snapshot();
        match changed_field {
            "receipt_id" => observed["result"]["request_id"] = json!("unrecorded-request"),
            "thread_id" => thread["result"]["result"]["thread"]["id"] = json!("foreign-thread"),
            "target" => thread["result"]["connection"]["target"] = json!("foreign-target"),
            "cwd" => thread["result"]["result"]["thread"]["cwd"] = json!("/foreign/repository"),
            _ => unreachable!(),
        }
        cas.json(RECEIPT, observed);
        cas.json(THREAD, thread);
        assert!(server.invoke("inspect_factory_cas",json!({"target_alias":"fixture","run_id":run.id,"request_id":planned.prepared.request_id})).await.is_err(),"identity mismatch accepted: {changed_field}");
        assert_eq!(
            serde_json::to_value(Store::open(&config).unwrap().get(&run.id).unwrap()).unwrap(),
            before
        );
    }
    cas.assert_read_only();
}

#[tokio::test]
async fn prepared_records_cannot_be_rebound_or_removed_by_snapshot_save() {
    use luna_factoryd::store::Store;
    let cas = CasFixture::start().await;
    let (_temp, config, _server) = setup(&cas);
    let (run, planned) = seed_plan(&config);
    let before = serde_json::to_value(&run).unwrap();
    for mutation in ["remove", "alias", "digest"] {
        let mut store = Store::open(&config).unwrap();
        let mut changed = store.get(&run.id).unwrap();
        let records = &mut changed.control.as_mut().unwrap().cas_requests;
        match mutation {
            "remove" => {
                records.remove(&planned.prepared.request_id);
            }
            "alias" => {
                records
                    .get_mut(&planned.prepared.request_id)
                    .unwrap()
                    .target_alias = "other-alias".into()
            }
            "digest" => {
                records
                    .get_mut(&planned.prepared.request_id)
                    .unwrap()
                    .prepared
                    .payload_sha256 = "f".repeat(64)
            }
            _ => unreachable!(),
        }
        let error = store.save(&mut changed).unwrap_err();
        assert!(
            error.to_string().contains("immutable_cas_request"),
            "{mutation}: {error}"
        );
        assert_eq!(
            serde_json::to_value(store.get(&run.id).unwrap()).unwrap(),
            before,
            "failed save changed persisted plan"
        );
    }
    assert!(cas.calls().is_empty());
}

#[tokio::test]
async fn legacy_control_without_cas_field_defaults_empty_without_rewriting_journal() {
    use luna_factoryd::store::Store;
    use sha2::{Digest, Sha256};
    let cas = CasFixture::start().await;
    let (_temp, config, server) = setup(&cas);
    let request=serde_json::from_value(json!({"repository":"fixture","objective":"Legacy bounded plan","acceptance":["Legacy acceptance"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"legacy-plan"})).unwrap();
    let run = Store::open(&config)
        .unwrap()
        .admit_planning(&config, &request)
        .unwrap()
        .run;
    drop(server);
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let mut old = serde_json::to_value(&run).unwrap();
    old["control"]
        .as_object_mut()
        .unwrap()
        .remove("cas_requests");
    let payload = old.to_string();
    let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
    connection
        .execute(
            "UPDATE runs SET payload=?2 WHERE id=?1",
            rusqlite::params![run.id, payload],
        )
        .unwrap();
    connection.execute("UPDATE control_events SET snapshot_sha256=?2,fingerprint=?2 WHERE run_id=?1 AND revision=0",rusqlite::params![run.id,digest]).unwrap();
    let loaded = Store::open(&config).unwrap().get(&run.id).unwrap();
    assert!(loaded.control.as_ref().unwrap().cas_requests.is_empty());
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(run.clone()).unwrap()
    );
    let actual: String = connection
        .query_row("SELECT payload FROM runs WHERE id=?1", [&run.id], |row| {
            row.get(0)
        })
        .unwrap();
    let actual_digest: String = connection
        .query_row(
            "SELECT snapshot_sha256 FROM control_events WHERE run_id=?1 AND revision=0",
            [&run.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actual, payload, "read rewrote legacy raw payload");
    assert_eq!(actual_digest, digest, "read rewrote legacy journal");
    assert!(cas.calls().is_empty());
}
