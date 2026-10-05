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

fn completed_report(dir: &tempfile::TempDir, factory: &Factory) {
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"CONVERGED","subject":subject,"acceptance":[{"id":"A1","passed":true,"evidence":"Synthetic completed native turn"}],"delta":"Recovered the completed owner result","remaining_gap":"","blocker":null}).to_string()).unwrap();
}

async fn request_operator_decision(
    dir: &tempfile::TempDir,
    factory: &Factory,
    run: &Value,
) -> Value {
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(
        dir.path().join("report.json"),
        json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],
            "delta":"One operator decision remains","remaining_gap":"Select the bounded option",
            "blocker":"Should the optional migration be omitted?"})
        .to_string(),
    )
    .unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    let id = run["id"].as_str().unwrap();
    factory
        .steer(id, run["turn_id"].as_str().unwrap(), "Return the decision")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = factory.get(id).await.unwrap();
            if current["state"] == "NEEDS_INPUT" {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn operator_decisions_with_zero_repair_budget_continue_once_on_the_same_owner() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 0;
    let mut run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap().to_owned();
    let deadline = run["deadline_at"].clone();
    for generation in 2..=3 {
        let decision = request_operator_decision(&dir, &factory, &run).await;
        let decision_id = decision["pending_decision"]["id"].as_str().unwrap();
        run = factory
            .resume_with_decision(&id, Some("Omit the optional migration"), Some(decision_id))
            .await
            .expect("answering a decision is not a repair attempt");
        assert_eq!(run["state"], "RUNNING");
        assert_eq!(run["owner_thread"], "owner");
        assert_eq!(run["generation"], generation);
        assert_eq!(run["repairs_used"], 0);
        assert_eq!(run["deadline_at"], deadline);
        assert_eq!(run["finish"], "local_candidate");
        let replay = factory
            .resume_with_decision(&id, Some("Omit the optional migration"), Some(decision_id))
            .await
            .unwrap();
        assert_eq!(
            replay["generation"], generation,
            "a duplicate answer must not dispatch another turn"
        );
        assert_eq!(
            calls(&dir)
                .iter()
                .filter(|call| call["method"] == "turn/start")
                .count(),
            generation as usize
        );
    }
    factory.cancel(&id).await.unwrap();
}

#[tokio::test]
async fn decision_answer_preserves_an_exhausted_repair_budget() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 1;
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    factory.cancel(id).await.unwrap();
    let repair = factory.resume(id).await.unwrap();
    assert_eq!(repair["repairs_used"], 1);
    let decision = request_operator_decision(&dir, &factory, &repair).await;
    let answered = factory
        .resume_with_decision(
            id,
            Some("Keep the accepted scope"),
            decision["pending_decision"]["id"].as_str(),
        )
        .await
        .expect("decision continuation must preserve the exhausted repair counter");
    assert_eq!(answered["repairs_used"], 1);
    factory.cancel(id).await.unwrap();
    assert_eq!(
        factory
            .resume_with_input(id, Some("A message is not a repair-budget bypass"))
            .await
            .unwrap_err()
            .to_string(),
        "repair_budget_exhausted"
    );
}

#[tokio::test]
async fn unanswered_decision_does_not_dispatch_or_consume_a_repair() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    request_operator_decision(&dir, &factory, &run).await;
    assert_eq!(
        factory.resume(id).await.unwrap_err().to_string(),
        "operator_answer_required"
    );
    assert_eq!(factory.get(id).await.unwrap()["repairs_used"], 0);
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|call| call["method"] == "turn/start")
            .count(),
        1
    );
    factory.cancel(id).await.unwrap();
}

