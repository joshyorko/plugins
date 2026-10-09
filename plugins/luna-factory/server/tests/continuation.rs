//! Live-monitor continuation through a disposable protocol fixture, never real Codex.
use luna_factoryd::{
    config::Config,
    lifecycle::Factory,
    store::{StartRequest, Store},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

fn setup() -> (tempfile::TempDir, Factory, StartRequest) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
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
    std::fs::write(repo.join("proof.txt"), "stable proof").unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/native/fake_continuation.py");
    let binary = dir.path().join("native-fixture");
    std::fs::copy(fixture, &binary).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "Synthetic continuation fixture; no inference.").unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":binary,"skill_path":skill,"repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":2,"repair_attempts":2,"wall_seconds":300}})).unwrap();
    let request = serde_json::from_value(json!({"repository":"fixture","objective":"Bounded continuation fixture","acceptance":["A1 current task-scoped proof"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":2,"wall_seconds":300,"idempotency_key":"continuation"})).unwrap();
    (dir, Factory::new(config).unwrap(), request)
}

fn calls(dir: &tempfile::TempDir, method: &str) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| call["method"] == method)
        .collect()
}
fn candidate(id: &str, dependencies: &[&str]) -> Value {
    json!({"id":id,"title":"Necessary bounded work","criterion_ids":["A1"],"dependencies":dependencies,"assumptions":{},"necessary":true,"effects":["native_owner_turn"]})
}
fn next(task: &str) -> Value {
    json!({"kind":"next","task_id":task,"diagnosis":null})
}
fn report(run: &Value) -> Value {
    json!({"state":"QUIESCENT","subject":run["current_subject"],"acceptance":[],"checks":[],"candidates":[],"selected_task":"objective","continuation":null,"delta":"Bounded work remains","remaining_gap":"A1 requires current proof","blocker":null})
}
async fn finish(dir: &tempfile::TempDir, factory: &Factory, run: &Value, report: Value) {
    std::fs::write(dir.path().join("report.json"), report.to_string()).unwrap();
    factory
        .steer(
            run["id"].as_str().unwrap(),
            run["turn_id"].as_str().unwrap(),
            "Return the fixture result",
        )
        .await
        .unwrap();
}
async fn wait_for(factory: &Factory, id: &str, matches: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let run = factory.get(id).await.unwrap();
            if matches(&run) {
                break run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("continuation predicate timed out for {id}"))
}
fn proof(run: &Value, id: &str, valid: bool) -> Value {
    let attempt = run["control"]["attempts"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    json!({"id":id,"kind":"file_sha256","path":"proof.txt","sha256":format!("{:x}",Sha256::digest(if valid {b"stable proof".as_slice()} else {b"expected other content".as_slice()})),"native_item":null,"binding":{"task_id":attempt["task_id"],"attempt_id":attempt["id"],"intent_generation":1,"dispatch_generation":attempt["dispatch_generation"],"subject":run["current_subject"],"assumptions":{}}})
}
fn acceptance(refs: &[&str]) -> Value {
    json!([{"id":"A1","passed":true,"accepted":true,"evidence":"Explicitly checked current scoped file predicates","check_refs":refs}])
}

#[tokio::test]
async fn two_tasks_continue_without_external_resume_and_keep_original_lineage() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 0;
    let original = factory.start(request).await.unwrap();
    let id = original["id"].as_str().unwrap();
    let mut first = report(&original);
    first["candidates"] = json!([candidate("task-a", &[]), candidate("task-b", &[])]);
    first["selected_task"] = json!("task-a");
    first["continuation"] = next("task-a");
    finish(&dir, &factory, &original, first).await;
    let a = wait_for(&factory, id, |run| {
        run["generation"] == 2 && run["state"] == "RUNNING"
    })
    .await;
    let a_check = proof(&a, "a-check", true);
    let mut second = report(&a);
    second["checks"] = json!([a_check]);
    second["acceptance"] = acceptance(&["a-check"]);
    second["selected_task"] = json!("task-b");
    second["continuation"] = next("task-b");
    finish(&dir, &factory, &a, second).await;
    let b = wait_for(&factory, id, |run| {
        run["generation"] == 3 && run["state"] == "RUNNING"
    })
    .await;
    let mut final_report = report(&b);
    final_report["state"] = json!("CONVERGED");
    final_report["remaining_gap"] = json!("");
    final_report["selected_task"] = json!("task-b");
    final_report["checks"] = json!([a_check, proof(&b, "b-check", true)]);
    final_report["acceptance"] = acceptance(&["a-check", "b-check"]);
    finish(&dir, &factory, &b, final_report).await;
    let done = wait_for(&factory, id, |run| run["state"] == "CONVERGED").await;
    assert_eq!(done["deadline_at"], original["deadline_at"]);
    assert_eq!(done["owner_thread"], original["owner_thread"]);
    assert_eq!(done["repairs_used"], 0);
    assert_eq!(done["control"]["intent_generation"], 1);
    assert_eq!(done["claim_held"], false);
    assert_eq!(calls(&dir, "turn/start").len(), 3);
    assert_eq!(calls(&dir, "thread/start").len(), 1);
    assert!(calls(&dir, "thread/resume").is_empty());
    let raw = Store::open(&factory.config).unwrap().get(id).unwrap();
    let control = raw.control.unwrap();
    for task in ["task-a", "task-b"] {
        assert_eq!(
            control.tasks[task].state,
            luna_factoryd::control::TaskState::Done
        );
        assert!(
            control
                .checks
                .iter()
                .any(|check| check.binding.task_id == task && check.outcome == "passed")
        );
    }
}

async fn returned(factory: &Factory, id: &str) -> Value {
    wait_for(factory, id, |run| {
        !matches!(
            run["state"].as_str(),
            Some("STARTING" | "RUNNING" | "VERIFYING")
        ) && run["control"]["attempts"]
            .as_array()
            .unwrap()
            .last()
            .is_some_and(|a| a["status"] != "active")
    })
    .await
}

#[tokio::test]
async fn failed_file_predicate_repairs_same_task_once_with_original_budget_and_deadline() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 1;
    let original = factory.start(request).await.unwrap();
    let id = original["id"].as_str().unwrap();
    let mut failure = report(&original);
    failure["checks"] = json!([proof(&original, "failed-first", false)]);
    failure["continuation"] = json!({"kind":"repair","task_id":"objective","diagnosis":{"summary":"Correct the observed file mismatch","check_refs":["failed-first"]}});
    finish(&dir, &factory, &original, failure).await;
    let repaired = wait_for(&factory, id, |run| {
        run["generation"] == 2 && run["state"] == "RUNNING"
    })
    .await;
    assert_eq!(repaired["repairs_used"], 1);
    assert_eq!(repaired["deadline_at"], original["deadline_at"]);
    assert_eq!(repaired["owner_thread"], original["owner_thread"]);
    let raw = Store::open(&factory.config).unwrap().get(id).unwrap();
    let control = raw.control.as_ref().unwrap();
    assert_eq!(control.attempts[1].task_id, "objective");
    assert_eq!(
        control.attempts[1].parent.as_deref(),
        Some(control.attempts[0].id.as_str())
    );
    assert_eq!(control.attempts[1].intent_generation, 1);
    assert_eq!(
        control.attempts[1].diagnosis.as_ref().unwrap().basis,
        "observed_failure"
    );
    assert!(
        control
            .checks
            .iter()
            .any(|c| c.id == "failed-first" && c.outcome == "failed")
    );
    let mut again = report(&repaired);
    again["checks"] = json!([proof(&repaired, "failed-second", false)]);
    again["continuation"] = json!({"kind":"repair","task_id":"objective","diagnosis":{"summary":"Second observed mismatch","check_refs":["failed-second"]}});
    finish(&dir, &factory, &repaired, again).await;
    returned(&factory, id).await;
    // Reconcile takes the same mutation lock as the live completion handler.
    // This avoids mistaking its intermediate QUIESCENT snapshot for a stop.
    let _ = factory.reconcile(id, None).await;
    let stopped = factory.get(id).await.unwrap();
    assert_eq!(stopped["generation"], 2);
    assert_eq!(stopped["repairs_used"], 1);
    assert_eq!(stopped["deadline_at"], original["deadline_at"]);
    assert_eq!(calls(&dir, "turn/start").len(), 2);
    assert!(calls(&dir, "thread/resume").is_empty());
}

