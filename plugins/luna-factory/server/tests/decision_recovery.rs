//! Actual Factory regression tests over a labeled subprocess fixture. No live inference.
use luna_factoryd::{
    config::Config,
    lifecycle::Factory,
    store::{StartRequest, Store},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

fn init_repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

fn setup() -> (tempfile::TempDir, Config, StartRequest) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    let binary = dir.path().join("review-native");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native/review_runtime.py"),
        &binary,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        "Synthetic review fixture; no live skill loading proof",
    )
    .unwrap();
    let config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787", "native_transport":"existing_daemon",
        "database":dir.path().join("state/runs.sqlite"), "codex_binary":binary, "skill_path":skill,
        "repositories":{"fixture":{"root":repo,"max_finish":"pr"}},
        "profiles":{"default":{"effort":"high"}},
        "limits":{"capacity":2,"repair_attempts":3,"wall_seconds":300}
    }))
    .unwrap();
    let request = serde_json::from_value(json!({
        "repository":"fixture", "objective":"Synthetic bounded review regression", "acceptance":["fixture only"],
        "non_goals":[], "finish":"pr", "profile":"default", "capacity":2,
        "repair_attempts":3, "wall_seconds":300, "idempotency_key":"review-1"
    })).unwrap();
    (dir, config, request)
}