#[tokio::test]
async fn inherited_provider_is_reported_as_configuration_and_cannot_silently_drift() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    assert_eq!(run["route"]["configured_provider"], "inherited-fixture");
    assert_eq!(run["route"]["requested_provider"], "inherited");
    assert_eq!(run["route"]["observed_provider"], Value::Null);
    assert_eq!(run["route"]["observed_model"], Value::Null);
    factory.cancel(id).await.unwrap();
    std::fs::write(dir.path().join("mode"), "provider_drift").unwrap();
    assert_eq!(
        factory.resume(id).await.unwrap_err().to_string(),
        "native_provider_configuration_changed"
    );
    assert_eq!(factory.get(id).await.unwrap()["repairs_used"], 0);
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|call| call["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn reroute_notifications_preserve_exact_turn_evidence_and_stop_owned_work() {
    for (mode, thread, turn) in [
        ("reroute_owner", "owner", "turn-1"),
        ("reroute_child", "child", "child-turn-1"),
    ] {
        let (dir, factory, request) = setup();
        let run = factory.start(request).await.unwrap();
        let id = run["id"].as_str().unwrap();
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        factory
            .steer(id, "turn-1", "Keep the approved route")
            .await
            .unwrap();
        let stopped = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let current = factory.get(id).await.unwrap();
                if current["state"] == "CANCELLED" {
                    return current;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("an observed route mismatch must stop owned work");
        let evidence = stopped["route"]["reroutes"].as_array().unwrap();
        assert_eq!(
            evidence.len(),
            1,
            "duplicate native telemetry must be idempotent"
        );
        assert_eq!(evidence[0]["thread_id"], thread);
        assert_eq!(evidence[0]["turn_id"], turn);
        assert_eq!(evidence[0]["source"], "model/rerouted");
        assert_eq!(evidence[0]["to_model"], "gpt-6-sol");
        assert_eq!(
            stopped["route"]["observed_model"],
            if thread == "owner" {
                json!("gpt-6-sol")
            } else {
                Value::Null
            }
        );
        assert_eq!(stopped["route"]["observed_effort"], Value::Null);
        assert_eq!(stopped["route"]["observed_provider"], Value::Null);
        let stored = luna_factoryd::store::Store::open(&factory.config)
            .unwrap()
            .get(id)
            .unwrap();
        assert_eq!(
            luna_factoryd::lifecycle::public_run(&stored)["route"],
            stopped["route"]
        );
    }
}

#[tokio::test]
async fn unrelated_stale_or_invalid_reroutes_cannot_contaminate_current_evidence() {
    for mode in ["reroute_unrelated", "reroute_stale", "reroute_invalid"] {
        let (dir, factory, request) = setup();
        let run = factory.start(request).await.unwrap();
        let id = run["id"].as_str().unwrap();
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        factory
            .steer(id, "turn-1", "Keep the approved route")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let current = factory.get(id).await.unwrap();
        assert_eq!(current["route"]["observed_model"], Value::Null);
        assert_eq!(current["route"]["reroutes"], json!([]));
        assert!(!current.to_string().contains("sk-synthetic-secret"));
        factory.cancel(id).await.unwrap();
    }
}

#[tokio::test]
async fn lost_ack_completed_turn_is_recovered_without_replaying_inference() {
    let (dir, factory, request) = setup();
    completed_report(&dir, &factory);
    std::fs::write(dir.path().join("mode"), "lost_ack_completed").unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    assert_eq!(run["state"], "BLOCKED");
    let recovered = factory.resume(id).await.unwrap();
    assert_eq!(recovered["state"], "CONVERGED");
    assert_eq!(recovered["turn_id"], "turn-1");
    assert_eq!(recovered["repairs_used"], 0);
    assert_eq!(recovered["deadline_at"], run["deadline_at"]);
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|r| r["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn missing_dispatch_correlation_keeps_claim_without_a_new_turn() {
    let (dir, base, request) = setup();
    let mut config = (*base.config).clone();
    config.database = dir.path().join("recovery-state/runs.sqlite");
    config.native_transport = "existing_daemon".into();
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.dispatch_phase = "turn_start_pending".into();
    run.state = "BLOCKED".into();
    let mut value = serde_json::to_value(&run).unwrap();
    value["dispatch_id"] = json!("missing-dispatch");
    run = serde_json::from_value(value).unwrap();
    store.save(&run).unwrap();
    drop(store);
    let factory = Factory::new(config).unwrap();
    let result = factory.resume(&run.id).await.unwrap();
    assert_eq!(result["state"], "BLOCKED");
    assert_eq!(result["claim_held"], true);
    assert!(!calls(&dir).iter().any(|r| r["method"] == "turn/start"));
}

#[tokio::test]
async fn restart_recovers_a_completed_dispatch_without_starting_a_thread_or_turn() {
    let (dir, base, request) = setup();
    completed_report(&dir, &base);
    let mut config = (*base.config).clone();
    config.database = dir.path().join("restart-state/runs.sqlite");
    config.native_transport = "existing_daemon".into();
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.dispatch_id = Some("persisted-dispatch".into());
    run.dispatch_phase = "turn_start_pending".into();
    store.save(&run).unwrap();
    drop(store);
    std::fs::write(dir.path().join("mode"), "lost_ack_completed").unwrap();
    std::fs::write(
        dir.path().join("native_history.json"),
        json!([{"id":"turn-1","status":"completed","client_id":"persisted-dispatch"}]).to_string(),
    )
    .unwrap();
    let factory = Factory::new(config).unwrap();
    factory.reconcile_startup().await.unwrap();
    let recovered = factory.get(&run.id).await.unwrap();
    assert_eq!(recovered["state"], "CONVERGED");
    assert_eq!(recovered["repairs_used"], 0);
    assert_eq!(recovered["deadline_at"], run.deadline_at);
    assert!(
        !calls(&dir)
            .iter()
            .any(|r| r["method"] == "thread/start" || r["method"] == "turn/start")
    );
}

#[tokio::test]
async fn ambiguous_recovery_blocks_one_run_without_preventing_service_startup() {
    let (dir, base, request) = setup();
    let mut config = (*base.config).clone();
    config.database = dir.path().join("ambiguous-state/runs.sqlite");
    config.native_transport = "existing_daemon".into();
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.dispatch_id = Some("duplicate-client-id".into());
    run.dispatch_phase = "turn_start_pending".into();
    store.save(&run).unwrap();
    drop(store);
    std::fs::write(dir.path().join("native_history.json"),json!([{"id":"turn-1","status":"interrupted","client_id":"duplicate-client-id"},{"id":"turn-2","status":"interrupted","client_id":"duplicate-client-id"}]).to_string()).unwrap();
    let factory = Factory::new(config).unwrap();
    assert!(factory.reconcile_startup().await.is_ok());
    assert_eq!(factory.get(&run.id).await.unwrap()["state"], "BLOCKED");
    assert!(factory.resume(&run.id).await.is_ok());
    assert!(!calls(&dir).iter().any(|r| r["method"] == "turn/start"));
}

#[tokio::test]
async fn cancellation_terminates_only_fresh_owned_terminal_and_requires_exit_evidence() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "terminal_exit").unwrap();
    let run = factory.start(request).await.unwrap();
    let stopped = factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    assert_eq!(stopped["state"], "CANCELLED");
    assert_eq!(stopped["claim_held"], false);
    let records = calls(&dir);
    let stops: Vec<_> = records
        .iter()
        .filter(|r| r["method"] == "thread/backgroundTerminals/terminate")
        .collect();
    assert_eq!(stops.len(), 1);
    assert_eq!(
        stops[0]["params"],
        json!({"threadId":"child","processId":"42"})
    );
    assert_eq!(
        records[0]["params"]["capabilities"]["experimentalApi"],
        true
    );
    assert!(
        !records
            .iter()
            .any(|r| r["method"] == "thread/backgroundTerminals/clean")
    );
}