#[tokio::test]
async fn real_decision_stops_automatic_work_and_answer_replays_without_another_turn() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 0;
    let original = factory.start(request).await.unwrap();
    let id = original["id"].as_str().unwrap();
    let mut decision = report(&original);
    decision["state"] = json!("NEEDS_INPUT");
    decision["blocker"] = json!("Choose the bounded implementation?");
    decision["candidates"] = json!([candidate("task-a", &[])]);
    decision["selected_task"] = json!("task-a");
    decision["continuation"] = next("task-a");
    finish(&dir, &factory, &original, decision).await;
    let stopped = wait_for(&factory, id, |run| run["state"] == "NEEDS_INPUT").await;
    assert_eq!(calls(&dir, "turn/start").len(), 1);
    let decision_id = stopped["pending_decision"]["id"].as_str().unwrap();
    let answered = factory
        .resume_with_decision(
            id,
            Some("Use the bounded implementation"),
            Some(decision_id),
        )
        .await
        .unwrap();
    let replay = factory
        .resume_with_decision(
            id,
            Some("Use the bounded implementation"),
            Some(decision_id),
        )
        .await
        .unwrap();
    assert_eq!(answered["turn_id"], replay["turn_id"]);
    assert_eq!(answered["owner_thread"], original["owner_thread"]);
    assert_eq!(answered["deadline_at"], original["deadline_at"]);
    assert_eq!(answered["repairs_used"], 0);
    assert_eq!(calls(&dir, "turn/start").len(), 2);
    assert!(
        factory
            .resume_with_decision(id, Some("Different answer"), Some(decision_id))
            .await
            .is_err()
    );
    factory.cancel(id).await.unwrap();
}

