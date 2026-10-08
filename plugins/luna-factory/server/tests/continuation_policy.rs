use luna_factoryd::{
    continuation::{Directive, Kind, RepairDiagnosis, decide},
    control::{Control, Event, EventEnvelope, RunControl, Settlement, TaskState, reduce},
};
use serde_json::json;
fn control() -> Control {
    let c = Control::new("subject", &["A1".into()], 1, 1000).unwrap();
    let c = reduce(
        &c,
        &EventEnvelope {
            id: "dispatch".into(),
            expected_revision: 0,
            event: Event::Dispatch {
                id: "attempt".into(),
                generation: 1,
                repair: false,
            },
        },
    )
    .unwrap();
    let mut c = reduce(
        &c,
        &EventEnvelope {
            id: "return".into(),
            expected_revision: 1,
            event: Event::Returned {
                id: "attempt".into(),
            },
        },
    )
    .unwrap();
    c.settlement = Settlement::Stopped;
    c.owner_liveness = Settlement::Stopped;
    c.run_control = RunControl::Quiescent;
    c
}
#[test]
fn next_is_explicit_necessary_unattempted_ready_and_does_not_consume_repairs() {
    let mut c = control();
    let mut task = c.tasks["objective"].clone();
    task.id = "next".into();
    task.necessity = "owner_declared_necessary".into();
    task.state = TaskState::Ready;
    c.tasks.insert("next".into(), task);
    c.selected_task = "next".into();
    c.repairs_used = 1;
    c.no_progress_attempts = 9;
    let directive = Directive {
        kind: Kind::Next,
        task_id: "next".into(),
        diagnosis: None,
    };
    assert!(!decide(&c, &directive, 900).unwrap().repair);
    assert!(decide(&c, &directive, 1000).is_err());
    c.tasks.get_mut("next").unwrap().necessity = "necessity_unverified".into();
    assert!(decide(&c, &directive, 900).is_err());
}
#[test]
fn repair_needs_current_retained_failed_observation_and_original_budget() {
    let mut c = control();
    let directive = Directive {
        kind: Kind::Repair,
        task_id: "objective".into(),
        diagnosis: Some(RepairDiagnosis {
            summary: "Repair observed predicate".into(),
            check_refs: vec!["failure".into()],
        }),
    };
    assert!(decide(&c, &directive, 900).is_err());
    c.checks.push(serde_json::from_value(json!({"id":"failure","kind":"native_command","outcome":"failed","reason":"native_exit_nonzero","path":null,"expected_sha256":null,"observed_sha256":null,"exit_code":1,"output_completeness":"unverified","environment":"unverified","binding":{"task_id":"objective","attempt_id":"attempt","intent_generation":1,"dispatch_generation":1,"subject":"subject","assumptions":{}}})).unwrap());
    let plan = decide(&c, &directive, 900).unwrap();
    assert!(plan.repair);
    assert_eq!(plan.diagnosis.as_ref().unwrap().basis, "observed_failure");
    let diagnosed = reduce(
        &c,
        &EventEnvelope {
            id: "diagnose".into(),
            expected_revision: c.revision,
            event: Event::Diagnose {
                diagnosis: plan.diagnosis.unwrap(),
            },
        },
    )
    .unwrap();
    let moved = reduce(
        &diagnosed,
        &EventEnvelope {
            id: "moved".into(),
            expected_revision: diagnosed.revision,
            event: Event::Subject {
                subject: "new-subject".into(),
            },
        },
    )
    .unwrap();
    assert!(
        moved.diagnosis.is_none(),
        "source movement must invalidate pending failure diagnosis"
    );
    for mutation in ["budget", "foreign", "contradictory", "live"] {
        let mut bad = c.clone();
        match mutation {
            "budget" => bad.repairs_used = 1,
            "foreign" => bad.checks[0].binding.attempt_id = "foreign".into(),
            "contradictory" => bad.checks[0].outcome = "contradictory".into(),
            "live" => bad.settlement = Settlement::Live,
            _ => unreachable!(),
        }
        assert!(
            decide(&bad, &directive, 900).is_err(),
            "accepted {mutation}"
        );
    }
}
#[test]
fn directive_rejects_model_authorization_and_untyped_repair_diagnosis() {
    assert!(serde_json::from_value::<Directive>(json!({"kind":"next","task_id":"next"})).is_err());
    assert!(serde_json::from_value::<Directive>(json!({"kind":"repair","task_id":"objective","diagnosis":{"summary":"claim","check_refs":[],"basis":"operator_semantic"}})).is_err());
}

#[test]
fn strict_owner_schema_closes_every_nullable_nested_object() {
    fn check(value: &serde_json::Value) {
        if let Some(properties) = value
            .get("properties")
            .and_then(serde_json::Value::as_object)
        {
            assert_eq!(value["additionalProperties"], false);
            let required = value["required"].as_array().unwrap();
            assert_eq!(required.len(), properties.len());
            for (name, field) in properties {
                assert!(required.contains(&json!(name)));
                check(field);
            }
        }
        if let Some(items) = value.get("items") {
            check(items);
        }
        for kind in ["anyOf", "oneOf", "allOf"] {
            if let Some(variants) = value.get(kind).and_then(serde_json::Value::as_array) {
                for variant in variants {
                    check(variant);
                }
            }
        }
    }
    let schema = luna_factoryd::lifecycle::owner_output_schema();
    check(&schema);
    assert_eq!(
        schema["properties"]["continuation"]["type"],
        json!(["object", "null"])
    );
    assert_eq!(
        schema["properties"]["continuation"]["properties"]["diagnosis"]["type"],
        json!(["object", "null"])
    );
}
