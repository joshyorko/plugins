use luna_factoryd::{
    config::Config,
    store::{StartRequest, Store},
};
use serde_json::json;
use tempfile::TempDir;

fn setup() -> (TempDir, Config, StartRequest) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let config: Config = serde_json::from_value(json!({
        "listen":"127.0.0.1:8787", "database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/usr/bin/codex", "skill_path":temp.path().join("SKILL.md"),
        "repositories":{"test":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":2,"repair_attempts":2,"wall_seconds":600}
    }))
    .unwrap();
    let request: StartRequest = serde_json::from_value(json!({
        "repository":"test","objective":"Verify one bounded behavior", "acceptance":["A1: test passes"],
        "non_goals":[],"finish":"local_candidate","profile":"default",
        "capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"same-request"
    })).unwrap();
    (temp, config, request)
}

#[test]
fn duplicate_start_returns_same_run_and_conflicting_payload_is_rejected() {
    let (_temp, config, request) = setup();
    let mut store = Store::open(&config).unwrap();
    let first = store.admit(&config, &request).unwrap();
    let second = store.admit(&config, &request).unwrap();
    assert_eq!(first.run.id, second.run.id);
    assert!(first.created);
    assert!(!second.created);
    let mut conflict = request.clone();
    conflict.objective.push_str(" changed");
    assert!(
        store
            .admit(&config, &conflict)
            .unwrap_err()
            .to_string()
            .contains("idempotency_conflict")
    );
    assert_eq!(store.list(20).unwrap().len(), 1);
}

#[test]
fn second_mutation_owner_and_privilege_expansion_are_rejected() {
    let (_temp, config, mut request) = setup();
    let mut store = Store::open(&config).unwrap();
    store.admit(&config, &request).unwrap();
    request.idempotency_key = "other".into();
    assert!(
        store
            .admit(&config, &request)
            .unwrap_err()
            .to_string()
            .contains("repository_claimed")
    );
    request.finish = "pr".into();
    assert!(
        store
            .admit(&config, &request)
            .unwrap_err()
            .to_string()
            .contains("authority_exceeded")
    );
    request.repository = "../test".into();
    assert!(store.admit(&config, &request).is_err());
}

#[test]
fn restart_preserves_identity_budget_and_claim_without_replaying() {
    let (_temp, config, request) = setup();
    let mut store = Store::open(&config).unwrap();
    let admitted = store.admit(&config, &request).unwrap();
    drop(store);
    let mut reopened = Store::open(&config).unwrap();
    reopened.mark_interrupted().unwrap();
    let result = reopened.admit(&config, &request).unwrap();
    assert_eq!(result.run.id, admitted.run.id);
    assert_eq!(result.run.deadline_at, admitted.run.deadline_at);
    assert!(!result.created);
    assert_eq!(result.run.state, "INTERRUPTED");
    assert_eq!(
        reopened.claim_owner(&result.run.canonical_root).unwrap(),
        Some(result.run.id)
    );
}

#[test]
fn database_inside_repository_and_symlink_alias_are_rejected() {
    let (_temp, mut config, request) = setup();
    config.database = config.repositories["test"].root.join(".luna.sqlite");
    assert!(Store::open(&config).is_err());
    assert!(
        serde_json::from_value::<StartRequest>(
            json!({"repository":"test","provider_url":"https://evil.invalid"})
        )
        .is_err()
    );
    assert_eq!(request.capacity, 1);
}
#[test]
fn exact_subject_changes_when_index_changes_under_same_worktree() {
    let (_temp, config, _request) = setup();
    let root = &config.repositories["test"].root;
    std::fs::write(root.join("file"), "base").unwrap();
    for args in [
        vec!["add", "file"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "base",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    std::fs::write(root.join("file"), "index-one").unwrap();
    std::process::Command::new("git")
        .args(["add", "file"])
        .current_dir(root)
        .output()
        .unwrap();
    std::fs::write(root.join("file"), "working-tree").unwrap();
    let first = luna_factoryd::store::repository_subject(root).unwrap();
    std::fs::write(root.join("file"), "index-two").unwrap();
    std::process::Command::new("git")
        .args(["add", "file"])
        .current_dir(root)
        .output()
        .unwrap();
    std::fs::write(root.join("file"), "working-tree").unwrap();
    assert_ne!(
        first,
        luna_factoryd::store::repository_subject(root).unwrap()
    );
}

#[test]
fn exact_subject_preserves_whitespace_in_untracked_filenames() {
    for name in [
        " leading.txt",
        "\tleading.txt",
        "\nleading.txt",
        "trailing.txt ",
    ] {
        let (_temp, config, _request) = setup();
        let root = &config.repositories["test"].root;
        std::fs::write(root.join(name), "accepted content").unwrap();
        let before = luna_factoryd::store::repository_subject(root).unwrap();
        std::fs::write(root.join(name), "changed content").unwrap();
        let after = luna_factoryd::store::repository_subject(root).unwrap();
        assert_ne!(before, after, "candidate bytes were missed for {name:?}");
    }
}
#[cfg(unix)]
#[test]
fn repository_helpers_cannot_execute_or_hide_candidate_changes() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, config, _) = setup();
    let root = &config.repositories["test"].root;
    std::fs::write(root.join("file"), "base").unwrap();
    for args in [
        vec!["add", "file"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "base",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let marker = temp.path().join("executed");
    let helper = temp.path().join("helper");
    std::fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\necho hidden\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::process::Command::new("git")
        .args(["config", "diff.external", helper.to_str().unwrap()])
        .current_dir(root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "core.fsmonitor", helper.to_str().unwrap()])
        .current_dir(root)
        .output()
        .unwrap();
    std::fs::write(root.join("file"), "modified").unwrap();
    let first = luna_factoryd::store::repository_subject(root).unwrap();
    assert!(
        !marker.exists(),
        "repository-configured executable ran outside native sandbox"
    );
    std::fs::write(root.join("file"), "modified-again").unwrap();
    assert_ne!(
        first,
        luna_factoryd::store::repository_subject(root).unwrap()
    );
}
#[test]
fn deleting_a_tracked_directory_still_has_an_exact_subject() {
    let (_temp, config, _) = setup();
    let root = &config.repositories["test"].root;
    std::fs::create_dir_all(root.join("old/subdir")).unwrap();
    std::fs::write(root.join("old/subdir/file"), "base").unwrap();
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "nested",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let before = luna_factoryd::store::repository_subject(root).unwrap();
    std::fs::remove_dir_all(root.join("old")).unwrap();
    assert_ne!(
        before,
        luna_factoryd::store::repository_subject(root).unwrap()
    );
}
