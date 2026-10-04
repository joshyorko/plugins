//! Protocol contract and fail-closed routing tests; all data is synthetic.
#[allow(dead_code)]
#[path = "../src/native.rs"]
mod native;
use native::{
    LUNA_MODEL, canonical_skill_input, child_thread_ids, configured_route, thread_is_idle,
    validate_luna_route,
};
use serde_json::json;
use std::path::Path;

#[test]
fn only_exact_catalog_model_and_supported_effort_are_accepted() {
    let catalog = json!({"data":[{"id":"gpt-6-luna","model":"gpt-6-luna","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]});
    assert!(validate_luna_route(&catalog, "high").is_ok());
    assert!(validate_luna_route(&catalog, "max").is_err());
    assert!(validate_luna_route(&json!({"data":[{"id":"gpt-6-luna","model":"gpt-6-sol","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]}), "high").is_err());
    assert!(validate_luna_route(&json!({"data":[]}), "high").is_err());
}

#[test]
fn canonical_skill_is_native_input_not_copied_instructions() {
    let path = Path::new(file!())
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .join("../../skills/luna-factory/SKILL.md")
        .canonicalize()
        .unwrap();
    let input = canonical_skill_input(&path, "Bounded task").unwrap();
    assert_eq!(input[0]["type"], "skill");
    assert_eq!(input[0]["name"], "luna-factory");
    assert_eq!(input[0]["path"], path.to_str().unwrap());
    assert_eq!(input[1]["text"], "Bounded task");
    assert!(canonical_skill_input(Path::new("relative/SKILL.md"), "x").is_err());
    assert!(canonical_skill_input(&path, "").is_err());
}

#[test]
fn configured_route_never_becomes_observed_execution_telemetry() {
    let route = configured_route(
        &json!({"model":LUNA_MODEL,"reasoningEffort":"high","modelProvider":"trusted-profile"}),
        "high",
    );
    assert_eq!(route.requested_model, LUNA_MODEL);
    assert_eq!(route.configured_model.as_deref(), Some(LUNA_MODEL));
    assert_eq!(route.observed_model, None);
    assert_eq!(route.observed_effort, None);
}

#[test]
fn descendant_graph_is_transitive_and_only_explicit_parentage_counts() {
    let records = vec![
        json!({"id":"child","parentThreadId":"owner"}),
        json!({"id":"grandchild","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"child"}}}}),
        json!({"id":"unrelated","forkedFromId":"owner"}),
    ];
    let ids = child_thread_ids("owner", &records).unwrap();
    assert_eq!(ids, vec!["child", "grandchild"]);
}

#[test]
fn uncertain_or_unloaded_thread_is_never_stopped_proof() {
    assert!(thread_is_idle(&json!({"status":{"type":"idle"}})));
    for state in ["active", "notLoaded", "systemError", "unrecognized"] {
        assert!(!thread_is_idle(&json!({"status":{"type":state}})));
    }
    assert!(!thread_is_idle(&json!({})));
}