#[tokio::test]
async fn manual_fresh_ready_task_needs_no_repair_budget_and_is_presented_as_resumable() {
    let (dir, factory, mut request) = setup();
    request.repair_attempts = 0;
    let original = factory.start(request).await.unwrap();
    let id = original["id"].as_str().unwrap();
    let mut result = report(&original);
    result["candidates"] = json!([candidate("fresh-task", &[])]);
    result["selected_task"] = json!("fresh-task");
    // Explicit null preserves the manual control path, even though READY work exists.
    finish(&dir, &factory, &original, result).await;
    returned(&factory, id).await;
    let _ = factory.reconcile(id, None).await;
    let stopped = factory.get(id).await.unwrap();
    assert_eq!(calls(&dir, "turn/start").len(), 1);
    let resume = stopped["presentation"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "resume")
        .unwrap();
    assert_eq!(resume["allowed"], true, "{stopped}");
    let resumed = factory.resume(id).await.unwrap();
    assert_eq!(resumed["generation"], 2);
    assert_eq!(resumed["repairs_used"], 0);
    assert_eq!(resumed["deadline_at"], original["deadline_at"]);
    assert_eq!(resumed["owner_thread"], original["owner_thread"]);
    assert_eq!(
        resumed["control"]["attempts"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["task_id"],
        "fresh-task"
    );
    assert_eq!(calls(&dir, "turn/start").len(), 2);
    factory.cancel(id).await.unwrap();
}

#[tokio::test]
async fn independent_ready_task_continues_while_unrelated_dependency_stays_blocked() {
    let (dir, factory, request) = setup();
    let original = factory.start(request).await.unwrap();
    let id = original["id"].as_str().unwrap();
    let mut result = report(&original);
    result["candidates"] = json!([
        candidate("blocked", &["objective"]),
        candidate("independent", &[])
    ]);
    result["selected_task"] = json!("independent");
    result["continuation"] = next("independent");
    finish(&dir, &factory, &original, result).await;
    let active = wait_for(&factory, id, |run| {
        run["generation"] == 2 && run["state"] == "RUNNING"
    })
    .await;
    let raw = Store::open(&factory.config).unwrap().get(id).unwrap();
    assert_eq!(
        raw.control
            .as_ref()
            .unwrap()
            .attempts
            .last()
            .unwrap()
            .task_id,
        "independent"
    );
    assert_eq!(
        raw.control.as_ref().unwrap().tasks["blocked"].state,
        luna_factoryd::control::TaskState::Blocked
    );
    assert_eq!(active["repairs_used"], 0);
    assert_eq!(calls(&dir, "turn/start").len(), 2);
    assert!(calls(&dir, "thread/resume").is_empty());
    factory.cancel(id).await.unwrap();
}

#[tokio::test]
async fn stale_source_unknown_child_and_unknown_effect_never_authorize_next_turn() {
    for condition in [
        "stale_source",
        "unknown_child",
        "unknown_effect",
        "authority_drift",
    ] {
        let (dir, factory, request) = setup();
        let original = factory.start(request).await.unwrap();
        let id = original["id"].as_str().unwrap();
        let mut result = report(&original);
        result["candidates"] = json!([candidate("task-a", &[])]);
        result["selected_task"] = json!("task-a");
        result["continuation"] = next("task-a");
        if condition == "stale_source" {
            std::fs::write(
                factory.config.repositories["fixture"]
                    .root
                    .join("drift.txt"),
                "changed after report subject",
            )
            .unwrap();
        } else {
            std::fs::write(dir.path().join("mode"), condition).unwrap();
        }
        finish(&dir, &factory, &original, result).await;
        wait_for(&factory, id, |run| {
            !matches!(
                run["state"].as_str(),
                Some("STARTING" | "RUNNING" | "VERIFYING")
            )
        })
        .await;
        let _ = factory.reconcile(id, None).await;
        let stopped = factory.get(id).await.unwrap();
        assert_eq!(stopped["generation"], 1, "{condition}: {stopped}");
        assert_eq!(stopped["repairs_used"], 0, "{condition}");
        assert_eq!(calls(&dir, "turn/start").len(), 1, "{condition}");
        if condition != "stale_source" {
            assert_eq!(stopped["claim_held"], true, "{condition}");
        }
    }
}

#[test]
fn recovery_of_completed_report_never_runs_its_continuation_directive() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (dir, config, original) = runtime.block_on(async {
        let (dir, base, request) = setup();
        let mut config = (*base.config).clone();
        drop(base);
        config.native_transport = "existing_daemon".into();
        let factory = Factory::new(config.clone()).unwrap();
        let original = factory.start(request).await.unwrap();
        let mut result = report(&original);
        result["candidates"] = json!([candidate("task-a", &[])]);
        result["selected_task"] = json!("task-a");
        result["continuation"] = next("task-a");
        std::fs::write(dir.path().join("mode"), "completion_without_notification").unwrap();
        finish(&dir, &factory, &original, result).await;
        assert_eq!(calls(&dir, "turn/start").len(), 1);
        (dir, config, original)
    });
    drop(runtime);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let id = original["id"].as_str().unwrap();
        let factory = Factory::new(config).unwrap();
        factory.reconcile_startup().await.unwrap();
        let _ = factory.reconcile(id, None).await;
        let recovered = factory.get(id).await.unwrap();
        assert_eq!(calls(&dir, "turn/start").len(), 1);
        assert_eq!(recovered["generation"], 1);
        assert_eq!(recovered["repairs_used"], 0);
        assert_eq!(recovered["deadline_at"], original["deadline_at"]);
        assert_eq!(recovered["owner_thread"], original["owner_thread"]);
        let raw = Store::open(&factory.config).unwrap().get(id).unwrap();
        assert_eq!(raw.control.as_ref().unwrap().attempts.len(), 1);
    });
}

