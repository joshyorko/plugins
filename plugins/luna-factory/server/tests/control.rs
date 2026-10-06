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
    stopped.settlement = luna_factoryd::control::Settlement::Stopped;
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
#[test]
fn read_detects_changed_snapshot_and_foreign_claim_projection() {
    use luna_factoryd::store::Store;
    let (_dir, config, request) = store_setup();
    let mut store = Store::open(&config).unwrap();
    let mut first = store.admit(&config, &request).unwrap().run;
    first.set_state(luna_factoryd::control::RunControl::Cancelled);
    first.thread_id = Some("first-owner".into());
    first.dispatch_phase = "terminal_observed".into();
    first.control.as_mut().unwrap().settlement = luna_factoryd::control::Settlement::Stopped;
    store.release_verified(&mut first).unwrap();
    let mut second_request = request.clone();
    second_request.idempotency_key = "other-owner".into();
    store.admit(&config, &second_request).unwrap();
    let observed = store.get(&first.id).unwrap();
    let view =
        luna_factoryd::presentation::project(&observed, observed.control.as_ref().unwrap(), 0);
    assert_eq!(view["claim"]["status"], "foreign");
    assert_eq!(
        view["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action["kind"] == "resume")
            .unwrap()["allowed"],
        false
    );
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    connection.execute("UPDATE runs SET payload=json_set(payload,'$.control.current_subject','valid-but-corrupt') WHERE id=?1",[&first.id]).unwrap();
    assert!(
        store.get(&first.id).is_err(),
        "changed snapshot read bypassed journal validation"
    );
}

#[test]
fn matching_donor_conformance_cases_use_the_production_core() {
    use luna_factoryd::{
        control::admit_task,
        evidence::{Binding, ObservedCheck, criterion_current},
        store::Store,
    };
    use serde_json::{Value, json};
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/control-conformance.json"
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        assert_eq!(case["donor"]["sha"].as_str().unwrap().len(), 40);
        let input = &case["input"];
        let id = case["id"].as_str().unwrap();
        let output = match id {
            "stale_revision" => {
                let mut control = initial();
                control.revision = input["current_revision"].as_u64().unwrap();
                let accepted = reduce(
                    &control,
                    &EventEnvelope {
                        id: "fixture-event".into(),
                        expected_revision: input["expected_revision"].as_u64().unwrap(),
                        event: Event::Subject {
                            subject: "next".into(),
                        },
                    },
                )
                .is_ok();
                json!({"accepted":accepted,"effects_executed":0})
            }
            "duplicate_event_conflict" => {
                let (_dir, config, request) = store_setup();
                let mut store = Store::open(&config).unwrap();
                let mut run = store.admit(&config, &request).unwrap().run;
                let first = EventEnvelope {
                    id: input["event_id"].as_str().unwrap().into(),
                    expected_revision: 0,
                    event: Event::Dispatch {
                        id: input["first_payload"].as_str().unwrap().into(),
                        generation: 1,
                        repair: false,
                    },
                };
                store.apply_event(&mut run, &first).unwrap();
                let before = run.control.clone().unwrap();
                let mut conflict = first;
                conflict.event = Event::Subject {
                    subject: input["second_payload"].as_str().unwrap().into(),
                };
                let accepted = store.apply_event(&mut run, &conflict).is_ok();
                json!({"accepted":accepted,"state_overwritten":store.get(&run.id).unwrap().control.unwrap()!=before})
            }
            "unknown_effect_repair" | "dispatch_generation_same_goal" => {
                let control = initial();
                let mut next = reduce(
                    &control,
                    &EventEnvelope {
                        id: "first".into(),
                        expected_revision: 0,
                        event: Event::Dispatch {
                            id: "first".into(),
                            generation: 1,
                            repair: false,
                        },
                    },
                )
                .unwrap();
                if id == "dispatch_generation_same_goal" {
                    next = reduce(
                        &next,
                        &EventEnvelope {
                            id: "return".into(),
                            expected_revision: 1,
                            event: Event::Returned { id: "first".into() },
                        },
                    )
                    .unwrap();
                    next.settlement = luna_factoryd::control::Settlement::Stopped;
                }
                let repair = reduce(
                    &next,
                    &EventEnvelope {
                        id: "repair".into(),
                        expected_revision: next.revision,
                        event: Event::Dispatch {
                            id: "repair".into(),
                            generation: 2,
                            repair: true,
                        },
                    },
                );
                if id == "unknown_effect_repair" {
                    let (_dir, config, request) = store_setup();
                    let mut store = Store::open(&config).unwrap();
                    let mut run = store.admit(&config, &request).unwrap().run;
                    run.thread_id = Some("observed-fixture-owner".into());
                    run.set_state(luna_factoryd::control::RunControl::Blocked);
                    assert_eq!(input["effect"], "pr_create");
                    assert_eq!(input["settlement"], "unknown");
                    run.control
                        .as_mut()
                        .unwrap()
                        .observe_effect("unsettled-pr", luna_factoryd::control::EffectKind::Pr)
                        .unwrap();
                    let projected = luna_factoryd::presentation::project(
                        &run,
                        run.control.as_ref().unwrap(),
                        0,
                    );
                    let actions = projected["actions"].as_array().unwrap();
                    json!({"repair_allowed":actions.iter().find(|action|action["kind"]=="resume").unwrap()["allowed"],"reconciliation_available":actions.iter().find(|action|action["kind"]=="reconcile").unwrap()["allowed"]})
                } else {
                    let repaired = repair.unwrap();
                    json!({"intent_generation":repaired.intent_generation,"dispatch_generation":repaired.dispatch_generation,"budget_reset":repaired.deadline_at!=control.deadline_at||repaired.repair_limit!=control.repair_limit||repaired.repairs_used<control.repairs_used})
                }
            }
            "admission_missing_dependency"
            | "admission_cycle"
            | "admission_stale_assumption"
            | "admission_foreign_claim" => {
                let mut control = initial();
                let mut task = control.tasks["objective"].clone();
                task.id = input["candidate"].as_str().unwrap_or("candidate").into();
                if let Some(deps) = input.get("dependencies") {
                    task.dependencies = serde_json::from_value(deps.clone()).unwrap();
                }
                if id == "admission_cycle" {
                    for known in input["known_tasks"].as_array().unwrap() {
                        let mut dependency = task.clone();
                        dependency.id = known["id"].as_str().unwrap().into();
                        dependency.dependencies =
                            serde_json::from_value(known["dependencies"].clone()).unwrap();
                        dependency.state = TaskState::Candidate;
                        control.tasks.insert(dependency.id.clone(), dependency);
                    }
                }
                if id == "admission_stale_assumption" {
                    assert_eq!(input["state"], "UNKNOWN");
                    assert_eq!(input["movement"], "CANDIDATE");
                    assert_eq!(input["reconcile"], "stale");
                    assert_eq!(input["fresh"], false);
                    control
                        .assumptions
                        .insert("acceptance".into(), "current".into());
                    task.assumptions.insert("acceptance".into(), "stale".into());
                }
                if id == "admission_foreign_claim" {
                    task.id = input["candidate"]["id"].as_str().unwrap().into();
                    let mut writer = task.clone();
                    writer.id = input["active_writer"]["id"].as_str().unwrap().into();
                    writer.state = TaskState::Running;
                    control.tasks.insert(writer.id.clone(), writer);
                    task.claim = "foreign".into();
                }
                let ready = admit_task(&control, &task).is_ok();
                json!({"ready":ready,"dispatch_allowed":ready})
            }
            "evidence_wrong_binding" | "evidence_prose_only" => {
                let control = initial();
                let mut next = reduce(
                    &control,
                    &EventEnvelope {
                        id: "first".into(),
                        expected_revision: 0,
                        event: Event::Dispatch {
                            id: "first".into(),
                            generation: 1,
                            repair: false,
                        },
                    },
                )
                .unwrap();
                next = reduce(
                    &next,
                    &EventEnvelope {
                        id: "return".into(),
                        expected_revision: 1,
                        event: Event::Returned { id: "first".into() },
                    },
                )
                .unwrap();
                next.settlement = luna_factoryd::control::Settlement::Stopped;
                next.criteria[0].accepted = true;
                next.criteria[0].subject = Some(next.current_subject.clone());
                if id == "evidence_prose_only" {
                    json!({"criterion_proven":criterion_current(&next,&next.criteria[0]),"converged":next.converged()})
                } else {
                    let binding = Binding {
                        task_id: "objective".into(),
                        attempt_id: "first".into(),
                        intent_generation: 1,
                        dispatch_generation: 1,
                        subject: next.current_subject.clone(),
                        assumptions: Default::default(),
                    };
                    next.criteria[0].check_refs = vec!["observed-fixture".into()];
                    // A pure fixture fact. The separate Factory/file tests prove the adapter.
                    next.checks.push(ObservedCheck {
                        id: "observed-fixture".into(),
                        binding,
                        kind: "file_sha256".into(),
                        outcome: "passed".into(),
                        reason: "predicate_observed".into(),
                        path: Some("fixture.txt".into()),
                        expected_sha256: Some("0".repeat(64)),
                        observed_sha256: Some("0".repeat(64)),
                        exit_code: None,
                        output_completeness: "not_applicable".into(),
                        environment: "not_applicable".into(),
                    });
                    assert!(criterion_current(&next, &next.criteria[0]));
                    let mut any_proven = false;
                    for mismatch in input["mismatches"].as_array().unwrap() {
                        let mut foreign = next.clone();
                        let binding = &mut foreign.checks[0].binding;
                        match mismatch.as_str().unwrap() {
                            "task" => binding.task_id = "foreign".into(),
                            "attempt" => binding.attempt_id = "foreign".into(),
                            "intent" => binding.intent_generation += 1,
                            "generation" | "dispatch" => binding.dispatch_generation += 1,
                            "subject" => binding.subject = "foreign".into(),
                            "assumption" => {
                                binding.assumptions.insert("foreign".into(), "claim".into());
                            }
                            other => panic!("unknown mismatch {other}"),
                        }
                        any_proven |= criterion_current(&foreign, &foreign.criteria[0]);
                    }
                    json!({"criterion_proven":any_proven})
                }
            }
            other => panic!("unimplemented conformance case {other}"),
        };
        assert_eq!(output, case["expected"], "case {id}");
    }
}

