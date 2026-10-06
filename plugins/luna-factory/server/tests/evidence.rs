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