#[tokio::test]
async fn blocked_absent_malformed_and_unverified_failure_directives_do_not_continue() {
    for condition in [
        "blocked",
        "absent",
        "malformed",
        "unverified_failure",
        "no_repair_budget",
        "blocker_object",
        "blocker_boolean",
        "blocker_missing",
        "diagnosis_missing",
        "checks_missing",
        "candidates_missing",
        "remaining_gap_missing",
        "selected_task_missing",
    ] {
        let (dir, factory, mut request) = setup();
        if condition == "no_repair_budget" {
            request.repair_attempts = 0;
        }
        let original = factory.start(request).await.unwrap();
        let id = original["id"].as_str().unwrap();
        let mut result = report(&original);
        match condition {
            "blocked" => {
                result["state"] = json!("BLOCKED");
                result["blocker"] = json!("Explicit blocker cannot be bypassed");
                result["candidates"] = json!([candidate("task-a", &[])]);
                result["selected_task"] = json!("task-a");
                result["continuation"] = next("task-a");
            }
            "absent" => {
                result.as_object_mut().unwrap().remove("continuation");
            }
            "malformed" => {
                result["continuation"] = json!({"kind":"next","task_id":"objective","diagnosis":null,"authority":"override"});
            }
            "blocker_object"
            | "blocker_boolean"
            | "blocker_missing"
            | "diagnosis_missing"
            | "checks_missing"
            | "candidates_missing"
            | "remaining_gap_missing"
            | "selected_task_missing" => {
                result["candidates"] = json!([candidate("task-a", &[])]);
                result["selected_task"] = json!("task-a");
                result["continuation"] = next("task-a");
                match condition {
                    "blocker_object" => result["blocker"] = json!({}),
                    "blocker_boolean" => result["blocker"] = json!(true),
                    "diagnosis_missing" => {
                        result["continuation"]
                            .as_object_mut()
                            .unwrap()
                            .remove("diagnosis");
                    }
                    _ => {
                        result
                            .as_object_mut()
                            .unwrap()
                            .remove(condition.strip_suffix("_missing").unwrap());
                    }
                }
            }
            _ => {
                let mut check = proof(&original, "failed", false);
                if condition == "unverified_failure" {
                    check["path"] = json!("missing-proof.txt");
                }
                result["checks"] = json!([check]);
                result["continuation"] = json!({"kind":"repair","task_id":"objective","diagnosis":{"summary":"Only an observed failure may authorize repair","check_refs":["failed"]}});
            }
        }
        finish(&dir, &factory, &original, result).await;
        wait_for(&factory, id, |run| {
            !matches!(
                run["state"].as_str(),
                Some("STARTING" | "RUNNING" | "VERIFYING")
            )
        })
        .await;
        let _ = factory.reconcile(id, None).await;
        let stopped = factory.get(id).await.unwrap();
        assert_eq!(stopped["generation"], 1, "{condition}: {stopped}");
        assert_eq!(stopped["repairs_used"], 0, "{condition}");
        assert_eq!(calls(&dir, "turn/start").len(), 1, "{condition}");
    }
}