#[tokio::test]
async fn terminal_disappearance_and_ack_are_not_exit_proof() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "terminal_disappeared").unwrap();
    let run = factory.start(request).await.unwrap();
    let stopped = factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    assert_eq!(stopped["state"], "BLOCKED");
    assert_eq!(stopped["claim_held"], true);
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|r| r["method"] == "thread/backgroundTerminals/terminate")
            .count(),
        1
    );
}

#[tokio::test]
async fn uncertain_terminal_stop_is_not_replayed_on_cancel_retry() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "terminal_lost_ack").unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    for _ in 0..2 {
        let stopped = factory.cancel(id).await.unwrap();
        assert_eq!(stopped["state"], "BLOCKED");
        assert_eq!(stopped["claim_held"], true);
    }
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|r| r["method"] == "thread/backgroundTerminals/terminate")
            .count(),
        1
    );
}

#[tokio::test]
async fn unsupported_terminal_observation_keeps_idle_run_claimed() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    let run = factory.resume(run["id"].as_str().unwrap()).await.unwrap();
    std::fs::write(dir.path().join("mode"), "terminal_unsupported").unwrap();
    let stopped = factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    assert_eq!(stopped["state"], "BLOCKED");
    assert_eq!(stopped["claim_held"], true);
}

#[tokio::test]
async fn stale_exit_history_and_reused_process_ids_cannot_release_a_live_terminal() {
    for mode in ["terminal_reused_pid", "terminal_conflicting_exit"] {
        let (dir, factory, request) = setup();
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        let run = factory.start(request).await.unwrap();
        let id = run["id"].as_str().unwrap();
        for _ in 0..2 {
            let stopped = factory.cancel(id).await.unwrap();
            assert_eq!(stopped["state"], "BLOCKED", "{mode}");
            assert_eq!(stopped["claim_held"], true, "{mode}");
        }
    }
}

