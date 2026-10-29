use luna_factoryd::{config::Config, qualification::ExecutionProfile};
use serde_json::json;
fn config() -> (tempfile::TempDir, Config) {
    let dir = tempfile::tempdir().unwrap();
    let config=serde_json::from_value(json!({"listen":"127.0.0.1:8787","database":dir.path().join("state/runs.sqlite"),
        "native_transport":"existing_daemon","native_socket":dir.path().join("daemon.sock"),"codex_binary":"/usr/bin/false","skill_path":dir.path().join("skill/SKILL.md"),
        "repositories":{},"profiles":{"default":{"effort":"high"}},"limits":{"capacity":1,"repair_attempts":0,"wall_seconds":30}})).unwrap();
    (dir, config)
}
#[test]
fn daemon_paths_require_explicit_mapping_and_mapping_drift_changes_the_profile() {
    let (dir, mut config) = config();
    let root = dir.path().join("repo");
    assert_eq!(
        config.native_path(&root).unwrap_err().to_string(),
        "native_execution_path_unmapped"
    );
    let original = ExecutionProfile::from_config(&config, "default")
        .unwrap()
        .fingerprint();
    config
        .native_paths
        .insert(root.clone(), "/host/work/repo".into());
    assert_eq!(
        config.native_path(&root.join("sub/file")).unwrap(),
        std::path::PathBuf::from("/host/work/repo/sub/file")
    );
    assert_ne!(
        ExecutionProfile::from_config(&config, "default")
            .unwrap()
            .fingerprint(),
        original
    );
    let first = ExecutionProfile::from_config(&config, "default")
        .unwrap()
        .fingerprint();
    config
        .native_paths
        .insert(root.clone(), "/different/host/repo".into());
    assert_ne!(
        ExecutionProfile::from_config(&config, "default")
            .unwrap()
            .fingerprint(),
        first
    );
    config.native_paths.insert(root.clone(), root.clone());
    assert_eq!(
        config.native_path(&root).unwrap(),
        root,
        "identity mapping is explicit too"
    );
    assert!(config.native_path(&dir.path().join("unmapped")).is_err());
    config.native_transport = "stdio".into();
    config.native_paths.clear();
    assert_eq!(config.native_path(&root).unwrap(), root);
}
#[test]
fn mapping_rejects_ambiguous_roots_and_traversal() {
    let (dir, mut config) = config();
    let root = dir.path().join("repo");
    config
        .native_paths
        .insert(root.clone(), "/host/repo".into());
    config.validate().unwrap();
    config
        .native_paths
        .insert(root.join("nested"), "/host/elsewhere".into());
    assert_eq!(
        config.validate().unwrap_err().to_string(),
        "overlapping_native_path_mappings"
    );
    config.native_paths.clear();
    for path in ["relative", "/host/../escape", "/"] {
        config.native_paths.insert(root.clone(), path.into());
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "invalid_native_path_mapping"
        );
    }
    assert!(config.native_path(&root.join("../escape")).is_err());
}
#[cfg(unix)]
#[test]
fn mapping_refuses_a_symlinked_factory_root() {
    let (dir, mut config) = config();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(real, &link).unwrap();
    config.native_paths.insert(link, "/host/real".into());
    assert_eq!(
        config.validate().unwrap_err().to_string(),
        "native_path_mapping_symlink"
    );
}