#[test]
fn lost_continuation_ack_reconcile_and_restart_never_redispatch() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (dir, config, original) = runtime.block_on(async {
        let (dir, base, request) = setup();
        let mut config = (*base.config).clone();
        drop(base);
        config.native_transport = "existing_daemon".into();
        let factory = Factory::new(config.clone()).unwrap();
        let original = factory.start(request).await.unwrap();
        let id = original["id"].as_str().unwrap();
        let mut result = report(&original);
        result["candidates"] = json!([candidate("task-a", &[])]);
        result["selected_task"] = json!("task-a");
        result["continuation"] = next("task-a");
        std::fs::write(dir.path().join("mode"), "lost_ack").unwrap();
        finish(&dir, &factory, &original, result).await;
        let unknown = wait_for(&factory, id, |run| {
            run["generation"] == 2
                && matches!(run["state"].as_str(), Some("BLOCKED" | "INTERRUPTED"))
        })
        .await;
        assert_eq!(unknown["claim_held"], true);
        assert_eq!(calls(&dir, "turn/start").len(), 2);
        let _ = factory.reconcile(id, None).await;
        assert_eq!(calls(&dir, "turn/start").len(), 2);
        (dir, config, original)
    });
    // Dropping this isolated runtime models process loss, including its monitor.
    // No cancellation or fresh execution is used to make recovery easier.
    drop(runtime);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let id = original["id"].as_str().unwrap();
        let restarted = Factory::new(config).unwrap();
        let _ = restarted.reconcile(id, None).await;
        assert_eq!(calls(&dir, "turn/start").len(), 2);
        let final_run = restarted.get(id).await.unwrap();
        assert_eq!(final_run["deadline_at"], original["deadline_at"]);
        assert_eq!(final_run["generation"], 2);
        assert_eq!(final_run["repairs_used"], 0);
    });
}