#[tokio::test]
async fn failed_read_only_terminal_preflight_can_retry_before_any_stop_dispatch() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "terminal_preflight").unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    assert_eq!(factory.cancel(id).await.unwrap()["state"], "BLOCKED");
    assert!(
        !calls(&dir)
            .iter()
            .any(|r| r["method"] == "thread/backgroundTerminals/terminate")
    );
    assert_eq!(factory.cancel(id).await.unwrap()["state"], "CANCELLED");
    assert_eq!(
        calls(&dir)
            .iter()
            .filter(|r| r["method"] == "thread/backgroundTerminals/terminate")
            .count(),
        1
    );
}

#[tokio::test]
async fn restart_preserves_pending_and_unknown_terminal_stops_without_replay() {
    for outcome in ["pending", "outcome_unknown"] {
        let (dir, base, request) = setup();
        let mut config = (*base.config).clone();
        config.database = dir.path().join("restart-stop/runs.sqlite");
        config.native_transport = "existing_daemon".into();
        let mut store = luna_factoryd::store::Store::open(&config).unwrap();
        let mut run = store.admit(&config, &request).unwrap().run;
        run.thread_id = Some("owner".into());
        run.owned_threads.push("child".into());
        run.terminal_stop_attempts
            .insert("child:terminal-item".into(), outcome.into());
        run.owned_commands
            .insert("child:terminal-item".into(), false);
        run.command_processes
            .insert("child:terminal-item".into(), "child:42".into());
        store.save(&run).unwrap();
        drop(store);
        std::fs::write(dir.path().join("mode"), "terminal_lost_ack").unwrap();
        let factory = Factory::new(config).unwrap();
        factory.reconcile_startup().await.unwrap();
        let stopped = factory.cancel(&run.id).await.unwrap();
        assert_eq!(stopped["claim_held"], true);
        assert_eq!(stopped["state"], "BLOCKED");
        assert!(
            !calls(&dir)
                .iter()
                .any(|r| r["method"] == "thread/backgroundTerminals/terminate"
                    || r["method"] == "turn/start")
        );
    }
}

#[tokio::test]
async fn changed_command_process_identity_after_stop_cannot_release_claim() {
    let (dir, factory, request) = setup();
    std::fs::write(dir.path().join("mode"), "terminal_changed_pid").unwrap();
    let run = factory.start(request).await.unwrap();
    let stopped = factory.cancel(run["id"].as_str().unwrap()).await.unwrap();
    assert_eq!(stopped["state"], "BLOCKED");
    assert_eq!(stopped["claim_held"], true);
}
