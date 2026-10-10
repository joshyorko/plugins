use luna_factoryd::extensions::{
    FormDecision, OPENAI_LEGACY_FORM_METHOD, legacy_repository_form_params, make_repository_form,
    new_repository_form_state, parse_repository_form_result, supports_openai_form,
};
use serde_json::json;

fn candidates() -> serde_json::Value {
    json!([{
        "id":"a".repeat(64),"name":"sample-service","root_alias":"sandbox",
        "max_finish":"local_candidate"
    }])
}

#[test]
fn openai_form_requires_both_capabilities_and_supported_protocol_version() {
    let caps = json!({
        "elicitation":{"form":{"schemaValidation":true}},
        "extensions":{"openai/elicitation":{"form":{}}}
    });
    assert!(supports_openai_form("2026-07-28", &caps));
    assert!(!supports_openai_form("2025-11-25", &caps));
    assert!(!supports_openai_form(
        "2026-07-28",
        &json!({"elicitation":{"form":{}},"extensions":{}})
    ));
    assert!(!supports_openai_form(
        "2026-07-28",
        &json!({"extensions":{"openai/elicitation":{"form":{}}}})
    ));
}

#[test]
fn mrtr_form_uses_empty_core_schema_and_openai_requested_schema() {
    let candidates = candidates();
    let request_state = new_repository_form_state();
    let result = make_repository_form(&candidates, &request_state).unwrap();
    let value = serde_json::to_value(result).unwrap();
    let request = &value["inputRequests"]["repository"]["params"];
    assert_eq!(request["mode"], "form");
    assert_eq!(
        request["requestedSchema"],
        json!({"type":"object","properties":{}})
    );
    let schema = &request["_meta"]["openai/elicitation"]["requestedSchema"];
    assert_eq!(
        schema["properties"]["candidate_id"]["oneOf"][0]["const"],
        "a".repeat(64)
    );
    assert_eq!(
        schema["properties"]["request_for_local_approval"]["type"],
        "boolean"
    );
    assert!(
        request["message"]
            .as_str()
            .unwrap()
            .contains("local operator must approve")
    );
    assert_eq!(value["requestState"], request_state);
}

#[test]
fn legacy_registered_form_uses_the_openai_method_and_bounded_schema() {
    let params = legacy_repository_form_params(&candidates()).unwrap();
    assert_eq!(OPENAI_LEGACY_FORM_METHOD, "openai/elicitation/create");
    assert_eq!(params["mode"], "form");
    assert_eq!(
        params["requestedSchema"]["properties"]["candidate_id"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!params.to_string().contains("/private/path"));
}

#[test]
fn accepted_refused_cancelled_and_invalid_form_responses_are_distinguished() {
    let candidates = candidates();
    let accepted = json!({"action":"accept","content":{
        "candidate_id":"a".repeat(64),"alias":"sample","max_finish":"local_candidate","request_for_local_approval":true
    }});
    assert!(matches!(
        parse_repository_form_result(&accepted, &candidates).unwrap(),
        FormDecision::Accepted(_)
    ));
    assert!(matches!(
        parse_repository_form_result(&json!({"action":"decline"}), &candidates).unwrap(),
        FormDecision::Declined
    ));
    assert!(matches!(
        parse_repository_form_result(&json!({"action":"cancel"}), &candidates).unwrap(),
        FormDecision::Cancelled
    ));
    for invalid in [
        json!({"action":"accept","content":{"candidate_id":"missing","alias":"sample","max_finish":"local_candidate","request_for_local_approval":true}}),
        json!({"action":"accept","content":{"candidate_id":"a".repeat(64),"alias":"../private","max_finish":"local_candidate","request_for_local_approval":true}}),
        json!({"action":"accept","content":{"candidate_id":"a".repeat(64),"alias":"sample","max_finish":"pr","request_for_local_approval":true}}),
        json!({"action":"accept","content":{"candidate_id":"a".repeat(64),"alias":"sample","max_finish":"local_candidate","request_for_local_approval":false}}),
        json!({"action":"accept","content":{"candidate_id":"a".repeat(64),"alias":"sample","max_finish":"local_candidate","request_for_local_approval":true,"root":"/etc"}}),
        json!({"action":"not-an-action"}),
    ] {
        assert!(
            parse_repository_form_result(&invalid, &candidates).is_err(),
            "accepted invalid form response: {invalid}"
        );
    }
}