#[tokio::test]
async fn replaced_repository_identity_cannot_accept_completion_or_release_claim() {
    let (dir, factory, request) = setup();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    let mut final_report = report(&run);
    final_report["state"] = json!("CONVERGED");
    final_report["remaining_gap"] = json!("");
    final_report["checks"] = json!([proof(&run, "identity-proof", true)]);
    final_report["acceptance"] = acceptance(&["identity-proof"]);
    // The fixture replaces .git after steer validation, just before its live
    // completion notification. HEAD and all source contents stay identical.
    std::fs::write(dir.path().join("mode"), "identity_drift").unwrap();
    finish(&dir, &factory, &run, final_report).await;
    let result = wait_for(&factory, id, |value| {
        value["state"] == "BLOCKED" || value["state"] == "CONVERGED"
    })
    .await;
    assert!(factory.graph(id).await.is_err());
    assert_ne!(result["state"], "CONVERGED");
    assert_eq!(result["claim_held"], true);
    let stored = Store::open(&factory.config).unwrap().get(id).unwrap();
    assert_ne!(stored.state, "CONVERGED");
    assert!(stored.claim_held);
    let cancelled = factory.cancel(id).await.unwrap();
    assert_eq!(cancelled["claim_held"], true);
    assert_eq!(cancelled["blocker"], "repository_identity_unverified");
}

#[tokio::test]
async fn final_native_preflight_cannot_bypass_source_skill_or_ignored_file_fences() {
    for mode in [
        "late_source_drift",
        "late_skill_drift",
        "late_ignored_predicate_drift",
    ] {
        let (dir, factory, request) = setup();
        if mode == "late_ignored_predicate_drift" {
            std::fs::write(dir.path().join("repo/.git/info/exclude"), "proof.txt\n").unwrap();
        }
        let original = factory.start(request).await.unwrap();
        let id = original["id"].as_str().unwrap();
        let mut first = report(&original);
        first["candidates"] = json!([candidate("task-a", &[]), candidate("task-b", &[])]);
        first["selected_task"] = json!("task-a");
        first["continuation"] = next("task-a");
        finish(&dir, &factory, &original, first).await;
        let a = wait_for(&factory, id, |run| {
            run["generation"] == 2 && run["state"] == "RUNNING"
        })
        .await;
        let mut second = report(&a);
        second["checks"] = json!([proof(&a, "a-check", true)]);
        second["acceptance"] = acceptance(&["a-check"]);
        second["selected_task"] = json!("task-b");
        second["continuation"] = next("task-b");
        std::fs::write(dir.path().join("mode"), mode).unwrap();
        finish(&dir, &factory, &a, second).await;
        let stopped = wait_for(&factory, id, |run| run["state"] == "BLOCKED").await;
        assert_eq!(calls(&dir, "turn/start").len(), 2, "{mode}");
        assert_eq!(stopped["claim_held"], true, "{mode}");
        // Intent preceded preflight; refusing its first byte never gives
        // recovery permission to synthesize/retry an inference call.
        let _ = factory.reconcile(id, None).await;
        assert_eq!(calls(&dir, "turn/start").len(), 2, "{mode}");
        assert!(
            Store::open(&factory.config)
                .unwrap()
                .get(id)
                .unwrap()
                .claim_held
        );
    }
}
