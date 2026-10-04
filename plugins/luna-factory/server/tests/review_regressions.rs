//! Actual Factory regression tests over a labeled subprocess fixture. No live inference.
use luna_factoryd::{
    config::Config,
    lifecycle::Factory,
    store::{Run, StartRequest, Store},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

fn init_repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

fn setup() -> (tempfile::TempDir, Config, StartRequest) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    let binary = dir.path().join("review-native");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native/review_runtime.py"),
        &binary,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let skill = dir.path().join("skills/luna-factory/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        "Synthetic review fixture; no live skill loading proof",
    )
    .unwrap();
    let config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787", "native_transport":"existing_daemon",
        "database":dir.path().join("state/runs.sqlite"), "codex_binary":binary, "skill_path":skill,
        "repositories":{"fixture":{"root":repo,"max_finish":"pr"}},
        "profiles":{"default":{"effort":"high"}},
        "limits":{"capacity":2,"repair_attempts":3,"wall_seconds":300}
    }))
    .unwrap();
    let request = serde_json::from_value(json!({
        "repository":"fixture", "objective":"Synthetic bounded review regression", "acceptance":["fixture only"],
        "non_goals":[], "finish":"pr", "profile":"default", "capacity":2,
        "repair_attempts":3, "wall_seconds":300, "idempotency_key":"review-1"
    })).unwrap();
    (dir, config, request)
}

fn calls(dir: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn seed(config: &Config, request: &StartRequest) -> Run {
    let mut store = Store::open(config).unwrap();
    let mut run = store.admit(config, request).unwrap().run;
    run.thread_id = Some("owner".into());
    run.turn_id = Some("turn-1".into());
    store.save(&run).unwrap();
    run
}

async fn wait_state(factory: &Factory, id: &str, expected: &str) -> Value {
    let wait = if expected == "CANCELLED" { 35 } else { 6 };
    tokio::time::timeout(Duration::from_secs(wait), async {
        loop {
            let run = factory.get(id).await.unwrap();
            if run["state"] == expected {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("run never reached {expected}"))
}

#[tokio::test]
async fn restart_rejects_revoked_repository_profile_and_reduced_authority_before_native_calls() {
    for change in [
        "remove_repository",
        "lower_finish",
        "remove_profile",
        "unsupported_profile",
        "remap_repository",
        "lower_capacity",
    ] {
        let (dir, mut config, request) = setup();
        let run = seed(&config, &request);
        match change {
            "remove_repository" => config.repositories.clear(),
            "lower_finish" => {
                config.repositories.get_mut("fixture").unwrap().max_finish =
                    "local_candidate".into()
            }
            "remove_profile" => config.profiles.clear(),
            "unsupported_profile" => {
                config.profiles.get_mut("default").unwrap().codex_profile = Some("different".into())
            }
            "remap_repository" => {
                let other = dir.path().join("other");
                init_repo(&other);
                config.repositories.get_mut("fixture").unwrap().root = other;
            }
            "lower_capacity" => config.limits.capacity = 1,
            _ => unreachable!(),
        }
        let factory = Factory::new(config).unwrap();
        assert!(
            factory.resume(&run.id).await.is_err(),
            "resumed after {change}"
        );
        assert!(
            calls(&dir).is_empty(),
            "contacted native runtime after {change}"
        );
        assert_eq!(factory.get(&run.id).await.unwrap()["claim_held"], true);
    }
}

#[tokio::test]
async fn event_lag_does_not_remove_the_persisted_deadline_watchdog() {
    let (dir, config, mut request) = setup();
    request.wall_seconds = 30;
    let run = seed(&config, &request);
    std::fs::write(dir.path().join("mode"), "lag").unwrap();
    std::fs::write(
        dir.path().join("native-state.json"),
        json!({"active":{"owner":true,"child":true},"turn":1,"closed_once":false}).to_string(),
    )
    .unwrap();
    let factory = Factory::new(config).unwrap();
    factory.reconcile_startup().await.unwrap();
    let blocked = wait_state(&factory, &run.id, "BLOCKED").await;
    assert_eq!(blocked["claim_held"], true);
    let stopped = wait_state(&factory, &run.id, "CANCELLED").await;
    assert_eq!(stopped["claim_held"], false);
    let history = calls(&dir);
    assert!(
        history
            .iter()
            .any(|call| call["method"] == "turn/interrupt")
    );
    assert!(
        !history.iter().any(|call| call["method"] == "turn/start"),
        "reconciliation must not infer"
    );
}

#[tokio::test]
async fn closed_daemon_proxy_reconnects_and_cancels_the_same_owned_execution() {
    let (dir, config, request) = setup();
    std::fs::write(dir.path().join("mode"), "close_once").unwrap();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    wait_state(&factory, id, "BLOCKED").await;
    let stopped = tokio::time::timeout(Duration::from_secs(5), factory.cancel(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stopped["state"], "CANCELLED");
    assert_eq!(stopped["claim_held"], false);
    let history = calls(&dir);
    assert_eq!(
        history
            .iter()
            .filter(|call| call["method"] == "initialize")
            .count(),
        2
    );
    assert_eq!(
        history
            .iter()
            .filter(|call| call["method"] == "thread/start")
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|call| call["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_keeps_one_event_listener_and_counts_each_child_failure_once() {
    let (dir, config, request) = setup();
    let factory = Factory::new(config).unwrap();
    let run = factory.start(request).await.unwrap();
    let id = run["id"].as_str().unwrap();
    assert_eq!(factory.cancel(id).await.unwrap()["state"], "CANCELLED");
    let resumed = factory.resume(id).await.unwrap();
    assert_eq!(resumed["repairs_used"], 1);
    std::fs::write(dir.path().join("mode"), "child_failure").unwrap();
    factory
        .steer(
            id,
            resumed["turn_id"].as_str().unwrap(),
            "Synthetic in-scope correction",
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if factory.get(id).await.unwrap()["repairs_used"]
                .as_u64()
                .unwrap()
                >= 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        factory.get(id).await.unwrap()["repairs_used"],
        2,
        "one native failure was counted by multiple listeners"
    );
    factory.cancel(id).await.unwrap();
}
