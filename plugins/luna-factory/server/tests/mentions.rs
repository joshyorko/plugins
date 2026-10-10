use luna_factoryd::mentions::{MentionRef, parse_mention_uri, resource_payload, search_items};
use serde_json::{Value, json};

fn run() -> Value {
    json!({
        "id":"run-123","state":"NEEDS_INPUT","objective":"Choose parser scope",
        "blocker":"The parser boundary needs an explicit choice.","remaining_gap":"Select the task scope.",
        "delta":"Tests identified the boundary.","repository":"private-alias","owner_thread":"private-thread",
        "current_subject":"private-subject","control":{"revision":7,"tasks":[
            {"id":"task-parser","title":"Review parser boundary","state":"ready","dependencies":[],"source":{"provider":"local","repository_id":"private-identity"}}
        ]}
    })
}

#[test]
fn empty_and_fuzzy_search_return_bounded_safe_run_and_task_links() {
    let runs = vec![run()];
    let empty = search_items(&runs, "").unwrap();
    let fuzzy = search_items(&runs, "PARSER").unwrap();
    let typo = search_items(&runs, "parsr").unwrap();
    let negative = search_items(&runs, "no-such-run-or-task").unwrap();

    assert_eq!(empty["items"].as_array().unwrap().len(), 1);
    assert_eq!(empty["items"][0]["type"], "resource_link");
    assert_eq!(fuzzy["items"].as_array().unwrap().len(), 2);
    assert!(
        fuzzy["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["uri"].as_str().unwrap().contains("task=task-parser"))
    );
    assert_eq!(typo["items"].as_array().unwrap().len(), 2);
    assert!(negative["items"].as_array().unwrap().is_empty());
    let serialized = fuzzy.to_string();
    for private in [
        "private-alias",
        "private-thread",
        "private-subject",
        "private-identity",
        "repository_id",
    ] {
        assert!(
            !serialized.contains(private),
            "mention search leaked {private}"
        );
    }
}

#[test]
fn mention_reads_require_exact_identity_and_revision_and_project_safe_fields() {
    let mention = MentionRef {
        run_id: "run-123".into(),
        revision: 7,
        task_id: Some("task-parser".into()),
    };
    let payload = resource_payload(&run(), &mention).unwrap();
    assert_eq!(payload["identity"]["run_id"], "run-123");
    assert_eq!(payload["identity"]["revision"], 7);
    assert_eq!(payload["task"]["id"], "task-parser");
    assert_eq!(payload["task"]["title"], "Review parser boundary");
    assert!(payload.get("repository").is_none());
    assert!(
        resource_payload(
            &run(),
            &MentionRef {
                revision: 8,
                ..mention.clone()
            }
        )
        .is_err()
    );

    assert_eq!(
        parse_mention_uri("luna-factory://mention/run-123?revision=7&task=task-parser").unwrap(),
        mention
    );
    for uri in [
        "file:///etc/passwd",
        "luna-factory://other/run-123?revision=7",
        "luna-factory://mention/run-123?revision=7&task=../x",
    ] {
        assert!(parse_mention_uri(uri).is_err(), "accepted {uri}");
    }
}
