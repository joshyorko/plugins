//! Matching Review/Changeplane invariants; these tests do not prove native policy hooks.
use luna_factoryd::control::{Control, Event, EventEnvelope, TaskState, reduce};

fn initial() -> Control {
    Control::new("subject-1", &["Mandatory result".into()], 3, 600).unwrap()
}
#[test]
fn revisions_dispatch_generations_and_return_do_not_certify_completion() {
    let control = initial();
    let event = EventEnvelope {
        id: "dispatch-1".into(),
        expected_revision: 0,
        event: Event::Dispatch {
            id: "dispatch-1".into(),
            generation: 1,
            repair: false,
        },
    };
    let next = reduce(&control, &event).unwrap();
    assert_eq!(next.revision, 1);
    assert_eq!(next.intent_generation, 1);
    assert_eq!(next.dispatch_generation, 1);
    assert!(reduce(&next, &event).is_err(), "stale revision accepted");
    let returned = reduce(
        &next,
        &EventEnvelope {
            id: "return-1".into(),
            expected_revision: 1,
            event: Event::Returned {
                id: "dispatch-1".into(),
            },
        },
    )
    .unwrap();
    assert_eq!(returned.tasks["objective"].state, TaskState::Verify);
    assert!(!returned.converged());
}
#[test]
fn unknown_effect_no_progress_and_original_budget_gate_repairs() {
    let control = initial();
    let next = reduce(
        &control,
        &EventEnvelope {
            id: "dispatch-1".into(),
            expected_revision: 0,
            event: Event::Dispatch {
                id: "dispatch-1".into(),
                generation: 1,
                repair: false,
            },
        },
    )
    .unwrap();
    assert!(
        reduce(
            &next,
            &EventEnvelope {
                id: "dispatch-2".into(),
                expected_revision: 1,
                event: Event::Dispatch {
                    id: "dispatch-2".into(),
                    generation: 2,
                    repair: true
                }
            }
        )
        .is_err()
    );
    let mut stopped = reduce(
        &next,
        &EventEnvelope {
            id: "return-1".into(),
            expected_revision: 1,
            event: Event::Returned {
                id: "dispatch-1".into(),
            },
        },
    )
    .unwrap();
    stopped.no_progress_attempts = 2;
    assert!(
        reduce(
            &stopped,
            &EventEnvelope {
                id: "dispatch-2".into(),
                expected_revision: 2,
                event: Event::Dispatch {
                    id: "dispatch-2".into(),
                    generation: 2,
                    repair: true
                }
            }
        )
        .is_err()
    );
    stopped.diagnosis = Some(luna_factoryd::control::Diagnosis {
        summary: "Changed failed predicate, same goal".into(),
        basis: "operator_semantic".into(),
        check_refs: vec![],
        same_goal_replan: false,
    });
    let repair = reduce(
        &stopped,
        &EventEnvelope {
            id: "dispatch-2".into(),
            expected_revision: 2,
            event: Event::Dispatch {
                id: "dispatch-2".into(),
                generation: 2,
                repair: true,
            },
        },
    )
    .unwrap();
    assert_eq!(repair.deadline_at, control.deadline_at);
    assert_eq!(repair.repair_limit, control.repair_limit);
    assert_eq!(repair.attempts[1].parent.as_deref(), Some("dispatch-1"));
}
#[test]
fn source_or_assumption_movement_invalidates_current_proof() {
    let mut control = initial();
    control.criteria[0].accepted = true;
    control.criteria[0].check_refs = vec!["check-1".into()];
    control.criteria[0].subject = Some("subject-1".into());
    let changed = reduce(
        &control,
        &EventEnvelope {
            id: "subject-2".into(),
            expected_revision: 0,
            event: Event::Subject {
                subject: "subject-2".into(),
            },
        },
    )
    .unwrap();
    assert!(!changed.criteria[0].accepted);
    assert!(!changed.converged());
}
#[test]
fn admission_rejects_cycles_missing_dependencies_stale_assumptions_unknown_effects_and_claims() {
    use luna_factoryd::control::admit_task;
    let mut control = initial();
    let task = control.tasks["objective"].clone();
    for mode in [
        "criterion",
        "necessity",
        "dependency",
        "cycle",
        "assumption",
        "effect",
        "claim",
    ] {
        let mut candidate = task.clone();
        candidate.id = "candidate".into();
        match mode {
            "criterion" => candidate.criteria.clear(),
            "necessity" => candidate.necessity = "unverified_native_child".into(),
            "dependency" => candidate.dependencies = vec!["missing".into()],
            "cycle" => candidate.dependencies = vec!["candidate".into()],
            "assumption" => {
                candidate.assumptions.insert("changed".into(), "old".into());
            }
            "effect" => candidate.effects = vec!["unknown".into()],
            "claim" => candidate.claim = "foreign".into(),
            _ => unreachable!(),
        }
        assert!(admit_task(&control, &candidate).is_err(), "admitted {mode}");
    }
    control.observe_child("child", true).unwrap();
    assert_eq!(control.tasks["child:child"].state, TaskState::Verify);
    assert!(admit_task(&control, &control.tasks["child:child"]).is_err());
}
fn store_setup() -> (
    tempfile::TempDir,
    luna_factoryd::config::Config,
    luna_factoryd::store::StartRequest,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
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
                .current_dir(&root)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let config=serde_json::from_value(serde_json::json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/usr/bin/codex","skill_path":dir.path().join("SKILL.md"),"repositories":{"fixture":{"root":root,"max_finish":"local_candidate"}},"profiles":{"default":{"effort":"low"}},"limits":{"capacity":2,"repair_attempts":3,"wall_seconds":600}})).unwrap();
    let request=serde_json::from_value(serde_json::json!({"repository":"fixture","objective":"Explicit bounded objective","acceptance":["Required artifact"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":3,"wall_seconds":600,"idempotency_key":"fixture"})).unwrap();
    (dir, config, request)
}
#[test]
fn sqlite_rejects_stale_writers_and_duplicate_event_payload_conflicts() {
    use luna_factoryd::store::Store;
    let (_dir, config, request) = store_setup();
    let mut store = Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    let mut stale = run.clone();
    let event = EventEnvelope {
        id: "dispatch".into(),
        expected_revision: 0,
        event: Event::Dispatch {
            id: "dispatch".into(),
            generation: 1,
            repair: false,
        },
    };
    assert!(store.apply_event(&mut run, &event).unwrap());
    assert!(!store.apply_event(&mut run, &event).unwrap());
    stale.delta = "Stale overwrite".into();
    assert!(
        store
            .save(&mut stale)
            .unwrap_err()
            .to_string()
            .contains("stale_control_revision")
    );
    let mut conflict = event.clone();
    conflict.event = Event::Subject {
        subject: "foreign".into(),
    };
    assert!(
        store
            .apply_event(&mut run, &conflict)
            .unwrap_err()
            .to_string()
            .contains("event_identity_conflict")
    );
    assert_eq!(
        store.get(&run.id).unwrap().control.unwrap().attempts.len(),
        1
    );
    drop(store);
    assert!(Store::open(&config).is_ok());
}
#[test]
fn legacy_import_preserves_claim_decision_dispatch_identity_deadline_and_budget_without_proof() {
    use luna_factoryd::store::Store;
    let (_dir, config, request) = store_setup();
    let mut store = Store::open(&config).unwrap();
    let run = store.admit(&config, &request).unwrap().run;
    drop(store);
    let mut legacy = serde_json::to_value(&run).unwrap();
    legacy.as_object_mut().unwrap().remove("control");
    legacy["state"] = serde_json::json!("NEEDS_INPUT");
    legacy["dispatch_phase"] = serde_json::json!("turn_start_pending");
    legacy["dispatch_id"] = serde_json::json!("uncertain-dispatch");
    legacy["thread_id"] = serde_json::json!("owner");
    legacy["repairs_used"] = serde_json::json!(2);
    legacy["answered_decisions"] = serde_json::json!({"consumed":"fingerprint"});
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    connection
        .execute(
            "UPDATE runs SET payload=?2 WHERE id=?1",
            rusqlite::params![run.id, legacy.to_string()],
        )
        .unwrap();
    connection
        .execute_batch("DROP TABLE control_events; PRAGMA user_version=0;")
        .unwrap();
    drop(connection);
    let migrated = Store::open(&config).unwrap();
    let result = migrated.get(&run.id).unwrap();
    assert_eq!(result.deadline_at, run.deadline_at);
    assert_eq!(result.repairs_used, 2);
    assert!(result.claim_held);
    assert_eq!(result.answered_decisions["consumed"], "fingerprint");
    assert_eq!(result.dispatch_id.as_deref(), Some("uncertain-dispatch"));
    assert!(!result.control.as_ref().unwrap().converged());
    assert!(result.control.as_ref().unwrap().unknown_effect());
    drop(migrated);
    assert!(Store::open(&config).is_ok());
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    connection
        .execute(
            "UPDATE runs SET payload=json_set(payload,'$.control.intent_generation',2)",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(Store::open(&config).is_err(), "corrupt snapshot accepted");
}
