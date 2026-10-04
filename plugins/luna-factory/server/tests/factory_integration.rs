//! Full runtime tests using a labeled subprocess fixture, never inference.
use luna_factoryd::{config::Config, lifecycle::Factory, store::StartRequest};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

fn setup() -> (tempfile::TempDir, Factory, StartRequest) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native/fake_factory.py");
    let binary = dir.path().join("native-fixture");
    std::fs::copy(fixture, &binary).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "Synthetic fixture skill; not a live loading proof").unwrap();
    let config:Config=serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":binary,"skill_path":skill,"repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":2,"repair_attempts":2,"wall_seconds":300}})).unwrap();
    let request=serde_json::from_value(json!({"repository":"fixture","objective":"Synthetic factory test","acceptance":["A1 fixture only"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":2,"wall_seconds":300,"idempotency_key":"fixture-1"})).unwrap();
    (dir, Factory::new(config).unwrap(), request)
}
fn calls(dir: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[tokio::test]
async fn runtime_start_status_steer_cancel_resume_preserve_native_owner() {
    let (dir, factory, request) = setup();
    let start = factory.start(request.clone()).await.unwrap();
    assert_eq!(start["state"], "RUNNING");
    let id = start["id"].as_str().unwrap();
    let duplicate = factory.start(request).await.unwrap();
    assert_eq!(duplicate["id"], id);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        factory.get(id).await.unwrap()["active_workers"],
        1,
        "spawn event before start response was lost"
    );
    let count = calls(&dir).len();
    for _ in 0..10 {
        factory.get(id).await.unwrap();
        factory.list(20).await.unwrap();
    }
    assert_eq!(calls(&dir).len(), count, "status called native runtime");
    factory
        .steer(id, "turn-1", "Keep the same objective")
        .await
        .unwrap();
    assert!(factory.steer(id, "stale", "bad").await.is_err());
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(stopped["state"], "CANCELLED");
    assert_eq!(stopped["claim_held"], false);
    let resumed = factory.resume(id).await.unwrap();
    assert_eq!(resumed["owner_thread"], "owner");
    assert_eq!(resumed["turn_id"], "turn-2");
    assert_eq!(resumed["deadline_at"], start["deadline_at"]);
    assert_eq!(resumed["repairs_used"], 1);
    let history = calls(&dir);
    assert_eq!(
        history
            .iter()
            .filter(|c| c["method"] == "thread/start")
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|c| c["method"] == "turn/start")
            .count(),
        2
    );
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn ambiguous_descendant_keeps_repository_claim() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    std::fs::write(dir.path().join("mode"), "unknown_child").unwrap();
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(stopped["state"], "BLOCKED");
    assert_eq!(stopped["claim_held"], true);
}
#[tokio::test]
async fn failed_resume_is_recoverable_blocked_not_stranded_starting() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    factory.cancel(id).await.unwrap();
    std::fs::write(dir.path().join("mode"), "fail_resume").unwrap();
    let _ = factory.resume(id).await;
    let result = factory.get(id).await.unwrap();
    assert_eq!(result["state"], "BLOCKED");
    assert_eq!(result["claim_held"], true);
    assert_eq!(result["owner_thread"], "owner");
}
#[tokio::test]
async fn second_service_cannot_reconcile_another_live_owner() {
    let (_dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    assert!(
        Factory::new((*factory.config).clone()).is_err(),
        "second daemon stole startup reconciliation"
    );
    assert_eq!(
        factory.get(run["id"].as_str().unwrap()).await.unwrap()["state"],
        "RUNNING"
    );
    factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
}
#[tokio::test]
async fn idle_threads_with_background_terminal_do_not_release_claim() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "background").unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(stopped["claim_held"], true);
    assert_eq!(stopped["state"], "BLOCKED");
    std::fs::write(dir.path().join("mode"), "process_exited").unwrap();
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(stopped["state"], "CANCELLED");
}
#[tokio::test]
async fn owner_acceptance_is_fetched_from_fenced_native_history_and_bound_to_subject() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"CONVERGED","subject":subject,"acceptance":[{"id":"A1","passed":true,"evidence":"Synthetic fixture exit 0"}],"delta":"Verified the bounded criterion","remaining_gap":"","blocker":null}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return the verified result")
        .await
        .unwrap();
    let mut result = factory.get(id).await.unwrap();
    for _ in 0..50 {
        if result["state"] == "CONVERGED" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        result = factory.get(id).await.unwrap();
    }
    assert_eq!(result["state"], "CONVERGED");
    assert_eq!(result["claim_held"], false);
    assert_eq!(result["current_subject"], subject);
    assert_eq!(result["delta"], "Verified the bounded criterion");
    assert_eq!(result["route"]["observed_model"], Value::Null);
    assert!(
        result["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == "criterion_acceptance")
    );
}
#[tokio::test]
async fn confirmed_preflight_failure_does_not_leave_an_unrecoverable_claim() {
    let (dir, factory, _request) = setup();
    let mut config = (*factory.config).clone();
    config.database = dir.path().join("other-state/runs.sqlite");
    config.codex_binary = PathBuf::from("/bin/false");
    let unavailable = Factory::new(config).unwrap();
    let request:StartRequest=serde_json::from_value(json!({"repository":"fixture","objective":"bounded","acceptance":["passes"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":2,"wall_seconds":300,"idempotency_key":"blocked-start"})).unwrap();
    let result = unavailable.start(request).await.unwrap();
    assert_eq!(result["state"], "FAILED");
    assert_eq!(result["claim_held"], false);
    assert_eq!(result["owner_thread"], Value::Null);
}
#[tokio::test]
async fn operator_answer_resumes_same_owner_without_expanding_authority() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    factory.cancel(id).await.unwrap();
    let resumed = factory
        .resume_with_input(id, Some("Keep the change local and omit the migration"))
        .await
        .unwrap();
    assert_eq!(resumed["owner_thread"], "owner");
    assert_eq!(resumed["finish"], "local_candidate");
    let records = calls(&dir);
    let turn = records
        .iter()
        .rev()
        .find(|v| v["method"] == "turn/start")
        .unwrap();
    assert!(
        turn["params"]["input"][1]["text"]
            .as_str()
            .unwrap()
            .contains("Keep the change local and omit the migration")
    );
    factory.cancel(id).await.unwrap();
}
