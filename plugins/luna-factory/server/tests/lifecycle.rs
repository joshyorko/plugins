use luna_factoryd::lifecycle::{accept_owner_report, public_run, terminal_threads};
use serde_json::json;

#[test]
fn interrupt_ack_and_missing_children_do_not_prove_cancellation() {
    assert!(!terminal_threads(
        &["owner".into(), "child".into()],
        &[json!({"id":"owner","status":{"type":"idle"}})]
    ));
    assert!(!terminal_threads(
        &["owner".into()],
        &[json!({"id":"owner","status":{"type":"notLoaded"}})]
    ));
    assert!(!terminal_threads(
        &["owner".into()],
        &[json!({"id":"owner","status":{"type":"active"}})]
    ));
    assert!(terminal_threads(
        &["owner".into(), "child".into()],
        &[
            json!({"id":"owner","status":{"type":"idle"}}),
            json!({"id":"child","status":{"type":"idle"}})
        ]
    ));
}

#[test]
fn convergence_requires_current_subject_and_every_mandatory_criterion() {
    let valid = json!({"state":"CONVERGED","subject":"abc:123","acceptance":[{"id":"A1","passed":true,"evidence":"test exit 0"}],"delta":"Done","remaining_gap":"","blocker":null});
    assert!(accept_owner_report(&valid, "abc:123", 1).is_ok());
    assert!(accept_owner_report(&valid, "abc:new", 1).is_err());
    assert!(accept_owner_report(&valid, "abc:123", 2).is_err());
    let mut failed = valid.clone();
    failed["acceptance"][0]["passed"] = json!(false);
    assert!(accept_owner_report(&failed, "abc:123", 1).is_err());
    failed["state"] = json!("QUIESCENT");
    assert!(accept_owner_report(&failed, "abc:123", 1).is_ok());
}

#[test]
fn external_result_does_not_include_host_paths_or_raw_logs() {
    let input = json!({"id":"run","request":{"repository":"plugins","objective":"work","acceptance":["passes"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"key"},"canonical_root":"/private/repo","repository_identity":"/private/repo/.git","state":"RUNNING","base_head":"abc","current_subject":"abc:123","thread_id":"owner","turn_id":"turn","owned_threads":[],"generation":1,"repairs_used":0,"created_at":0,"updated_at":0,"deadline_at":60,"delta":"Started","blocker":null,"observed_model":null,"observed_effort":null,"configured_model":"gpt-6-luna","configured_effort":"low","claim_held":true});
    let run = serde_json::from_value(input).unwrap();
    let value = public_run(&run);
    assert!(value.get("canonical_root").is_none());
    assert!(value.get("repository_identity").is_none());
    assert_eq!(value["route"]["observed_model"], serde_json::Value::Null);
    assert_eq!(value["route"]["requested_model"], "gpt-6-luna");
}
#[test]
fn bounded_owner_summaries_withhold_synthetic_credentials() {
    for secret in [
        "sk-synthetic-secret",
        "Authorization: Bearer synthetic",
        "password=synthetic",
        "ghp_synthetic",
    ] {
        let summary = luna_factoryd::lifecycle::safe_summary(secret, 100);
        assert!(!summary.contains(secret));
        assert!(summary.contains("withheld"));
    }
    assert_eq!(
        luna_factoryd::lifecycle::safe_summary("A1 passed, one decision remains", 100),
        "A1 passed, one decision remains"
    );
}

#[test]
fn all_owner_report_states_validate_bounded_unique_criterion_ids() {
    for state in ["NEEDS_INPUT", "BLOCKED", "QUIESCENT", "CONVERGED"] {
        let valid = json!({"state":state,"subject":"abc:123","acceptance":[{"id":"A1","passed":false,"evidence":"Unproved"}],"delta":"One gap remains","remaining_gap":"Select the bounded option","blocker":"Which option?"});
        for id in ["ghp_SYNTHETIC_TEST_SECRET_ONLY", "A0", "A2", "A01", ""] {
            let mut invalid = valid.clone();
            invalid["acceptance"][0]["id"] = json!(id);
            assert!(
                accept_owner_report(&invalid, "abc:123", 1).is_err(),
                "accepted {id:?} in {state}"
            );
        }
        let mut duplicate = valid.clone();
        duplicate["acceptance"] = json!([valid["acceptance"][0], valid["acceptance"][0]]);
        assert!(
            accept_owner_report(&duplicate, "abc:123", 2).is_err(),
            "duplicate criterion accepted in {state}"
        );
    }
}
