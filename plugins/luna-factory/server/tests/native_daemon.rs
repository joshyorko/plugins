//! Native daemon transport fixtures use a real Unix WebSocket, never a JSONL proxy.
#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use luna_factoryd::native::NativeClient;
use serde_json::{Value, json};
use std::path::Path;
use tokio::net::UnixListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn existing_daemon_uses_websocket_frames_and_closes_only_connection() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = ws.next().await {
                let message: Value = serde_json::from_str(&text).unwrap();
                if let Some(id) = message.get("id") {
                    assert!(matches!(
                        message["method"].as_str(),
                        Some("initialize" | "model/list")
                    ));
                    let result = if message["method"] == "initialize" {
                        json!({"userAgent":"synthetic-unix-websocket"})
                    } else {
                        json!({"data":[{"model":"gpt-6-luna","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]})
                    };
                    ws.send(Message::Text(
                        json!({"id":id,"result":result}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                }
            }
        }
    });
    for connection in 0..2 {
        let client = NativeClient::connect_existing(
            Path::new("/absent/binary-is-not-executed"),
            Some(&socket),
        )
        .await
        .unwrap();
        assert_eq!(
            client.list_models().await.unwrap()["data"][0]["model"],
            "gpt-6-luna"
        );
        if connection == 0 {
            drop(client); // last-client drop must close the connection too
        } else {
            client.shutdown().await.unwrap();
        }
    }
    server.await.unwrap();
}

#[tokio::test]
async fn cancelled_backpressured_write_cannot_be_flushed_by_a_later_ping() {
    use luna_factoryd::native::NativeOptions;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (paused, at_pause) = tokio::sync::oneshot::channel();
    let (resume, resumed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let initialize: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        ws.send(Message::Text(
            json!({"id":initialize["id"],"result":{}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        let initialized: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(initialized["method"], "initialized");
        paused.send(()).unwrap();
        resumed.await.unwrap();
        let _ = ws.send(Message::Ping(vec![1, 2, 3].into())).await;
        let mut complete_rpc = false;
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout(Duration::from_secs(2), ws.next()).await
        {
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                complete_rpc |= value["method"] == "turn/start";
                break;
            }
        }
        complete_rpc
    });
    let client = NativeClient::connect_existing_with_options(
        Path::new("/absent"),
        Some(&socket),
        NativeOptions {
            request_timeout: Duration::from_secs(5),
            max_frame_bytes: 4 * 1024 * 1024,
            event_capacity: 8,
        },
    )
    .await
    .unwrap();
    at_pause.await.unwrap();
    let sending_client = client.clone();
    let pending = tokio::spawn(async move {
        sending_client
            .request(
                "turn/start",
                json!({"threadId":"owner","input":"x".repeat(2 * 1024 * 1024)}),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!pending.is_finished());
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(
        client.is_closed(),
        "cancelled partial write must poison transport"
    );
    resume.send(()).unwrap();
    assert!(
        !server.await.unwrap(),
        "a poisoned transport flushed the aborted RPC later"
    );
    client.shutdown().await.unwrap();
}

type ServerSocket = tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>;
async fn message(ws: &mut ServerSocket) -> Value {
    serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap()
}
async fn reply(ws: &mut ServerSocket, id: &Value, result: Value) {
    ws.send(Message::Text(
        json!({"id":id,"result":result}).to_string().into(),
    ))
    .await
    .unwrap();
}
async fn initialized(listener: &UnixListener) -> ServerSocket {
    let (stream, _) = listener.accept().await.unwrap();
    let mut ws = accept_async(stream).await.unwrap();
    let request = message(&mut ws).await;
    assert_eq!(request["method"], "initialize");
    reply(
        &mut ws,
        &request["id"],
        json!({"userAgent":"synthetic-daemon"}),
    )
    .await;
    assert_eq!(message(&mut ws).await["method"], "initialized");
    ws
}

#[tokio::test]
async fn raw_byte_proxy_cannot_turn_jsonl_into_a_websocket_handshake() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        assert!(accept_async(stream).await.is_err());
    });
    let proxy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native/raw_proxy.py");
    let result = NativeClient::spawn(
        Path::new("/usr/bin/python3"),
        &[
            proxy.to_string_lossy().into_owned(),
            "--sock".into(),
            socket.to_string_lossy().into_owned(),
        ],
    )
    .await;
    assert!(result.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn websocket_callbacks_are_events_and_never_automatic_approvals() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let mut ws = initialized(&listener).await;
        let trigger = message(&mut ws).await;
        ws.send(Message::Ping(vec![1].into())).await.unwrap();
        ws.send(Message::Text(json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"threadId":"owner"}}).to_string().into())).await.unwrap();
        reply(&mut ws, &trigger["id"], json!({"delivered":true})).await;
        // Skip only the protocol pong; any callback response is a failure.
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Pong(_) => {}
                Message::Text(text) => {
                    let next: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(next["method"], "read-only-check");
                    reply(&mut ws, &next["id"], json!({"approved":false})).await;
                    break;
                }
                other => panic!("unexpected callback response: {other:?}"),
            }
        }
    });
    let client = NativeClient::connect_existing(Path::new("/absent"), Some(&socket))
        .await
        .unwrap();
    let mut events = client.subscribe();
    client.request("trigger", json!({})).await.unwrap();
    let event = events.recv().await.unwrap();
    assert_eq!(event["id"], "approval-1");
    assert_eq!(event["method"], "item/commandExecution/requestApproval");
    assert_eq!(
        client.request("read-only-check", json!({})).await.unwrap()["approved"],
        false
    );
    server.await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_and_oversized_websocket_frames_close_pending_requests() {
    use luna_factoryd::native::NativeOptions;
    for data in ["not-json".to_owned(), "x".repeat(2048)] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let mut ws = initialized(&listener).await;
            message(&mut ws).await;
            let _ = ws.send(Message::Text(data.into())).await;
            let _ = ws.next().await;
        });
        let client = NativeClient::connect_existing_with_options(
            Path::new("/absent"),
            Some(&socket),
            NativeOptions {
                max_frame_bytes: 1024,
                ..NativeOptions::default()
            },
        )
        .await
        .unwrap();
        assert!(client.request("read", json!({})).await.is_err());
        assert!(client.is_closed());
        server.await.unwrap();
        client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn websocket_preflight_and_writer_guard_preserve_the_original_deadline() {
    use std::sync::atomic::{AtomicU64, Ordering};
    for operation in ["thread/start", "turn/start"] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (entered, at_preflight) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut ws = initialized(&listener).await;
            let catalog = message(&mut ws).await;
            assert_eq!(catalog["method"], "model/list");
            entered.send(()).unwrap();
            released.await.unwrap();
            reply(&mut ws, &catalog["id"], json!({"data":[{"model":"gpt-6-luna","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]})).await;
            if operation == "turn/start" {
                let read = message(&mut ws).await;
                assert_eq!(read["method"], "thread/read");
                reply(
                    &mut ws,
                    &read["id"],
                    json!({"thread":{"id":"owner","model":"gpt-6-luna","status":{"type":"idle"}}}),
                )
                .await;
            }
            let read = message(&mut ws).await;
            assert_eq!(
                read["method"], "safe-read",
                "expired mutation reached daemon"
            );
            reply(&mut ws, &read["id"], json!({})).await;
        });
        let client = NativeClient::connect_existing(Path::new("/absent"), Some(&socket))
            .await
            .unwrap();
        let skill = dir.path().join("luna-factory/SKILL.md");
        std::fs::create_dir(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "Synthetic canonical skill").unwrap();
        let clock = AtomicU64::new(99);
        let guard = || {
            anyhow::ensure!(clock.load(Ordering::SeqCst) < 100, "time_budget_exhausted");
            Ok(())
        };
        let dispatch = async {
            if operation == "thread/start" {
                client
                    .start_thread_guarded(dir.path(), "high", 1, &guard)
                    .await
            } else {
                client
                    .start_skill_turn_with_id_guarded(
                        "owner",
                        &skill,
                        "Bounded fixture",
                        "high",
                        None,
                        Some("dispatch-one"),
                        &guard,
                    )
                    .await
            }
        };
        let advance = async {
            at_preflight.await.unwrap();
            clock.store(100, Ordering::SeqCst);
            release.send(()).unwrap();
        };
        let (result, ()) = tokio::join!(dispatch, advance);
        assert_eq!(result.unwrap_err().to_string(), "time_budget_exhausted");
        assert!(!client.is_closed());
        client.request("safe-read", json!({})).await.unwrap();
        server.await.unwrap();
        client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn daemon_discovery_is_bounded_read_only_and_requires_a_running_absolute_socket() {
    use luna_factoryd::native::NativeOptions;
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    for scenario in ["running", "stopped", "relative", "oversized", "hang"] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let binary = dir.path().join("codex-fixture");
        let program = format!(
            r#"#!/usr/bin/env python3
import json, pathlib, sys, time
home=pathlib.Path(__file__).parent
(home/'argv.json').write_text(json.dumps(sys.argv[1:]))
scenario={scenario:?}
if scenario=='hang': time.sleep(10)
elif scenario=='oversized': print('x'*65537)
else: print(json.dumps({{'status':'running' if scenario!='stopped' else 'stopped','socketPath':'relative.sock' if scenario=='relative' else str(home/'daemon.sock')}}))
"#
        );
        std::fs::write(&binary, program).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let server = if scenario == "running" {
            Some(tokio::spawn(async move {
                let _ws = initialized(&listener).await;
            }))
        } else {
            None
        };
        let result = NativeClient::connect_existing_with_options(
            &binary,
            None,
            NativeOptions {
                request_timeout: Duration::from_millis(500),
                ..NativeOptions::default()
            },
        )
        .await;
        assert_eq!(result.is_ok(), scenario == "running", "{scenario}");
        let args: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("argv.json")).unwrap()).unwrap();
        assert_eq!(args, json!(["app-server", "daemon", "version"]));
        if let Ok(client) = result {
            client.shutdown().await.unwrap();
        }
        if let Some(server) = server {
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn stalled_websocket_handshake_is_bounded_without_starting_a_daemon() {
    use luna_factoryd::native::NativeOptions;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        let mut trailing = Vec::new();
        stream.read_to_end(&mut trailing).await.unwrap();
    });
    let result = NativeClient::connect_existing_with_options(
        Path::new("/absent"),
        Some(&socket),
        NativeOptions {
            request_timeout: Duration::from_millis(100),
            ..NativeOptions::default()
        },
    )
    .await;
    assert!(result.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn doctor_uses_the_same_read_only_unix_websocket_connector() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let mut ws = initialized(&listener).await;
        let catalog = message(&mut ws).await;
        assert_eq!(catalog["method"], "model/list");
        reply(&mut ws, &catalog["id"], json!({"data":[{"model":"gpt-6-luna","supportedReasoningEfforts":[{"reasoningEffort":"low"}]}]})).await;
        // EOF is expected: doctor must close only its client connection.
        assert!(!matches!(ws.next().await, Some(Ok(Message::Text(_)))));
    });
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "Synthetic doctor fixture; no real skill proof").unwrap();
    let config = dir.path().join("config.json");
    std::fs::write(&config, json!({
        "listen":"127.0.0.1:8787", "database":dir.path().join("state/runs.sqlite"),
        "codex_binary":"/absent/not-spawned", "native_transport":"existing_daemon", "native_socket":socket,
        "skill_path":skill, "repositories":{"fixture":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}}, "limits":{"capacity":1,"repair_attempts":1,"wall_seconds":30}
    }).to_string()).unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
        .args(["doctor", "--config"])
        .arg(&config)
        .arg("--probe-native")
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["native_probe"]["initialize"], "passed");
    assert_eq!(report["native_probe"]["catalog_luna_low"], true);
    assert_eq!(report["inference_calls"], 0);
    server.await.unwrap();
}
