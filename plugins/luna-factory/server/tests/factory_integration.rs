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
async fn ended_owner_unknown_is_reconciled_without_redispatch_or_budget_reset() {
    let (dir, factory, request) = setup();
    let started = factory.start(request).await.unwrap();
    let id = started["id"].as_str().unwrap();
    let subject = started["current_subject"].clone();
    std::fs::write(dir.path().join("report.json"), json!({"state":"BLOCKED","subject":subject,
        "acceptance":[],"checks":[],"candidates":[],"selected_task":"objective",
        "delta":"Original turn ended","remaining_gap":"Original criteria unproved","blocker":"Needs evidence"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish_owner_unknown").unwrap();
    factory
        .steer(id, "turn-1", "Return the same objective")
        .await
        .unwrap();
    let blocked = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let value = factory.get(id).await.unwrap();
            if value["blocker"] == "Owner turn ended but owned execution is not verified stopped." {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(blocked["claim_held"], true);
    assert_eq!(blocked["presentation"]["owner"]["liveness"], "unknown");
    let action = &blocked["presentation"]["primary_action"];
    assert_eq!(action["tool"], "reconcile_factory_run");
    assert_eq!(action["allowed"], true);
    let catalog = serde_json::to_value(luna_factoryd::mcp::tool_definitions()).unwrap();
    assert!(catalog.as_array().unwrap().iter().any(|tool| {
        tool["name"] == action["tool"]
            && tool["_meta"]["ui"]["visibility"]
                .as_array()
                .is_none_or(|contexts| contexts.iter().any(|context| context == "model"))
    }));
    let count = calls(&dir).len();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    let recovered = factory
        .reconcile(id, blocked["control"]["revision"].as_u64())
        .await
        .unwrap();
    assert_eq!(recovered["generation"], 1);
    assert_eq!(recovered["owner_thread"], started["owner_thread"]);
    assert_eq!(recovered["turn_id"], started["turn_id"]);
    assert_eq!(recovered["deadline_at"], started["deadline_at"]);
    assert_eq!(recovered["repairs_used"], 0);
    assert_eq!(recovered["claim_held"], true);
    assert_eq!(recovered["presentation"]["owner"]["liveness"], "idle");
    assert!(calls(&dir)[count..].iter().all(|call| !matches!(
        call["method"].as_str(),
        Some("thread/start" | "thread/resume" | "turn/start" | "turn/steer" | "turn/interrupt")
    )));
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
    let subject = predicate_report(&dir, &factory, "Verified the bounded criterion");
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
    predicate_report(dir, factory, "Recovered the completed owner result");
}
fn predicate_report(dir: &tempfile::TempDir, factory: &Factory, delta: &str) -> String {
    use sha2::{Digest, Sha256};
    let root = &factory.config.repositories["fixture"].root;
    std::fs::write(root.join("predicate.txt"), "synthetic bounded predicate").unwrap();
    let subject = luna_factoryd::store::repository_subject(root).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"synthetic bounded predicate"));
    // The fake native producer binds this declaration to its actual persisted dispatch.
    std::fs::write(dir.path().join("report.json"),json!({"state":"CONVERGED","subject":subject,
        "_fixture_bind_dispatch":true,
        "checks":[{"id":"fixture-file","kind":"file_sha256","path":"predicate.txt","sha256":digest,"native_item":null}],
        "acceptance":[{"id":"A1","passed":true,"accepted":true,"check_refs":["fixture-file"],"evidence":"Owner semantic judgment of the synthetic predicate"}],
        "delta":delta,"remaining_gap":"","blocker":null}).to_string()).unwrap();
    subject
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
        ("reroute_child_completed", "child", "child-turn-1"),
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
    for mode in [
        "reroute_unrelated",
        "reroute_stale",
        "reroute_invalid",
        "reroute_child_unknown",
    ] {
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
    run.set_state(luna_factoryd::control::RunControl::Blocked);
    let mut value = serde_json::to_value(&run).unwrap();
    value["dispatch_id"] = json!("missing-dispatch");
    run = serde_json::from_value(value).unwrap();
    store.save(&mut run).unwrap();
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
    store
        .apply_event(
            &mut run,
            &luna_factoryd::control::EventEnvelope {
                id: "persisted-dispatch".into(),
                expected_revision: 0,
                event: luna_factoryd::control::Event::Dispatch {
                    id: "persisted-dispatch".into(),
                    generation: 1,
                    repair: false,
                },
            },
        )
        .unwrap();
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
    store.save(&mut run).unwrap();
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
        store.save(&mut run).unwrap();
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

#[tokio::test]
async fn prose_only_completion_is_verify_not_convergence_and_keeps_claim() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"CONVERGED","subject":subject,"acceptance":[{"id":"A1","passed":true,"evidence":"Owner claims all tests pass"}],"delta":"Claimed completion","remaining_gap":"","blocker":null}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory.steer(id, "turn-1", "Return result").await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "BLOCKED" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result["claim_held"], true);
    assert_eq!(result["presentation"]["criteria"]["proven"], 0);
    assert_eq!(result["control"]["tasks"][0]["state"], "candidate");
    assert!(
        result["control"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "objective" && task["state"] == "verify")
    );
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn current_source_movement_demotes_old_success_without_native_work() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    predicate_report(&dir, &factory, "Observed file predicate");
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory.steer(id, "turn-1", "Return result").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if factory.get(id).await.unwrap()["state"] == "CONVERGED" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let count = calls(&dir).len();
    std::fs::write(
        factory.config.repositories["fixture"]
            .root
            .join("predicate.txt"),
        "changed source",
    )
    .unwrap();
    let stale = factory.get(id).await.unwrap();
    assert_eq!(stale["state"], "QUIESCENT");
    assert_eq!(stale["presentation"]["criteria"]["proven"], 0);
    assert_eq!(
        stale["presentation"]["result"]["kind"],
        "stopped_unresolved"
    );
    assert_eq!(calls(&dir).len(), count);
}
#[tokio::test]
async fn stale_ui_mutations_and_read_only_reconcile_use_same_current_projection() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let current = factory.get(id).await.unwrap();
    let revision = current["control"]["revision"].as_u64().unwrap();
    let count = calls(&dir).len();
    assert!(
        factory
            .steer_at_revision(id, "turn-1", "Correction", Some(revision.saturating_sub(1)))
            .await
            .is_err()
    );
    assert!(
        factory
            .cancel_at_revision(id, Some(revision.saturating_sub(1)))
            .await
            .is_err()
    );
    assert_eq!(calls(&dir).len(), count);
    let projected = current["presentation"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "resume")
        .unwrap();
    assert_eq!(projected["allowed"], false);
    assert!(
        factory
            .resume_at_revision(id, None, None, Some(revision))
            .await
            .is_err()
    );
    assert_eq!(calls(&dir).len(), count);
    let reconciled = factory.reconcile(id, Some(revision)).await.unwrap();
    assert_eq!(reconciled["owner_thread"], "owner");
    let after = calls(&dir);
    assert!(after[count..].iter().all(|call| !matches!(
        call["method"].as_str(),
        Some("thread/start" | "thread/resume" | "turn/start" | "turn/interrupt" | "turn/steer")
    )));
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn unproved_delivery_cannot_offer_retry_or_release_its_claim() {
    let (dir, base, mut request) = setup();
    let mut config = (*base.config).clone();
    drop(base);
    config.repositories.get_mut("fixture").unwrap().max_finish = "pr".into();
    request.finish = "pr".into();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    predicate_report(
        &dir,
        &factory,
        "Owner says PR delivered; file predicate is independent but delivery is not",
    );
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory.steer(id, "turn-1", "Return result").await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "BLOCKED" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let resume = result["presentation"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "resume")
        .unwrap();
    assert_eq!(
        resume["allowed"], false,
        "unproved external delivery offered a retry"
    );
    let count = calls(&dir).len();
    assert!(factory.resume(id).await.is_err());
    assert_eq!(
        calls(&dir).len(),
        count,
        "delivery uncertainty contacted native runtime before denial"
    );
    let stopped = factory.cancel(id).await.unwrap();
    assert_eq!(
        stopped["claim_held"], true,
        "unknown external effect released its claim"
    );
    assert!(
        stopped["delta"]
            .as_str()
            .unwrap()
            .contains("claim retained")
    );
}
#[tokio::test]
async fn uncertain_steer_intent_blocks_replay_of_the_same_native_correction() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    std::fs::write(dir.path().join("mode"), "lost_steer_ack").unwrap();
    assert!(
        factory
            .steer(id, "turn-1", "One bounded correction")
            .await
            .is_err()
    );
    let count = calls(&dir).len();
    assert!(
        factory
            .steer(id, "turn-1", "One bounded correction")
            .await
            .is_err()
    );
    assert_eq!(
        calls(&dir).len(),
        count,
        "uncertain steering effect was replayed"
    );
    let result = factory.cancel(id).await.unwrap();
    assert!(result["claim_held"] == true || result["state"] == "CANCELLED");
}
#[tokio::test]
async fn two_no_progress_returns_require_explicit_labelled_diagnosis_without_budget_reset() {
    let (dir, base, mut request) = setup();
    let mut config = (*base.config).clone();
    drop(base);
    config.limits.repair_attempts = 3;
    request.repair_attempts = 3;
    let factory = Factory::new(config).unwrap();
    let mut run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap().to_owned();
    let deadline = run["deadline_at"].clone();
    for attempt in 0..2 {
        let subject =
            luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
                .unwrap();
        std::fs::write(dir.path().join("report.json"),json!({"state":"BLOCKED","subject":subject,"acceptance":[],"checks":[],"delta":"No current criterion proof","remaining_gap":"Failed predicate remains","blocker":"Predicate unproved"}).to_string()).unwrap();
        std::fs::write(dir.path().join("mode"), "finish").unwrap();
        factory
            .steer(
                &id,
                run["turn_id"].as_str().unwrap(),
                "Return retained failure",
            )
            .await
            .unwrap();
        run = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let result = factory.get(&id).await.unwrap();
                if result["state"] == "BLOCKED" {
                    break result;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        if attempt == 0 {
            run = factory.resume(&id).await.unwrap();
        }
    }
    let count = calls(&dir).len();
    assert!(
        factory
            .resume_with_input(&id, Some("Ordinary text is not a diagnosis"))
            .await
            .unwrap_err()
            .to_string()
            .contains("diagnosis_required")
    );
    assert_eq!(
        calls(&dir).len(),
        count,
        "denied repair contacted native runtime"
    );
    let diagnosis = luna_factoryd::control::Diagnosis {
        summary: "The retained predicate is unchanged; narrow the correction".into(),
        basis: "operator_semantic".into(),
        check_refs: vec![],
        same_goal_replan: true,
    };
    let repaired = factory
        .resume_with_diagnosis(&id, None, None, None, Some(diagnosis.clone()))
        .await
        .unwrap();
    assert_eq!(repaired["repairs_used"], 2);
    assert_eq!(repaired["deadline_at"], deadline);
    assert_eq!(repaired["control"]["intent_generation"], 1);
    assert_eq!(repaired["control"]["attempts"].as_array().unwrap().len(), 3);
    factory.cancel(&id).await.unwrap();
    let count = calls(&dir).len();
    assert!(
        factory
            .resume_with_diagnosis(&id, None, None, None, Some(diagnosis))
            .await
            .unwrap_err()
            .to_string()
            .contains("replan_denied")
    );
    assert_eq!(calls(&dir).len(), count);
}
#[tokio::test]
async fn owner_candidate_selection_gates_the_real_next_native_packet() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    let candidate = json!({"id":"bounded-check","title":"One necessary bounded check","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]});
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":[candidate],"selected_task":"bounded-check","delta":"One bounded candidate proposed","remaining_gap":"A1 needs proof","blocker":"Continue the selected task?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return proposal")
        .await
        .unwrap();
    let proposed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "NEEDS_INPUT" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        proposed["control"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "bounded-check"
                && task["state"] == "ready"
                && task["admission"] == "admitted")
    );
    let resumed = factory
        .resume_with_decision(
            id,
            Some("Continue within the original objective"),
            proposed["pending_decision"]["id"].as_str(),
        )
        .await
        .unwrap();
    assert_eq!(
        resumed["control"]["attempts"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["task_id"],
        "bounded-check"
    );
    let history = calls(&dir);
    assert!(
        history
            .iter()
            .rev()
            .find(|call| call["method"] == "turn/start")
            .unwrap()["params"]
            .to_string()
            .contains("task_id=bounded-check")
    );
    assert_eq!(resumed["owner_thread"], run["owner_thread"]);
    assert_eq!(resumed["deadline_at"], run["deadline_at"]);
    predicate_report(
        &dir,
        &factory,
        "Selected task supplied a current independent predicate",
    );
    factory
        .steer(
            id,
            resumed["turn_id"].as_str().unwrap(),
            "Return current proof",
        )
        .await
        .unwrap();
    let accepted = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "CONVERGED" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        accepted["control"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "bounded-check" && task["state"] == "done")
    );
    assert_eq!(accepted["presentation"]["criteria"]["proven"], 1);
    assert_eq!(accepted["claim_held"], false);
}
#[tokio::test]
async fn rejected_owner_candidates_never_authorize_a_managed_dispatch() {
    for mode in [
        "missing",
        "cycle",
        "stale_assumption",
        "foreign_claim",
        "effect",
        "necessity",
    ] {
        let (dir, factory, request) = setup();
        let run = factory.start(request).await.unwrap();
        let id = run["id"].as_str().unwrap();
        let subject =
            luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
                .unwrap();
        let mut candidate = json!({"id":"bounded-check","title":"Proposed bounded check","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]});
        match mode {
            "missing" => candidate["dependencies"] = json!(["missing"]),
            "cycle" => candidate["dependencies"] = json!(["bounded-check"]),
            "stale_assumption" => candidate["assumptions"] = json!({"acceptance":"old"}),
            "foreign_claim" => candidate["claim"] = json!("foreign"),
            "effect" => candidate["effects"] = json!(["merge"]),
            "necessity" => candidate["necessary"] = json!(false),
            _ => unreachable!(),
        }
        std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":[candidate],"selected_task":"bounded-check","delta":"Proposed work","remaining_gap":"Admission is unresolved","blocker":"Continue?"}).to_string()).unwrap();
        std::fs::write(dir.path().join("mode"), "finish").unwrap();
        factory
            .steer(id, "turn-1", "Return proposal")
            .await
            .unwrap();
        let proposed = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let result = factory.get(id).await.unwrap();
                if result["state"] == "BLOCKED" {
                    break result;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            !proposed["control"]["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|task| task["id"] == "bounded-check" && task["state"] == "ready"),
            "{mode} became READY"
        );
        let count = calls(&dir).len();
        assert!(factory.resume(id).await.is_err(), "{mode} dispatched");
        assert_eq!(
            calls(&dir).len(),
            count,
            "{mode} contacted native runtime on denial"
        );
        factory.cancel(id).await.unwrap();
    }
}
#[tokio::test]
async fn ignored_file_predicate_changes_invalidate_current_completion() {
    let (dir, factory, request) = setup();
    let root = &factory.config.repositories["fixture"].root;
    std::fs::write(root.join(".gitignore"), "predicate.txt\n").unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject = predicate_report(&dir, &factory, "Ignored artifact was independently checked");
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory.steer(id, "turn-1", "Return proof").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if factory.get(id).await.unwrap()["state"] == "CONVERGED" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(root.join("predicate.txt"), "changed ignored artifact").unwrap();
    assert_eq!(
        luna_factoryd::store::repository_subject(root).unwrap(),
        subject
    );
    let count = calls(&dir).len();
    let changed = factory.get(id).await.unwrap();
    assert_ne!(changed["state"], "CONVERGED");
    assert_eq!(changed["presentation"]["criteria"]["proven"], 0);
    assert_eq!(calls(&dir).len(), count);
}
#[tokio::test]
async fn observed_push_remains_unknown_even_when_owner_requests_an_answer() {
    let (dir, base, mut request) = setup();
    let mut config = (*base.config).clone();
    drop(base);
    config.repositories.get_mut("fixture").unwrap().max_finish = "push".into();
    request.finish = "push".into();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"delta":"Push result is uncertain","remaining_gap":"Remote effect needs settlement","blocker":"Retry the uncertain push?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish_external").unwrap();
    factory
        .steer(id, "turn-1", "Return the actual uncertainty")
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "NEEDS_INPUT" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let answer = result["presentation"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "answer")
        .unwrap();
    assert_eq!(answer["allowed"], false);
    let count = calls(&dir).len();
    assert!(
        factory
            .resume_with_decision(
                id,
                Some("Do not repeat an uncertain effect"),
                result["pending_decision"]["id"].as_str()
            )
            .await
            .is_err()
    );
    assert_eq!(calls(&dir).len(), count);
    assert_eq!(factory.cancel(id).await.unwrap()["claim_held"], true);
}
#[tokio::test]
async fn all_criteria_proven_does_not_finish_an_outstanding_necessary_selected_task() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    predicate_report(
        &dir,
        &factory,
        "Owner claims convergence while selecting undispatched necessary work",
    );
    let path = dir.path().join("report.json");
    let mut report: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    report["candidates"] = json!([{"id":"undispatched","title":"Claimed necessary work","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]}]);
    report["selected_task"] = json!("undispatched");
    std::fs::write(&path, report.to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return both claims")
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if ["CONVERGED", "BLOCKED"]
                .iter()
                .any(|state| result["state"] == *state)
            {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_ne!(result["state"], "CONVERGED");
    assert_ne!(
        result["presentation"]["result"]["kind"],
        "finished_verified"
    );
    assert_eq!(result["claim_held"], true);
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn previously_admitted_necessary_sibling_cannot_be_done_by_another_tasks_receipt() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    let candidates:Vec<_>=["task-a","task-b"].iter().map(|id|json!({"id":id,"title":"Necessary bounded work","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]})).collect();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":candidates,"selected_task":"task-a","delta":"Two necessary candidates","remaining_gap":"A1 needs proof","blocker":"Continue A?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return candidates")
        .await
        .unwrap();
    let proposed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "NEEDS_INPUT" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second = factory
        .resume_with_decision(
            id,
            Some("Continue A within the same goal"),
            proposed["pending_decision"]["id"].as_str(),
        )
        .await
        .unwrap();
    predicate_report(&dir, &factory, "Only A supplied a task-scoped predicate");
    factory
        .steer(id, second["turn_id"].as_str().unwrap(), "Return A proof")
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if ["CONVERGED", "BLOCKED"]
                .iter()
                .any(|state| result["state"] == *state)
            {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_ne!(result["state"], "CONVERGED");
    assert_eq!(result["claim_held"], true);
    assert!(
        result["control"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "task-b" && task["state"] == "ready")
    );
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn failed_selected_task_repairs_its_observed_output_without_replacement() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let deadline = run["deadline_at"].clone();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":[{"id":"repairable","title":"One necessary task","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]}],"selected_task":"repairable","delta":"Bounded selected work","remaining_gap":"A1 needs proof","blocker":"Continue?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return proposal")
        .await
        .unwrap();
    let proposed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "NEEDS_INPUT" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second = factory
        .resume_with_decision(
            id,
            Some("Continue"),
            proposed["pending_decision"]["id"].as_str(),
        )
        .await
        .unwrap();
    std::fs::write(
        factory.config.repositories["fixture"]
            .root
            .join("failed-output.txt"),
        "Retained failed candidate",
    )
    .unwrap();
    let output =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"BLOCKED","subject":output,"acceptance":[],"checks":[],"delta":"Failed predicate with retained output","remaining_gap":"A1 remains unproved","blocker":"Repair the failed predicate"}).to_string()).unwrap();
    factory
        .steer(id, second["turn_id"].as_str().unwrap(), "Return failure")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if factory.get(id).await.unwrap()["state"] == "BLOCKED" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let diagnosis = luna_factoryd::control::Diagnosis {
        summary: "Inspect retained failed output and correct the predicate".into(),
        basis: "operator_semantic".into(),
        check_refs: vec![],
        same_goal_replan: false,
    };
    let repaired = factory
        .resume_with_diagnosis(id, None, None, None, Some(diagnosis))
        .await
        .unwrap();
    let attempt = repaired["control"]["attempts"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(attempt["task_id"], "repairable");
    assert_eq!(attempt["subject"], output);
    assert_eq!(repaired["deadline_at"], deadline);
    assert_eq!(repaired["repairs_used"], 1);
    assert_eq!(repaired["control"]["intent_generation"], 1);
    factory.cancel(id).await.unwrap();
}
#[tokio::test]
async fn optional_rejected_discovery_does_not_reopen_a_verified_objective() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    predicate_report(&dir, &factory, "Current mandatory proof");
    let path = dir.path().join("report.json");
    let mut report: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    report["candidates"] = json!([{"id":"optional-note","title":"Unnecessary discovery","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":false,"effects":["native_owner_turn"]}]);
    report["selected_task"] = json!("objective");
    std::fs::write(&path, report.to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return bounded result")
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "CONVERGED" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result["claim_held"], false);
    assert!(
        result["control"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "optional-note" && task["state"] == "blocked")
    );
}
#[tokio::test]
async fn sequential_necessary_tasks_finish_only_after_explicit_current_reattestation() {
    use sha2::{Digest, Sha256};
    let (dir, factory, request) = setup();
    let root = &factory.config.repositories["fixture"].root;
    std::fs::write(root.join("read-only-proof.txt"), "stable predicate").unwrap();
    let subject = luna_factoryd::store::repository_subject(root).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"stable predicate"));
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let candidates:Vec<_>=["task-a","task-b"].iter().map(|id|json!({"id":id,"title":"Necessary read-only work","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["native_owner_turn"]})).collect();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":candidates,"selected_task":"task-a","delta":"Select A then B","remaining_gap":"Both own proofs required","blocker":"Continue A?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory.steer(id, "turn-1", "Return tasks").await.unwrap();
    async fn wait(factory: &Factory, id: &str, state: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let result = factory.get(id).await.unwrap();
                if result["state"] == state {
                    break result;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    let first = wait(&factory, id, "NEEDS_INPUT").await;
    let a = factory
        .resume_with_decision(
            id,
            Some("Continue A"),
            first["pending_decision"]["id"].as_str(),
        )
        .await
        .unwrap();
    let a_attempt = a["control"]["attempts"].as_array().unwrap().last().unwrap()["id"].clone();
    let a_check = json!({"id":"a-check","kind":"file_sha256","path":"read-only-proof.txt","sha256":digest,"native_item":null,"binding":{"task_id":"task-a","attempt_id":a_attempt,"intent_generation":1,"dispatch_generation":2,"subject":subject,"assumptions":{}}});
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[{"id":"A1","passed":true,"accepted":true,"evidence":"A predicate is observed; B still necessary","check_refs":["a-check"]}],"checks":[a_check],"candidates":[],"selected_task":"task-b","delta":"A observed; B remains","remaining_gap":"B needs scoped proof","blocker":"Continue B?"}).to_string()).unwrap();
    factory
        .steer(id, a["turn_id"].as_str().unwrap(), "Return A and select B")
        .await
        .unwrap();
    let next = wait(&factory, id, "NEEDS_INPUT").await;
    let b = factory
        .resume_with_decision(
            id,
            Some("Continue B"),
            next["pending_decision"]["id"].as_str(),
        )
        .await
        .unwrap();
    let b_attempt = b["control"]["attempts"].as_array().unwrap().last().unwrap()["id"].clone();
    let b_check = json!({"id":"b-check","kind":"file_sha256","path":"read-only-proof.txt","sha256":digest,"native_item":null,"binding":{"task_id":"task-b","attempt_id":b_attempt,"intent_generation":1,"dispatch_generation":3,"subject":subject,"assumptions":{}}});
    std::fs::write(dir.path().join("report.json"),json!({"state":"CONVERGED","subject":subject,"acceptance":[{"id":"A1","passed":true,"accepted":true,"evidence":"Both current task-scoped predicates explicitly rechecked","check_refs":["a-check","b-check"]}],"checks":[a_check,b_check],"candidates":[],"selected_task":"task-b","delta":"Both necessary tasks verified","remaining_gap":"","blocker":null}).to_string()).unwrap();
    factory
        .steer(
            id,
            b["turn_id"].as_str().unwrap(),
            "Explicitly re-attest A and B",
        )
        .await
        .unwrap();
    let done = wait(&factory, id, "CONVERGED").await;
    assert_eq!(done["claim_held"], false);
    for task in ["task-a", "task-b"] {
        assert!(
            done["control"]["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["id"] == task && item["state"] == "done")
        );
    }
}
#[tokio::test]
async fn read_only_check_task_never_permits_native_owner_turn() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let subject =
        luna_factoryd::store::repository_subject(&factory.config.repositories["fixture"].root)
            .unwrap();
    std::fs::write(dir.path().join("report.json"),json!({"state":"NEEDS_INPUT","subject":subject,"acceptance":[],"checks":[],"candidates":[{"id":"file-only","title":"Only a file check","criterion_ids":["A1"],"dependencies":[],"assumptions":{},"necessary":true,"effects":["read_only_file_check"]}],"selected_task":"file-only","delta":"Only file effects permitted","remaining_gap":"No native turn permit","blocker":"Continue?"}).to_string()).unwrap();
    std::fs::write(dir.path().join("mode"), "finish").unwrap();
    factory
        .steer(id, "turn-1", "Return effect bounds")
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = factory.get(id).await.unwrap();
            if result["state"] == "NEEDS_INPUT" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        result["presentation"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action["kind"] == "answer")
            .unwrap()["allowed"],
        false
    );
    let count = calls(&dir).len();
    assert!(
        factory
            .resume_with_decision(
                id,
                Some("Keep effects read-only"),
                result["pending_decision"]["id"].as_str()
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("native_owner_turn_not_permitted")
    );
    assert_eq!(calls(&dir).len(), count);
    factory.cancel(id).await.unwrap();
}

#[tokio::test]
async fn identical_repository_replacement_before_or_during_report_read_retains_claim() {
    for mode in ["finish_identity_drift", "finish_identity_late"] {
        let (dir, factory, request) = setup();
        let started = factory.start(request).await.unwrap();
        let id = started["id"].as_str().unwrap();
        let stamp = luna_factoryd::store::Store::open(&factory.config)
            .unwrap()
            .get(id)
            .unwrap()
            .graph_repository_stamp;
        completed_report(&dir, &factory);
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        factory
            .steer(id, "turn-1", "Return the scoped fixture proof")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let current = factory.get(id).await.unwrap();
                if current["state"] == "BLOCKED" || current["state"] == "CONVERGED" {
                    assert_ne!(current["state"], "CONVERGED", "{mode}");
                    assert_eq!(current["claim_held"], true, "{mode}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(factory.graph(id).await.is_err(), "{mode}");
        let _ = factory.reconcile(id, None).await;
        let cancelled = factory.cancel(id).await.unwrap();
        assert_eq!(cancelled["claim_held"], true, "{mode}");
        let stored = luna_factoryd::store::Store::open(&factory.config)
            .unwrap()
            .get(id)
            .unwrap();
        assert_eq!(stored.graph_repository_stamp, stamp, "{mode}");
        assert!(stored.claim_held, "{mode}");
        assert_eq!(
            calls(&dir)
                .iter()
                .filter(|call| call["method"] == "turn/start")
                .count(),
            1,
            "{mode}"
        );
    }
}