fn load_actual_v1(config: &luna_factoryd::config::Config, corrupt: bool) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/schema-v1-822cdf81.json")).unwrap();
    assert_eq!(
        fixture["producer_commit"],
        "822cdf81ca3d52e5dd17ee74340d1621707db7a8"
    );
    drop(luna_factoryd::store::Store::open(config).unwrap());
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let original = fixture["payload"].as_str().unwrap();
    let payload = if corrupt {
        format!("{original} ")
    } else {
        original.to_owned()
    };
    let events = fixture["events"].as_array().unwrap();
    assert_eq!(
        events.last().unwrap()["snapshot_sha256"],
        format!("{:x}", Sha256::digest(original.as_bytes()))
    );
    connection
        .execute(
            "INSERT INTO runs(id,idem,fingerprint,root,state,payload) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                fixture["id"].as_str().unwrap(),
                fixture["idem"].as_str().unwrap(),
                fixture["fingerprint"].as_str().unwrap(),
                fixture["root"].as_str().unwrap(),
                fixture["state"].as_str().unwrap(),
                payload
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO claims(identity,run_id) VALUES (?1,?2)",
            rusqlite::params![
                fixture["claim_identity"].as_str().unwrap(),
                fixture["id"].as_str().unwrap()
            ],
        )
        .unwrap();
    for event in events {
        connection.execute("INSERT INTO control_events(run_id,event_id,revision,fingerprint,envelope,snapshot_sha256) VALUES (?1,?2,?3,?4,?5,?6)",rusqlite::params![fixture["id"].as_str().unwrap(),event["event_id"].as_str().unwrap(),event["revision"].as_i64().unwrap(),event["fingerprint"].as_str().unwrap(),event["envelope"].as_str().unwrap(),event["snapshot_sha256"].as_str().unwrap()]).unwrap();
    }
    connection.execute_batch("PRAGMA user_version=1;").unwrap();
    fixture
}
#[test]
fn actual_822_v1_snapshot_imports_transactionally_without_invented_cessation() {
    let (_dir, config, _request) = store_setup();
    let fixture = load_actual_v1(&config, false);
    let original: serde_json::Value =
        serde_json::from_str(fixture["payload"].as_str().unwrap()).unwrap();
    let store =
        luna_factoryd::store::Store::open(&config).expect("actual v1 snapshot must migrate");
    let run = store.get(fixture["id"].as_str().unwrap()).unwrap();
    let imported = serde_json::to_value(&run).unwrap();
    for key in [
        "id",
        "request",
        "canonical_root",
        "repository_identity",
        "base_head",
        "current_subject",
        "thread_id",
        "dispatch_phase",
        "dispatch_id",
        "turn_id",
        "owned_threads",
        "generation",
        "repairs_used",
        "deadline_at",
        "pending_decision",
        "answered_decisions",
        "skill_sha256",
        "configured_provider",
        "claim_held",
        "terminal_stop_attempts",
    ] {
        assert_eq!(imported[key], original[key], "changed {key}");
    }
    assert_eq!(run.control.as_ref().unwrap().schema_version, 2);
    assert_eq!(
        run.control.as_ref().unwrap().settlement,
        luna_factoryd::control::Settlement::Unknown
    );
    assert!(run.control.as_ref().unwrap().unknown_effect());
    assert_eq!(
        run.control.as_ref().unwrap().revision,
        original["control"]["revision"].as_u64().unwrap() + 1
    );
    drop(store);
    assert!(
        luna_factoryd::store::Store::open(&config).is_ok(),
        "subsequent strict v2 open failed"
    );
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let old_digest: String = connection
        .query_row(
            "SELECT snapshot_sha256 FROM control_events WHERE run_id=?1 AND revision=?2",
            rusqlite::params![
                fixture["id"].as_str().unwrap(),
                original["control"]["revision"].as_i64().unwrap()
            ],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        old_digest,
        fixture["events"].as_array().unwrap().last().unwrap()["snapshot_sha256"]
    );
}
#[test]
fn corrupt_actual_v1_snapshot_fails_before_import_and_keeps_schema_and_claim() {
    let (_dir, config, _request) = store_setup();
    let fixture = load_actual_v1(&config, true);
    assert!(
        luna_factoryd::store::Store::open(&config)
            .err()
            .unwrap()
            .to_string()
            .contains("control_snapshot_corrupt")
    );
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let owner: String = connection
        .query_row(
            "SELECT run_id FROM claims WHERE identity=?1",
            [fixture["claim_identity"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, fixture["id"]);
}
#[test]
fn public_projection_bounds_do_not_narrow_authoritative_history() {
    let (_dir, config, mut request) = store_setup();
    request.objective = "x".repeat(8000);
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let mut run = store.admit(&config, &request).unwrap().run;
    let control = run.control.as_mut().unwrap();
    for index in 0..1000 {
        control.attempts.push(luna_factoryd::control::Attempt {
            id: format!("history-{index}"),
            task_id: "objective".into(),
            parent: None,
            intent_generation: 1,
            dispatch_generation: 1,
            source_subject: control.current_subject.clone(),
            assumptions: Default::default(),
            phase: "returned".into(),
            turn_id: None,
            certified_before: 0,
            diagnosis: None,
        });
    }
    run.owned_threads = (0..127).map(|index| format!("child-{index}")).collect();
    let public = luna_factoryd::presentation::public_control(&run, run.control.as_ref().unwrap());
    let projection = luna_factoryd::presentation::project(&run, run.control.as_ref().unwrap(), 0);
    assert!(
        public["tasks"][0]["title"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count()
            <= 1000
    );
    assert_eq!(public["attempts"].as_array().unwrap().len(), 256);
    assert_eq!(
        public["tasks"][0]["attempt_ids"].as_array().unwrap().len(),
        64
    );
    assert_eq!(projection["workers"].as_array().unwrap().len(), 64);
    assert_eq!(run.control.as_ref().unwrap().attempts.len(), 1000);
    assert_eq!(run.request.objective.len(), 8000);
}
