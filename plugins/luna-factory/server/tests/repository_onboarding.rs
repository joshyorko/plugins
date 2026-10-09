//! Real local repositories and durable approval. No native process is launched.
use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory, mcp::tool_definitions};
use serde_json::json;
use std::{path::Path, process::Command, sync::Arc};

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        path.join("README.md"),
        "private-content-must-never-be-discovered",
    )
    .unwrap();
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=Acceptance",
                "-c",
                "user.email=acceptance@localhost",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                "fixture"
            ])
            .status()
            .unwrap()
            .success()
    );
}
fn setup() -> (tempfile::TempDir, Config, McpServer) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("discovery");
    repository(&root.join("sample"));
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),"codex_binary":"/native-deliberately-absent","skill_path":dir.path().join("SKILL.md"),"repositories":{},"discovery_roots":{"tests":{"root":root,"max_finish":"local_candidate"}},"profiles":{"default":{"effort":"low"}},"limits":{"capacity":1,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    std::fs::write(
        dir.path().join("operator.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let server = McpServer {
        factory: Factory::new(config.clone()).unwrap(),
        html: Arc::new("<html></html>".into()),
    };
    (dir, config, server)
}
async fn candidate(server: &McpServer) -> String {
    let discovered = server
        .invoke("discover_factory_repositories", json!({}))
        .await
        .unwrap();
    assert_eq!(discovered["candidates"].as_array().unwrap().len(), 1);
    discovered["candidates"][0]["id"].as_str().unwrap().into()
}

#[test]
fn onboarding_tools_are_app_only_with_truthful_write_annotations() {
    let tools = serde_json::to_value(tool_definitions()).unwrap();
    for (name, read_only) in [
        ("discover_factory_repositories", true),
        ("request_factory_repository", false),
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .expect("onboarding tool missing");
        assert_eq!(tool["_meta"]["ui"]["visibility"], json!(["app"]));
        assert_eq!(tool["annotations"]["readOnlyHint"], read_only);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert!(tool["inputSchema"]["properties"].get("path").is_none());
        assert!(tool["inputSchema"]["properties"].get("root").is_none());
        assert!(tool["inputSchema"]["properties"].get("approved").is_none());
    }
}
#[tokio::test]
async fn discovery_exposes_opaque_identity_without_paths_contents_or_native_calls() {
    let (dir, _, server) = setup();
    let result = server
        .invoke("discover_factory_repositories", json!({}))
        .await
        .unwrap();
    let text = result.to_string();
    assert!(!text.contains(dir.path().to_str().unwrap()));
    assert!(!text.contains("private-content"));
    assert_eq!(result["candidates"][0]["name"], "sample");
    assert_eq!(result["candidates"][0]["root_alias"], "tests");
    assert_eq!(result["approval"], "local_operator");
    assert_eq!(
        server.invoke("list_factory_runs", json!({})).await.unwrap()["runs"],
        json!([])
    );
    assert!(
        server
            .invoke("discover_factory_repositories", json!({"path":"/etc"}))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn requests_do_not_grant_access_and_local_approval_survives_restart() {
    let (dir, config, server) = setup();
    let id = candidate(&server).await;
    let args = json!({"candidate_id":id,"alias":"sandbox-test","max_finish":"local_candidate"});
    let requested = server
        .invoke("request_factory_repository", args.clone())
        .await
        .unwrap();
    assert_eq!(requested["status"], "pending");
    assert_eq!(
        server
            .invoke("request_factory_repository", args)
            .await
            .unwrap()["id"],
        requested["id"]
    );
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
    let command = || {
        Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
            .arg("approve-repository")
            .arg("--config")
            .arg(dir.path().join("operator.json"))
            .arg("--request-id")
            .arg(requested["id"].as_str().unwrap())
            .output()
            .unwrap()
    };
    assert!(command().status.success());
    assert!(command().status.success());
    let caps = server
        .invoke("get_factory_capabilities", json!({}))
        .await
        .unwrap();
    assert_eq!(
        caps["repositories"],
        json!([{"alias":"sandbox-test","max_finish":"local_candidate"}])
    );
    assert_eq!(caps["status_inference_calls"], 0);
    drop(server);
    let restarted = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new("html".into()),
    };
    assert_eq!(
        restarted
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        caps["repositories"]
    );
}
#[tokio::test]
async fn unknown_candidates_paths_authority_and_alias_collisions_are_rejected() {
    let (_dir, _, server) = setup();
    let id = candidate(&server).await;
    for args in [
        json!({"candidate_id":"/etc","alias":"test","max_finish":"local_candidate"}),
        json!({"candidate_id":id,"alias":"../escape","max_finish":"local_candidate"}),
        json!({"candidate_id":id,"alias":"test","max_finish":"push"}),
        json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate","approved":true}),
        json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate","path":"/etc"}),
    ] {
        assert!(
            server
                .invoke("request_factory_repository", args)
                .await
                .is_err()
        );
    }
    server
        .invoke(
            "request_factory_repository",
            json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate"}),
        )
        .await
        .unwrap();
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_and_replaced_candidates_cannot_be_approved() {
    use std::os::unix::fs::symlink;
    let (dir, _, server) = setup();
    let external = dir.path().join("outside");
    repository(&external);
    symlink(&external, dir.path().join("discovery/escape")).unwrap();
    let id = candidate(&server).await;
    let requested = server
        .invoke(
            "request_factory_repository",
            json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate"}),
        )
        .await
        .unwrap();
    std::fs::rename(
        dir.path().join("discovery/sample"),
        dir.path().join("original"),
    )
    .unwrap();
    symlink(&external, dir.path().join("discovery/sample")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
        .arg("approve-repository")
        .arg("--config")
        .arg(dir.path().join("operator.json"))
        .arg("--request-id")
        .arg(requested["id"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(dir.path().to_str().unwrap()));
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
}
#[tokio::test]
async fn revoked_roots_remove_dynamic_aliases_and_existing_aliases_keep_precedence() {
    let (dir, config, server) = setup();
    let id = candidate(&server).await;
    let request = server
        .invoke(
            "request_factory_repository",
            json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate"}),
        )
        .await
        .unwrap();
    assert!(
        Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
            .args(["approve-repository", "--config"])
            .arg(dir.path().join("operator.json"))
            .arg("--request-id")
            .arg(request["id"].as_str().unwrap())
            .status()
            .unwrap()
            .success()
    );
    drop(server);
    let mut revoked = serde_json::to_value(&config).unwrap();
    revoked["discovery_roots"] = json!({});
    let server = McpServer {
        factory: Factory::new(serde_json::from_value(revoked).unwrap()).unwrap(),
        html: Arc::new("html".into()),
    };
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
    drop(server);
    let mut configured = serde_json::to_value(&config).unwrap();
    configured["repositories"] = json!({"test":{"root":dir.path().join("outside-configured"),"max_finish":"local_candidate"}});
    repository(&dir.path().join("outside-configured"));
    let server = McpServer {
        factory: Factory::new(serde_json::from_value(configured).unwrap()).unwrap(),
        html: Arc::new("html".into()),
    };
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([{"alias":"test","max_finish":"local_candidate"}])
    );
}

fn bounded_request(alias: &str, finish: &str, key: &str) -> luna_factoryd::store::StartRequest {
    serde_json::from_value(json!({"repository":alias,"objective":"A local fixture objective","acceptance":["Inspect the exact fixture"],"non_goals":[],"finish":finish,"profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":30,"idempotency_key":key})).unwrap()
}
fn approve_request(dir: &Path, request: &serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_luna-factoryd"))
        .args(["approve-repository", "--config"])
        .arg(dir.join("operator.json"))
        .arg("--request-id")
        .arg(request["id"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "local approval failed");
}
#[tokio::test]
async fn dynamic_alias_enforces_finish_and_shares_existing_repository_claims() {
    let (dir, mut config, server) = setup();
    let id = candidate(&server).await;
    let request = server
        .invoke(
            "request_factory_repository",
            json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate"}),
        )
        .await
        .unwrap();
    approve_request(dir.path(), &request);
    config.repositories.insert(
        "existing".into(),
        luna_factoryd::config::Repository {
            root: dir.path().join("discovery/sample"),
            max_finish: "local_candidate".into(),
        },
    );
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let effective = luna_factoryd::repositories::effective_config(
        &config,
        &store.repository_registrations().unwrap(),
    );
    assert!(
        store
            .admit(&effective, &bounded_request("test", "push", "too-wide"))
            .unwrap_err()
            .to_string()
            .contains("authority_exceeded")
    );
    let admitted = store
        .admit(
            &effective,
            &bounded_request("test", "local_candidate", "local"),
        )
        .unwrap();
    assert!(admitted.run.claim_held);
    assert!(
        store
            .admit(
                &effective,
                &bounded_request("existing", "local_candidate", "collision")
            )
            .unwrap_err()
            .to_string()
            .contains("repository_claimed")
    );
    assert_eq!(store.list(100).unwrap().len(), 1);
}
#[cfg(unix)]
#[tokio::test]
async fn replacing_an_approved_candidate_revokes_admission_and_resume() {
    let (dir, config, server) = setup();
    let id = candidate(&server).await;
    let request = server
        .invoke(
            "request_factory_repository",
            json!({"candidate_id":id,"alias":"test","max_finish":"local_candidate"}),
        )
        .await
        .unwrap();
    approve_request(dir.path(), &request);
    let mut store = luna_factoryd::store::Store::open(&config).unwrap();
    let effective = luna_factoryd::repositories::effective_config(
        &config,
        &store.repository_registrations().unwrap(),
    );
    let admitted = store
        .admit(
            &effective,
            &bounded_request("test", "local_candidate", "before-replacement"),
        )
        .unwrap();
    std::fs::rename(
        dir.path().join("discovery/sample"),
        dir.path().join("retained-original"),
    )
    .unwrap();
    repository(&dir.path().join("discovery/sample"));
    assert_eq!(
        server
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
    let revoked = luna_factoryd::repositories::effective_config(
        &config,
        &store.repository_registrations().unwrap(),
    );
    assert!(
        store
            .admit(
                &revoked,
                &bounded_request("test", "local_candidate", "after-replacement")
            )
            .unwrap_err()
            .to_string()
            .contains("unknown_repository")
    );
    assert!(
        server
            .factory
            .resume(&admitted.run.id)
            .await
            .unwrap_err()
            .to_string()
            .contains("unknown_repository")
    );
    assert!(store.get(&admitted.run.id).unwrap().claim_held);
}
#[tokio::test]
async fn discovery_depth_and_entry_limits_bound_the_read_only_scan() {
    let (dir, _, server) = setup();
    repository(&dir.path().join("discovery/a/b/c/d/e/sample"));
    assert_eq!(
        server
            .invoke("discover_factory_repositories", json!({}))
            .await
            .unwrap()["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for index in 0..2048 {
        std::fs::write(dir.path().join(format!("discovery/file-{index}")), "").unwrap();
    }
    assert!(
        server
            .invoke("discover_factory_repositories", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("discovery_entry_limit")
    );
}
#[cfg(unix)]
#[tokio::test]
async fn discovery_and_status_do_not_launch_the_configured_native_executable() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, mut config, server) = setup();
    drop(server);
    let marker = dir.path().join("native-was-launched");
    let binary = dir.path().join("native-probe");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    config.codex_binary = binary;
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new("html".into()),
    };
    for method in [
        "discover_factory_repositories",
        "get_factory_capabilities",
        "list_factory_runs",
        "open_factory",
        "open_factory_panel",
        "refresh_factory",
        "read_factory_settings",
    ] {
        server.invoke(method, json!({})).await.unwrap();
    }
    assert!(!marker.exists());
}
