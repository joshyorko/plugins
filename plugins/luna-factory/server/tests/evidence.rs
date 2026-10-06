use luna_factoryd::{
    control::{Control, Event, EventEnvelope, reduce},
    evidence::{Binding, CheckSpec, reconcile_report},
};
use serde_json::json;
use sha2::{Digest, Sha256};
fn setup() -> (tempfile::TempDir, Control, CheckSpec) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "expected").unwrap();
    let control = Control::new("subject", &["Required behavior".into()], 2, 600).unwrap();
    let control = reduce(
        &control,
        &EventEnvelope {
            id: "dispatch".into(),
            expected_revision: 0,
            event: Event::Dispatch {
                id: "dispatch".into(),
                generation: 1,
                repair: false,
            },
        },
    )
    .unwrap();
    let control = reduce(
        &control,
        &EventEnvelope {
            id: "return".into(),
            expected_revision: 1,
            event: Event::Returned {
                id: "dispatch".into(),
            },
        },
    )
    .unwrap();
    let mut control = control;
    control.settlement = luna_factoryd::control::Settlement::Stopped;
    let spec = CheckSpec {
        id: "file-1".into(),
        kind: "file_sha256".into(),
        path: Some("result.txt".into()),
        sha256: Some(format!("{:x}", Sha256::digest(b"expected"))),
        native_item: None,
        binding: Binding {
            task_id: "objective".into(),
            attempt_id: "dispatch".into(),
            intent_generation: 1,
            dispatch_generation: 1,
            subject: "subject".into(),
            assumptions: Default::default(),
        },
    };
    (dir, control, spec)
}
fn report(spec: &CheckSpec) -> serde_json::Value {
    json!({"checks":[spec],"acceptance":[{"id":"A1","passed":true,"accepted":true,"evidence":"Owner semantic judgment","check_refs":["file-1"]}]})
}
#[test]
fn prose_is_not_proof_and_structured_file_fact_requires_semantic_acceptance() {
    let (dir, mut control, spec) = setup();
    reconcile_report(
        &mut control,
        &json!({"acceptance":[{"id":"A1","passed":true,"evidence":"Looks done"}]}),
        dir.path(),
    )
    .unwrap();
    assert!(!control.converged());
    let mut missing_acceptance = report(&spec);
    missing_acceptance["acceptance"][0]["accepted"] = json!(false);
    reconcile_report(&mut control, &missing_acceptance, dir.path()).unwrap();
    assert!(!control.converged());
    reconcile_report(&mut control, &report(&spec), dir.path()).unwrap();
    assert!(control.converged());
    std::fs::write(dir.path().join("result.txt"), "changed").unwrap();
    reconcile_report(&mut control, &report(&spec), dir.path()).unwrap();
    assert!(!control.converged(), "contradictory check was accepted");
}
#[test]
fn foreign_bindings_unknown_environment_and_path_escape_never_certify() {
    for mutation in [
        "task",
        "attempt",
        "intent",
        "dispatch",
        "subject",
        "assumption",
        "native",
        "escape",
    ] {
        let (dir, mut control, mut spec) = setup();
        match mutation {
            "task" => spec.binding.task_id = "foreign".into(),
            "attempt" => spec.binding.attempt_id = "foreign".into(),
            "intent" => spec.binding.intent_generation = 2,
            "dispatch" => spec.binding.dispatch_generation = 2,
            "subject" => spec.binding.subject = "foreign".into(),
            "assumption" => {
                spec.binding
                    .assumptions
                    .insert("unobserved".into(), "claim".into());
            }
            "native" => {
                spec.kind = "native_command".into();
                spec.path = None;
                spec.sha256 = None;
                spec.native_item = Some("owner:item".into());
            }
            "escape" => spec.path = Some("../outside".into()),
            _ => unreachable!(),
        }
        let result = reconcile_report(&mut control, &report(&spec), dir.path());
        assert!(
            result.is_err() || !control.converged(),
            "accepted {mutation}"
        );
    }
}
#[cfg(unix)]
#[test]
fn symlink_and_nonregular_files_are_rejected() {
    let (dir, mut control, mut spec) = setup();
    std::os::unix::fs::symlink("result.txt", dir.path().join("link")).unwrap();
    spec.path = Some("link".into());
    assert!(reconcile_report(&mut control, &report(&spec), dir.path()).is_err());
    spec.path = Some(".".into());
    assert!(reconcile_report(&mut control, &report(&spec), dir.path()).is_err());
}
#[test]
fn new_dispatch_requires_explicit_reattestation_and_denies_old_active_or_unknown_attempts() {
    use luna_factoryd::control::{Event, EventEnvelope, TaskState};
    let (dir, mut control, spec) = setup();
    let mut task = control.tasks["objective"].clone();
    task.id = "second-task".into();
    task.necessity = "owner_declared_necessary".into();
    task.state = TaskState::Ready;
    control.tasks.insert(task.id.clone(), task);
    reconcile_report(&mut control, &report(&spec), dir.path()).unwrap();
    control.selected_task = "second-task".into();
    let revision = control.revision;
    control = reduce(
        &control,
        &EventEnvelope {
            id: "second-dispatch".into(),
            expected_revision: revision,
            event: Event::Dispatch {
                id: "second-dispatch".into(),
                generation: 2,
                repair: true,
            },
        },
    )
    .unwrap();
    control = reduce(
        &control,
        &EventEnvelope {
            id: "second-return".into(),
            expected_revision: control.revision,
            event: Event::Returned {
                id: "second-dispatch".into(),
            },
        },
    )
    .unwrap();
    control.settlement = luna_factoryd::control::Settlement::Stopped;
    let mut second = spec.clone();
    second.id = "second-check".into();
    second.binding.task_id = "second-task".into();
    second.binding.attempt_id = "second-dispatch".into();
    second.binding.dispatch_generation = 2;
    let packet = serde_json::json!({"checks":[second],"acceptance":[{"id":"A1","passed":true,"accepted":true,"evidence":"Only B explicitly checked","check_refs":["file-1","second-check"]}]});
    reconcile_report(&mut control, &packet, dir.path()).unwrap();
    assert!(
        !control.converged(),
        "old A fact was credited without re-attestation"
    );
    let mut current = packet.clone();
    current["checks"] = serde_json::json!([spec, second]);
    reconcile_report(&mut control, &current, dir.path()).unwrap();
    assert!(control.converged());
    for phase in ["active", "intent_unknown"] {
        let mut unknown = control.clone();
        unknown.attempts[0].phase = phase.into();
        assert!(
            !luna_factoryd::evidence::binding_current(&unknown, &spec.binding),
            "accepted {phase}"
        );
    }
    let mut newer = control.clone();
    let mut replacement = newer.attempts[0].clone();
    replacement.id = "newer-same-task".into();
    replacement.dispatch_generation = 3;
    newer.dispatch_generation = 3;
    newer.attempts.push(replacement);
    assert!(
        !luna_factoryd::evidence::binding_current(&newer, &spec.binding),
        "accepted superseded task attempt"
    );
    let mut changed = control.clone();
    changed
        .assumptions
        .insert("required".into(), "changed".into());
    assert!(!luna_factoryd::evidence::binding_current(
        &changed,
        &spec.binding
    ));
    let mut moved = control.clone();
    moved.current_subject = "moved".into();
    assert!(!luna_factoryd::evidence::binding_current(
        &moved,
        &spec.binding
    ));
}
fn dependency_setup() -> (tempfile::TempDir, Control, CheckSpec, serde_json::Value) {
    use luna_factoryd::control::{Criterion, TaskState};
    let (dir, mut control, spec) = setup();
    control.criteria.push(Criterion {
        id: "A2".into(),
        accepted: false,
        check_refs: vec![],
        subject: None,
        reason: "evidence_missing".into(),
    });
    for (id, criterion, dependencies) in [
        ("prerequisite", "A1", vec![]),
        ("dependent", "A2", vec!["prerequisite".into()]),
    ] {
        let mut task = control.tasks["objective"].clone();
        task.id = id.into();
        task.criteria = vec![criterion.into()];
        task.dependencies = dependencies;
        task.necessity = "owner_declared_necessary".into();
        task.state = TaskState::Verify;
        control.tasks.insert(task.id.clone(), task);
        let mut attempt = control.attempts[0].clone();
        attempt.id = format!("{id}-attempt");
        attempt.task_id = id.into();
        control.attempts.push(attempt);
    }
    let mut prerequisite = spec.clone();
    prerequisite.id = "prerequisite-check".into();
    prerequisite.binding.task_id = "prerequisite".into();
    prerequisite.binding.attempt_id = "prerequisite-attempt".into();
    let mut dependent = spec;
    dependent.id = "dependent-check".into();
    dependent.binding.task_id = "dependent".into();
    dependent.binding.attempt_id = "dependent-attempt".into();
    // Dependent observation deliberately precedes prerequisite observation.
    let packet = json!({
        "checks": [dependent, prerequisite],
        "acceptance": [
            {"id":"A1","passed":true,"accepted":true,"check_refs":["prerequisite-check"]},
            {"id":"A2","passed":true,"accepted":true,"check_refs":["dependent-check"]}
        ]
    });
    (dir, control, dependent, packet)
}
#[test]
fn dependent_criterion_requires_its_prerequisites_own_scoped_proof() {
    use luna_factoryd::evidence::{binding_current, criterion_current, task_proof_current};
    let (dir, mut control, dependent, packet) = dependency_setup();
    reconcile_report(&mut control, &packet, dir.path()).unwrap();
    assert!(criterion_current(&control, &control.criteria[1]));
    for mutation in [
        "failed",
        "unverified",
        "contradictory",
        "missing",
        "stale_attempt",
        "acceptance",
        "criterion_subject",
        "task_assumptions",
        "own_ref",
        "foreign_claim",
        "subject",
        "missing_dependency",
        "cycle",
        "transitive",
    ] {
        let mut invalid = control.clone();
        match mutation {
            "failed" | "unverified" | "contradictory" => {
                invalid
                    .checks
                    .iter_mut()
                    .find(|check| check.id == "prerequisite-check")
                    .unwrap()
                    .outcome = mutation.into();
            }
            "missing" => invalid
                .checks
                .retain(|check| check.id != "prerequisite-check"),
            "stale_attempt" => {
                invalid
                    .attempts
                    .iter_mut()
                    .find(|attempt| attempt.task_id == "prerequisite")
                    .unwrap()
                    .phase = "active".into()
            }
            "acceptance" => invalid.criteria[0].accepted = false,
            "criterion_subject" => invalid.criteria[0].subject = Some("stale".into()),
            "task_assumptions" => {
                invalid
                    .tasks
                    .get_mut("prerequisite")
                    .unwrap()
                    .assumptions
                    .insert("changed".into(), "value".into());
            }
            "own_ref" => invalid.criteria[0].check_refs = vec!["file-1".into()],
            "foreign_claim" => {
                invalid.tasks.get_mut("prerequisite").unwrap().claim = "foreign".into()
            }
            "subject" => invalid.tasks.get_mut("prerequisite").unwrap().subject = "stale".into(),
            "missing_dependency" => {
                invalid.tasks.get_mut("dependent").unwrap().dependencies = vec!["missing".into()]
            }
            "cycle" => {
                invalid.tasks.get_mut("prerequisite").unwrap().dependencies =
                    vec!["dependent".into()]
            }
            "transitive" => {
                invalid.tasks.get_mut("prerequisite").unwrap().dependencies =
                    vec!["objective".into()]
            }
            _ => unreachable!(),
        }
        assert!(
            !binding_current(&invalid, &dependent.binding),
            "binding retained {mutation} prerequisite"
        );
        assert!(
            !criterion_current(&invalid, &invalid.criteria[1]),
            "criterion retained {mutation} prerequisite"
        );
        assert!(
            !task_proof_current(&invalid, &invalid.tasks["dependent"]),
            "task retained {mutation} prerequisite"
        );
    }
}
#[test]
fn shared_criterion_dependency_uses_only_the_prerequisites_own_refs() {
    let (dir, mut control, dependent, mut packet) = dependency_setup();
    control.criteria.pop();
    control.tasks.get_mut("dependent").unwrap().criteria = vec!["A1".into()];
    packet["acceptance"] = json!([
        {"id":"A1","passed":true,"accepted":true,"check_refs":["prerequisite-check","dependent-check"]}
    ]);
    reconcile_report(&mut control, &packet, dir.path()).unwrap();
    assert!(luna_factoryd::evidence::binding_current(
        &control,
        &dependent.binding
    ));
    assert!(luna_factoryd::evidence::criterion_current(
        &control,
        &control.criteria[0]
    ));
    assert_eq!(
        control.tasks["prerequisite"].state,
        luna_factoryd::control::TaskState::Done
    );
    assert_eq!(
        control.tasks["dependent"].state,
        luna_factoryd::control::TaskState::Done
    );
}
#[test]
fn dependency_failure_demotes_completed_dependent_on_reconciliation() {
    let (dir, mut control, _, mut packet) = dependency_setup();
    reconcile_report(&mut control, &packet, dir.path()).unwrap();
    assert_eq!(
        control.tasks["dependent"].state,
        luna_factoryd::control::TaskState::Done
    );
    control
        .checks
        .iter_mut()
        .find(|check| check.id == "prerequisite-check")
        .unwrap()
        .outcome = "failed".into();
    packet["checks"] = json!([]);
    reconcile_report(&mut control, &packet, dir.path()).unwrap();
    assert_eq!(
        control.tasks["dependent"].state,
        luna_factoryd::control::TaskState::Verify
    );
    assert!(!luna_factoryd::evidence::criterion_current(
        &control,
        &control.criteria[1]
    ));
}