fn calls(dir: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn wait_state(factory: &Factory, id: &str, expected: &str) -> Value {
    let wait = if expected == "CANCELLED" { 35 } else { 6 };
    tokio::time::timeout(Duration::from_secs(wait), async {
        loop {
            let run = factory.get(id).await.unwrap();
            if run["state"] == expected {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("run never reached {expected}"))
}

#[tokio::test]
async fn terminal_decision_survives_factory_startup() {
    let (_dir, mut config, mut request) = setup();
    request.repair_attempts = 0;
    config.native_transport = "existing_daemon".into();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.turn_id = Some("turn-1".into());
    run.dispatch_phase = "terminal_observed".into();
    run.control.as_mut().unwrap().settlement = luna_factoryd::control::Settlement::Stopped;
    run.set_state(luna_factoryd::control::RunControl::NeedsInput);
    run.blocker = Some("Should the optional migration be omitted?".into());
    // Imported pre-dispatch-ID terminal identity retains its observed generation,
    // but supplies no invented attempt or acceptance proof.
    run.control.as_mut().unwrap().dispatch_generation = run.generation;
    run.control.as_mut().unwrap().migrated = true;
    store.save(&mut run).unwrap();
    drop(store);
    let factory = Factory::new(config).unwrap();
    let actual = factory.get(&run.id).await.unwrap();
    assert_eq!(
        actual["state"], "NEEDS_INPUT",
        "Factory::new erased the persisted terminal decision: {actual}"
    );
    assert_eq!(
        actual["blocker"],
        "Should the optional migration be omitted?"
    );
}
#[tokio::test]
async fn terminal_decision_survives_reconcile_and_remains_answerable_with_zero_repairs() {
    let (dir, mut config, mut request) = setup();
    request.repair_attempts = 0;
    config.native_transport = "existing_daemon".into();
    let factory = Factory::new(config.clone()).unwrap();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.turn_id = Some("turn-1".into());
    run.dispatch_phase = "terminal_observed".into();
    run.control.as_mut().unwrap().settlement = luna_factoryd::control::Settlement::Stopped;
    run.set_state(luna_factoryd::control::RunControl::NeedsInput);
    run.blocker = Some("Should the optional migration be omitted?".into());
    // Imported pre-dispatch-ID terminal identity retains its observed generation,
    // but supplies no invented attempt or acceptance proof.
    run.control.as_mut().unwrap().dispatch_generation = run.generation;
    run.control.as_mut().unwrap().migrated = true;
    store.save(&mut run).unwrap();
    drop(store);
    factory.reconcile_startup().await.unwrap();
    let actual = factory.get(&run.id).await.unwrap();
    let answered = factory
        .resume_with_decision(
            &run.id,
            Some("Omit it"),
            actual["pending_decision"]["id"].as_str(),
        )
        .await;
    assert!(
        answered.is_ok(),
        "idle reconcile erased decision; answer failed: {:?}; run={actual}",
        answered.err()
    );
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|v| v["method"] == "turn/start")
            .count(),
        1
    );
}
#[tokio::test]
async fn replayed_child_failure_does_not_consume_two_repairs() {
    let (dir, config, request) = setup();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    std::fs::write(dir.path().join("mode"), "child_failure").unwrap();
    for _ in 0..2 {
        factory
            .steer(id, "turn-1", "Same failure replay")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let used = factory.get(id).await.unwrap()["repairs_used"].clone();
    let stored = Store::open(&factory.config).unwrap().get(id).unwrap();
    assert!(
        stored
            .counted_failures
            .contains(&("child".into(), "child-failure".into()))
    );
    factory.cancel(id).await.unwrap();
    assert_eq!(
        used, 1,
        "same owned child + native turn counted more than once"
    );
}
#[tokio::test]
async fn failure_before_resume_ack_is_not_overwritten_by_resume_snapshot() {
    let (dir, config, request) = setup();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    factory.cancel(id).await.unwrap();
    std::fs::write(dir.path().join("mode"), "failure_before_resume_ack").unwrap();
    let resumed = factory.resume(id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    let actual = factory.get(id).await.unwrap();
    factory.cancel(id).await.unwrap();
    assert_eq!(
        actual["repairs_used"], 2,
        "one resume plus distinct child failure should be retained; response={resumed}"
    );
}

#[tokio::test]
async fn transport_close_preserves_accepted_terminal_decision() {
    let (dir, config, mut request) = setup();
    request.repair_attempts = 0;
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"delta":"One operator decision remains","remaining_gap":"Select option","blocker":"Omit the optional migration?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return decision")
        .await
        .unwrap();
    wait_state(&factory, id, "NEEDS_INPUT").await;
    std::fs::write(
        dir.path().join("close-transport"),
        "close fixture transport",
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let actual = factory.get(id).await.unwrap();
    assert_eq!(
        actual["state"], "NEEDS_INPUT",
        "transport loss erased a completed decision: {actual}"
    );
    assert_eq!(actual["blocker"], "Omit the optional migration?");
}

async fn decision_report(
    dir: &tempfile::TempDir,
    factory: &Factory,
    run: &Value,
    blocker: &str,
    acceptance: Value,
) -> Value {
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":acceptance,"delta":"One operator decision remains","remaining_gap":"Select option","blocker":blocker}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    let id = run["id"].as_str().unwrap();
    factory
        .steer(id, run["turn_id"].as_str().unwrap(), "Return decision")
        .await
        .unwrap();
    wait_state(factory, id, "NEEDS_INPUT").await
}
#[tokio::test]
async fn late_duplicate_answer_must_not_answer_next_decision() {
    let (dir, config, mut request) = setup();
    request.repair_attempts = 0;
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let first = decision_report(&dir, &factory, &run, "Use option A or B?", json!([])).await;
    let decision_id = first["pending_decision"]["id"].as_str().unwrap();
    assert_eq!(
        factory
            .resume_with_input(id, Some("Option A"))
            .await
            .unwrap_err()
            .to_string(),
        "expected_decision_id_required"
    );
    let next = factory
        .resume_with_decision(id, Some("Option A"), Some(decision_id))
        .await
        .unwrap();
    decision_report(
        &dir,
        &factory,
        &next,
        "Delete optional content or keep it?",
        json!([]),
    )
    .await;
    let retry = factory
        .resume_with_decision(id, Some("Option A"), Some(decision_id))
        .await
        .unwrap();
    assert_eq!(
        retry["generation"], 2,
        "the old answer dispatched a new turn"
    );
    assert_eq!(retry["state"], "NEEDS_INPUT");
    assert_ne!(retry["pending_decision"]["id"], decision_id);
    assert_eq!(
        factory
            .resume_with_decision(id, Some("Different answer"), Some(decision_id))
            .await
            .unwrap_err()
            .to_string(),
        "decision_answer_idempotency_conflict"
    );
    assert_eq!(
        factory
            .resume_with_decision(id, Some("Option A"), Some("unknown-decision"))
            .await
            .unwrap_err()
            .to_string(),
        "stale_decision_id"
    );
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|call| call["method"] == "turn/start")
            .count(),
        2
    );
    let mut recovered_config = (*factory.config).clone();
    recovered_config.database = dir.path().join("recovered-state/runs.sqlite");
    std::fs::create_dir_all(recovered_config.database.parent().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            recovered_config.database.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    rusqlite::Connection::open(&factory.config.database)
        .unwrap()
        .execute(
            "VACUUM INTO ?1",
            [recovered_config.database.to_str().unwrap()],
        )
        .unwrap();
    let recovered = Factory::new(recovered_config).unwrap();
    let count = calls(&dir).len();
    let replayed = recovered
        .resume_with_decision(id, Some("Option A"), Some(decision_id))
        .await
        .unwrap();
    assert_eq!(replayed["generation"], 2);
    assert_eq!(
        calls(&dir).len(),
        count,
        "restart replay made a native call"
    );
}
