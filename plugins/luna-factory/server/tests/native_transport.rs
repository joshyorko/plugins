//! Synthetic process tests. These never invoke Codex or inference.
#[allow(dead_code)]
#[path = "../src/native.rs"]
mod native;
use native::{NativeClient, NativeOptions};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn client() -> NativeClient {
    let script = Path::new(file!())
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .join("../../tests/fixtures/native/fake_app_server.py");
    NativeClient::spawn_with_options(
        Path::new("/usr/bin/python3"),
        &["-u".into(), script.to_string_lossy().into_owned()],
        NativeOptions {
            request_timeout: Duration::from_millis(150),
            max_frame_bytes: 8192,
            event_capacity: 8,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn initializes_correlates_concurrent_requests_and_streams_events() {
    let c = client().await;
    let mut events = c.subscribe();
    let (a, b) = tokio::join!(
        c.request("echo", json!({"n":1})),
        c.request("echo", json!({"n":2}))
    );
    assert_eq!(a.unwrap()["n"], 1);
    assert_eq!(b.unwrap()["n"], 2);
    c.request("notify", json!({})).await.unwrap();
    assert_eq!(events.recv().await.unwrap()["method"], "thread/started");
    c.shutdown().await.unwrap();
    assert!(c.request("echo", json!({})).await.is_err());
}

#[tokio::test]
async fn timeout_does_not_retry_a_mutating_request() {
    let c = client().await;
    let err = c
        .request("turn/start", json!({}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("timed out"));
    let counts = c.request("counts", json!({})).await.unwrap();
    assert_eq!(counts["turn/start"], 1);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn rpc_errors_do_not_leak_untrusted_error_messages() {
    let c = client().await;
    let err = c.request("error", json!({})).await.unwrap_err().to_string();
    assert!(err.contains("-32001"));
    assert!(!err.contains("fake-secret"));
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_frame_closes_transport_without_unbounded_buffering() {
    let c = client().await;
    let err = c
        .request("oversized", json!({}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("frame") || err.contains("closed"));
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn eof_resolves_pending_requests_and_shutdown_is_idempotent() {
    let c = client().await;
    assert!(c.request("exit", Value::Null).await.is_err());
    c.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn approval_requests_are_visible_and_never_auto_approved() {
    let c = client().await;
    let mut events = c.subscribe();
    c.request("approval", json!({})).await.unwrap();
    let event = events.recv().await.unwrap();
    assert_eq!(event["method"], "item/commandExecution/requestApproval");
    assert_eq!(event["id"], "server-approval");
    let counts = c.request("counts", json!({})).await.unwrap();
    assert_eq!(counts["responses"], 0);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn responses_may_arrive_out_of_order() {
    let c = client().await;
    let (a, b) = tokio::join!(
        c.request("reorder", json!({"n":1})),
        c.request("reorder", json!({"n":2}))
    );
    assert_eq!(a.unwrap()["n"], 1);
    assert_eq!(b.unwrap()["n"], 2);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_helpers_preserve_security_fields_and_use_current_protocol() {
    let c = client().await;
    let root = Path::new(file!())
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .join("../../../..")
        .canonicalize()
        .unwrap();
    let skill = root.join("plugins/luna-factory/skills/luna-factory/SKILL.md");
    let started = c.start_thread(&root, "high", 2).await.unwrap();
    assert_eq!(started["thread"]["id"], "owner");
    c.start_skill_turn(
        "owner",
        &skill,
        "Synthetic bounded task",
        "high",
        Some(json!({"type":"object"})),
    )
    .await
    .unwrap();
    c.resume_thread("owner").await.unwrap();
    c.steer_turn("owner", "turn-one", "Stay in scope")
        .await
        .unwrap();
    c.interrupt_turn("owner", "turn-one").await.unwrap();
    let history = c.request("history", json!({})).await.unwrap();
    let calls = history.as_array().unwrap();
    let start = calls
        .iter()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(
        start["params"]["config"]["agents.max_concurrent_threads_per_session"],
        2
    );
    let turn = calls.iter().find(|v| v["method"] == "turn/start").unwrap();
    assert_eq!(turn["params"]["input"][0]["type"], "skill");
    assert_eq!(turn["params"]["outputSchema"]["type"], "object");
    let resume = calls
        .iter()
        .find(|v| v["method"] == "thread/resume")
        .unwrap();
    assert_eq!(
        resume["params"],
        json!({"threadId":"owner","excludeTurns":true})
    );
    for call in calls {
        for key in [
            "sandbox",
            "approvalPolicy",
            "sandboxPolicy",
            "modelProvider",
            "serviceTier",
            "serviceTierForTurn",
        ] {
            assert!(
                call["params"].get(key).is_none(),
                "unexpected security/routing override {key}"
            );
        }
    }
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn loaded_ephemeral_descendants_are_included_and_active_turns_are_explicit() {
    let c = client().await;
    let descendants = c.descendants("owner").await.unwrap();
    assert_eq!(descendants.len(), 2);
    assert!(descendants.iter().any(|v| v["id"] == "ephemeral-child"));
    assert_eq!(c.active_turn_ids("owner").await.unwrap(), vec!["turn-one"]);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropping_last_client_stops_only_owned_process() {
    let c = client().await;
    let pid = c.request("pid", json!({})).await.unwrap().as_u64().unwrap();
    drop(c);
    let dead = tokio::time::timeout(Duration::from_secs(3), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(dead.is_ok(), "owned process survived client drop");
}

#[tokio::test]
async fn cancelling_partial_write_invalidates_transport() {
    let script = Path::new(file!())
        .canonicalize()
        .unwrap()
        .parent()
        .unwrap()
        .join("../../tests/fixtures/native/fake_app_server.py");
    let c = NativeClient::spawn_with_options(
        Path::new("/usr/bin/python3"),
        &["-u".into(), script.to_string_lossy().into_owned()],
        NativeOptions {
            request_timeout: Duration::from_millis(150),
            max_frame_bytes: 128 * 1024,
            event_capacity: 8,
        },
    )
    .await
    .unwrap();
    c.request("stop-reading", json!({})).await.unwrap();
    let err = c
        .request("echo", json!({"text":"x".repeat(100 * 1024)}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"));
    let next = c.request("echo", json!({})).await.unwrap_err();
    assert!(
        next.to_string().contains("closed"),
        "a partial JSON frame cannot safely be reused"
    );
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_history_envelopes_are_unwrapped_and_turn_fenced() {
    let c = client().await;
    let items = c.thread_items("history-owner").await.unwrap();
    assert_eq!(items[0]["type"], "commandExecution");
    assert_eq!(items[0]["processId"], "owned-pty");
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_native_history_is_never_silently_accepted_as_empty_evidence() {
    let c = client().await;
    assert!(c.thread_items("malformed-history").await.is_err());
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_history_from_another_turn_cannot_be_acceptance_evidence() {
    let c = client().await;
    assert!(
        c.turn_items("history-owner", "a-different-turn")
            .await
            .is_err()
    );
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn dispatch_correlation_uses_client_id_not_item_id_or_latest_turn() {
    let c = client().await;
    let recovered = c
        .find_dispatch_turn("correlation-owner", "dispatch-one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered["id"], "turn-one");
    assert!(
        c.find_dispatch_turn("correlation-owner", "unknown-dispatch")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        c.find_dispatch_turn("ambiguous-correlation", "dispatch-one")
            .await
            .is_err()
    );
    c.shutdown().await.unwrap();
}
