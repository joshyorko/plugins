//! Persisted exact-profile execution qualification (#71): every native effect path
//! fails closed. Synthetic subprocess fixtures and temporary databases only; no Codex,
//! no inference and no real database. Fixtures record every native message they receive.
#[allow(dead_code)]
mod support;
use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use luna_factoryd::{
    config::Config,
    http::McpServer,
    lifecycle::Factory,
    qualification::{self, ExecutionProfile, Scope},
    store::StartRequest,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success(),
        "git {args:?}"
    );
}
fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q"]);
    git(
        path,
        &[
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
    );
}

struct Fixture {
    dir: tempfile::TempDir,
    config: Config,
    request: StartRequest,
}
fn fixture(script: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    repository(&repo);
    std::fs::write(repo.join("proof.txt"), "stable proof").unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/native")
        .join(script);
    let binary = dir.path().join("native-fixture");
    std::fs::copy(source, &binary).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        "Synthetic qualification fixture skill; no inference.",
    )
    .unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787",
        "database":dir.path().join("state/runs.sqlite"),"codex_binary":binary,"skill_path":skill,
        "repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"high"}},
        "limits":{"capacity":2,"repair_attempts":2,"wall_seconds":300}}))
    .unwrap();
    let request = serde_json::from_value(json!({"repository":"fixture","objective":"Synthetic qualification test",
        "acceptance":["A1 current task-scoped proof"],"non_goals":[],"finish":"local_candidate","profile":"default",
        "capacity":1,"repair_attempts":2,"wall_seconds":300,"idempotency_key":"qualification-1"}))
    .unwrap();
    Fixture {
        dir,
        config,
        request,
    }
}
fn calls(dir: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn count(dir: &tempfile::TempDir, method: &str) -> usize {
    calls(dir).iter().filter(|c| c["method"] == method).count()
}
fn fingerprint(config: &Config) -> String {
    ExecutionProfile::from_config(config, "default")
        .unwrap()
        .fingerprint()
}
fn revoke(config: &Config) {
    qualification::revoke(config, &fingerprint(config)).unwrap();
}
fn expire(config: &Config) {
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload FROM execution_qualifications WHERE fingerprint=?1",
            [fingerprint(config)],
            |row| row.get(0),
        )
        .unwrap();
    let mut value: Value = serde_json::from_str(&payload).unwrap();
    value["expires_at"] = json!(1);
    connection
        .execute(
            "UPDATE execution_qualifications SET payload=?1 WHERE fingerprint=?2",
            rusqlite::params![value.to_string(), fingerprint(config)],
        )
        .unwrap();
}
fn no_unknown_effects(run: &Value) {
    let effects = run["control"]["effects"].as_array().unwrap();
    assert!(
        effects.iter().all(|effect| effect["status"] != "unknown"),
        "an unsent effect was left unknown: {effects:?}"
    );
}

#[derive(Clone, Copy, Debug)]
enum Denial {
    Missing,
    Revoked,
    Expired,
    ChangedEffort,
    ChangedBinary,
    QualificationOnly,
    OverCapacity,
}
const DENIALS: [Denial; 7] = [
    Denial::Missing,
    Denial::Revoked,
    Denial::Expired,
    Denial::ChangedEffort,
    Denial::ChangedBinary,
    Denial::QualificationOnly,
    Denial::OverCapacity,
];
/// Returns the Factory configuration, start request and exact expected code.
fn denied_case(fixture: &Fixture, denial: Denial) -> (Config, StartRequest, &'static str) {
    let config = fixture.config.clone();
    let mut request = fixture.request.clone();
    let code = match denial {
        Denial::Missing => qualification::NOT_QUALIFIED,
        Denial::Revoked => {
            revoke(&support::qualified(config.clone()));
            qualification::REVOKED
        }
        Denial::Expired => {
            expire(&support::qualified(config.clone()));
            qualification::EXPIRED
        }
        Denial::ChangedEffort => {
            support::qualified(config.clone());
            let mut changed = config.clone();
            changed.profiles.get_mut("default").unwrap().effort = "low".into();
            return (changed, request, qualification::PROFILE_MISMATCH);
        }
        Denial::ChangedBinary => {
            support::qualified(config.clone());
            let mut bytes = std::fs::read(&config.codex_binary).unwrap();
            bytes.extend_from_slice(b"\n# replaced binary\n");
            std::fs::write(&config.codex_binary, bytes).unwrap();
            qualification::PROFILE_MISMATCH
        }
        Denial::QualificationOnly => {
            support::qualified_with(config.clone(), Scope::QualificationOnly, 8);
            qualification::SCOPE
        }
        Denial::OverCapacity => {
            support::qualified_with(config.clone(), Scope::Ordinary, 1);
            request.capacity = 2;
            qualification::CAPACITY
        }
    };
    (config, request, code)
}

async fn rpc(app: &Router, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientInfo":{"name":"luna-tests","version":"1"},
        "io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}}}});
    let mut request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "127.0.0.1:8787")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method);
    if let Some(name) = params.get("name").and_then(Value::as_str) {
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
    assert!(response.status().is_success());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn direct_start_refuses_every_unqualified_grant_before_admission_or_native_contact() {
    for denial in DENIALS {
        let fixture = fixture("fake_factory.py");
        let (config, request, code) = denied_case(&fixture, denial);
        let factory = Factory::new(config).unwrap();
        let error = factory.start(request).await.unwrap_err();
        assert_eq!(error.to_string(), code, "{denial:?}");
        assert_eq!(factory.list(10).await.unwrap(), json!([]), "{denial:?}");
        assert!(
            calls(&fixture.dir).is_empty(),
            "{denial:?} contacted native"
        );
    }
}

#[tokio::test]
async fn public_mcp_start_factory_refuses_every_unqualified_grant_with_the_exact_code() {
    for denial in DENIALS {
        let fixture = fixture("fake_factory.py");
        let (config, request, code) = denied_case(&fixture, denial);
        let factory = Factory::new(config).unwrap();
        let app = luna_factoryd::http::router(
            factory.clone(),
            "<!doctype html><title>Luna fixture</title>".into(),
            CancellationToken::new(),
        );
        let response = rpc(
            &app,
            "tools/call",
            json!({"name":"start_factory","arguments":serde_json::to_value(&request).unwrap()}),
        )
        .await;
        assert_eq!(
            response["result"]["isError"], true,
            "{denial:?}: {response}"
        );
        assert_eq!(
            response["result"]["content"][0]["text"], code,
            "{denial:?}: {response}"
        );
        assert_eq!(factory.list(10).await.unwrap(), json!([]), "{denial:?}");
        assert!(
            calls(&fixture.dir).is_empty(),
            "{denial:?} contacted native"
        );
    }
}

#[tokio::test]
async fn revocation_refuses_steer_resume_and_repair_but_keeps_cancel_and_reconcile() {
    let fixture = fixture("fake_factory.py");
    let config = support::qualified(fixture.config.clone());
    let factory = Factory::new(config.clone()).unwrap();
    let run = factory.start(fixture.request.clone()).await.unwrap();
    assert_eq!(run["state"], "RUNNING");
    let id = run["id"].as_str().unwrap();
    revoke(&config);
    let before = calls(&fixture.dir).len();
    assert_eq!(
        factory
            .steer(id, "turn-1", "Stay in scope")
            .await
            .unwrap_err()
            .to_string(),
        qualification::REVOKED
    );
    assert_eq!(
        factory.resume(id).await.unwrap_err().to_string(),
        qualification::REVOKED
    );
    assert_eq!(
        calls(&fixture.dir).len(),
        before,
        "a refused effect reached native"
    );
    no_unknown_effects(&factory.get(id).await.unwrap());
    // Containment stays available without qualification.
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(stopped["state"], "CANCELLED");
    assert!(count(&fixture.dir, "turn/interrupt") >= 1);
    // Reconcile is read-only and never gated on qualification.
    if let Err(error) = factory.reconcile(id, None).await {
        assert!(!error.to_string().starts_with("execution_"), "{error}");
    }
    let after_cancel = calls(&fixture.dir).len();
    // Repair resume of the stopped run is refused before any connection or read.
    assert_eq!(
        factory.resume(id).await.unwrap_err().to_string(),
        qualification::REVOKED
    );
    assert_eq!(calls(&fixture.dir).len(), after_cancel);
    assert_eq!(count(&fixture.dir, "thread/resume"), 0);
    assert_eq!(count(&fixture.dir, "turn/start"), 1);
    assert_eq!(count(&fixture.dir, "turn/steer"), 0);
}

#[tokio::test]
async fn revocation_by_another_process_while_awaiting_readiness_stops_thread_start_at_the_wire() {
    let fixture = fixture("fake_factory.py");
    let factory = Factory::new(support::qualified(fixture.config.clone())).unwrap();
    // The fixture revokes from its own process and connection while model/list awaits.
    std::fs::write(fixture.dir.path().join("mode"), "revoke_on_model/list").unwrap();
    let run = factory.start(fixture.request.clone()).await.unwrap();
    assert!(count(&fixture.dir, "model/list") >= 1);
    assert_eq!(
        count(&fixture.dir, "thread/start"),
        0,
        "start byte escaped the guard"
    );
    assert_eq!(count(&fixture.dir, "turn/start"), 0);
    assert_eq!(run["state"], "FAILED");
    assert_eq!(run["claim_held"], false);
    assert_eq!(
        run["blocker"],
        qualification::blocker(qualification::REVOKED)
    );
    no_unknown_effects(&run);
}

#[tokio::test]
async fn revocation_after_thread_start_dispatches_no_turn_and_cancel_still_works() {
    let fixture = fixture("fake_factory.py");
    let factory = Factory::new(support::qualified(fixture.config.clone())).unwrap();
    std::fs::write(fixture.dir.path().join("mode"), "revoke_on_thread/start").unwrap();
    let run = factory.start(fixture.request.clone()).await.unwrap();
    assert_eq!(count(&fixture.dir, "thread/start"), 1);
    assert_eq!(
        count(&fixture.dir, "turn/start"),
        0,
        "turn byte escaped the gate"
    );
    assert_eq!(run["state"], "BLOCKED");
    assert!(
        run["blocker"]
            .as_str()
            .unwrap()
            .starts_with("execution_not_qualified: execution_qualification_revoked;")
    );
    assert_eq!(run["control"]["attempts"], json!([]));
    no_unknown_effects(&run);
    std::fs::remove_file(fixture.dir.path().join("mode")).unwrap();
    factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    assert_eq!(count(&fixture.dir, "turn/start"), 0);
}

fn candidate(id: &str) -> Value {
    json!({"id":id,"title":"Necessary bounded work","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]})
}

#[tokio::test]
async fn automatic_continuation_after_revocation_leaves_an_honest_blocker_and_sends_nothing() {
    let fixture = fixture("fake_continuation.py");
    let mut request = fixture.request.clone();
    request.repair_attempts = 0;
    let factory = Factory::new(support::qualified(fixture.config.clone())).unwrap();
    let run = factory.start(request).await.unwrap();
    assert_eq!(run["state"], "RUNNING");
    let id = run["id"].as_str().unwrap();
    let report = json!({"state":"QUIESCENT","subject":run["current_subject"],"acceptance":[],"checks":[],
        "candidates":[candidate("task-a")],"selected_task":"task-a",
        "continuation":{"kind":"next","task_id":"task-a","diagnosis":null},
        "delta":"Bounded work remains","remaining_gap":"A1 requires current proof","blocker":null});
    std::fs::write(fixture.dir.path().join("report.json"), report.to_string()).unwrap();
    // The fixture completes the turn and revokes from its own process before notifying.
    std::fs::write(fixture.dir.path().join("mode"), "revoke_after_completion").unwrap();
    factory
        .steer(
            id,
            run["turn_id"].as_str().unwrap(),
            "Return the fixture result",
        )
        .await
        .unwrap();
    let blocked = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = factory.get(id).await.unwrap();
            if current["state"] == "BLOCKED" {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("continuation was not refused");
    assert_eq!(
        blocked["blocker"],
        qualification::blocker(qualification::REVOKED)
    );
    assert_eq!(
        count(&fixture.dir, "turn/start"),
        1,
        "continuation was sent"
    );
    assert_eq!(blocked["generation"], 1);
    no_unknown_effects(&blocked);
}

#[tokio::test]
async fn planning_needs_no_qualification_and_contacts_no_native_runtime() {
    let fixture = fixture("fake_factory.py");
    let factory = Factory::new(fixture.config.clone()).unwrap();
    let graph = factory.create_graph(fixture.request.clone()).await.unwrap();
    let id = graph["graph"]["run_id"].as_str().unwrap();
    assert_eq!(graph["graph"]["planning_only"], true);
    factory.graph(id).await.unwrap();
    assert!(calls(&fixture.dir).is_empty());
}

#[tokio::test]
async fn a_qualified_start_still_cannot_replay_a_planning_key_as_execution() {
    let fixture = fixture("fake_factory.py");
    let factory = Factory::new(support::qualified(fixture.config.clone())).unwrap();
    factory.create_graph(fixture.request.clone()).await.unwrap();
    assert!(
        factory
            .start(fixture.request.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("idempotency_conflict")
    );
    assert!(calls(&fixture.dir).is_empty());
}

fn capabilities_validator() -> jsonschema::Validator {
    let tool = luna_factoryd::mcp::tool_definitions()
        .into_iter()
        .find(|t| t.name == "get_factory_capabilities")
        .unwrap();
    jsonschema::validator_for(&serde_json::to_value(tool.output_schema.unwrap()).unwrap()).unwrap()
}

#[tokio::test]
async fn capabilities_report_eligibility_only_for_a_current_ordinary_grant() {
    let validator = capabilities_validator();
    let fixture = fixture("fake_factory.py");
    let server = McpServer {
        factory: Factory::new(fixture.config.clone()).unwrap(),
        html: Arc::new(String::new()),
    };
    let execution = |output: &Value| {
        assert!(validator.is_valid(output), "{output}");
        let execution = output["execution"].clone();
        assert!(
            !execution.to_string().contains('/'),
            "paths leaked: {execution}"
        );
        execution
    };
    let unqualified = execution(
        &server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap(),
    );
    assert_eq!(
        unqualified,
        json!({"eligible":false,"reason":"execution_not_qualified","profiles":[
            {"alias":"default","status":"denied","reason":"execution_not_qualified","max_capacity":null}]})
    );
    support::qualified_with(fixture.config.clone(), Scope::QualificationOnly, 1);
    let scoped = execution(&server.factory.capabilities().await.unwrap());
    assert_eq!(scoped["eligible"], false);
    assert_eq!(scoped["reason"], qualification::SCOPE);
    support::qualified_with(fixture.config.clone(), Scope::Ordinary, 2);
    let qualified = execution(
        &server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap(),
    );
    assert_eq!(
        qualified,
        json!({"eligible":true,"reason":"execution_qualified","profiles":[
            {"alias":"default","status":"qualified","reason":"execution_qualified","max_capacity":2}]})
    );
    revoke(&fixture.config);
    let revoked = execution(&server.factory.capabilities().await.unwrap());
    assert_eq!(revoked["eligible"], false);
    assert_eq!(revoked["reason"], qualification::REVOKED);
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
        .args(args)
        .output()
        .unwrap()
}
fn write_config(fixture: &Fixture) -> String {
    let path = fixture.dir.path().join("config.json");
    std::fs::write(&path, serde_json::to_vec(&fixture.config).unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}
fn refused(output: &std::process::Output, code: &str) {
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(code), "expected {code}: {stderr}");
}

#[test]
fn operator_cli_refuses_missing_mismatched_and_unproved_evidence() {
    let fixture = fixture("fake_factory.py");
    let config = write_config(&fixture);
    let profile = cli(&[
        "qualification-profile",
        "--config",
        &config,
        "--profile",
        "default",
    ]);
    assert!(profile.status.success(), "{profile:?}");
    let printed: Value = serde_json::from_slice(&profile.stdout).unwrap();
    assert_eq!(printed["fingerprint"], fingerprint(&fixture.config));
    assert_eq!(printed["profile"]["model"], "gpt-6-luna");
    let evidence = fixture.dir.path().join("evidence.json");
    let evidence_arg = evidence.to_string_lossy().into_owned();
    let record = |scope: &str| {
        cli(&[
            "record-qualification",
            "--config",
            &config,
            "--profile",
            "default",
            "--evidence",
            &evidence_arg,
            "--scope",
            scope,
        ])
    };
    refused(&record("ordinary"), "qualification_evidence_missing");
    std::fs::write(
        &evidence,
        json!({"profile_fingerprint":"0".repeat(64)}).to_string(),
    )
    .unwrap();
    refused(
        &record("ordinary"),
        "qualification_evidence_fingerprint_mismatch",
    );
    refused(
        &record("qualification_only"),
        "qualification_evidence_fingerprint_mismatch",
    );
    std::fs::write(
        &evidence,
        json!({"profile_fingerprint":fingerprint(&fixture.config)}).to_string(),
    )
    .unwrap();
    refused(
        &record("ordinary"),
        "qualification_evidence_harness_unrecognized",
    );
    let mut failed = support::passed_evidence(&fingerprint(&fixture.config));
    failed["qualification_run"]["outcome"] = json!("failed");
    std::fs::write(&evidence, failed.to_string()).unwrap();
    refused(&record("ordinary"), "qualification_evidence_not_passed");
    refused(&record("everything"), "invalid_qualification_scope");
    let list = cli(&["qualifications", "--config", &config]);
    assert_eq!(
        serde_json::from_slice::<Value>(&list.stdout).unwrap(),
        json!([])
    );
    // A qualification_only grant needs only the exact fingerprint; it never runs ordinary work.
    std::fs::write(
        &evidence,
        json!({"profile_fingerprint":fingerprint(&fixture.config)}).to_string(),
    )
    .unwrap();
    let granted = record("qualification_only");
    assert!(granted.status.success(), "{granted:?}");
    let listed: Value =
        serde_json::from_slice(&cli(&["qualifications", "--config", &config]).stdout).unwrap();
    assert_eq!(listed[0]["scope"], "qualification_only");
    assert_eq!(listed[0]["status"], "current");
    assert_eq!(
        qualification::check(&fixture.config, "default", Scope::Ordinary, 1).status,
        qualification::Status::Denied(qualification::SCOPE)
    );
    let revoked = cli(&[
        "revoke-qualification",
        "--config",
        &config,
        "--fingerprint",
        &fingerprint(&fixture.config),
    ]);
    assert!(revoked.status.success(), "{revoked:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&revoked.stdout).unwrap()["status"],
        "revoked"
    );
    refused(
        &cli(&[
            "revoke-qualification",
            "--config",
            &config,
            "--fingerprint",
            &"a".repeat(64),
        ]),
        "qualification_not_found",
    );
}

#[test]
fn canary_refuses_missing_grants_and_non_disposable_workspaces_without_native_contact() {
    let fixture = fixture("fake_factory.py");
    let workspace = fixture.dir.path().join("canary");
    repository(&workspace);
    let refusal = |config: &Config, path: &Path| {
        qualification::canary_preconditions(config, "default", path)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refusal(&fixture.config, &workspace),
        qualification::NOT_QUALIFIED
    );
    support::qualified_with(fixture.config.clone(), Scope::Ordinary, 1);
    assert_eq!(refusal(&fixture.config, &workspace), qualification::SCOPE);
    support::qualified_with(fixture.config.clone(), Scope::QualificationOnly, 1);
    let configured = fixture.config.repositories["fixture"].root.clone();
    assert_eq!(
        refusal(&fixture.config, &configured),
        "canary_workspace_overlaps_configured_repository"
    );
    let plain = fixture.dir.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    assert_eq!(
        refusal(&fixture.config, &plain),
        "canary_workspace_not_git_repository"
    );
    let remote = fixture.dir.path().join("with-remote");
    repository(&remote);
    git(
        &remote,
        &["remote", "add", "origin", "https://example.invalid/r.git"],
    );
    assert_eq!(
        refusal(&fixture.config, &remote),
        "canary_workspace_has_remotes"
    );
    assert_eq!(
        refusal(&fixture.config, Path::new("relative/canary")),
        "canary_workspace_must_be_absolute"
    );
    // A disposable workspace passes preconditions, but the harness refuses honestly.
    let config = write_config(&fixture);
    let output = fixture.dir.path().join("canary-evidence.json");
    let run = cli(&[
        "qualification-canary",
        "--config",
        &config,
        "--profile",
        "default",
        "--workspace",
        &workspace.to_string_lossy(),
        "--output",
        &output.to_string_lossy(),
    ]);
    refused(&run, "qualification_canary_not_yet_implemented");
    assert!(!output.exists(), "canary wrote evidence without running");
    assert!(calls(&fixture.dir).is_empty(), "canary contacted native");
    // The qualification_only grant still cannot start ordinary work.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let factory = Factory::new(fixture.config.clone()).unwrap();
        assert_eq!(
            factory
                .start(fixture.request.clone())
                .await
                .unwrap_err()
                .to_string(),
            qualification::SCOPE
        );
    });
    assert!(calls(&fixture.dir).is_empty());
}
