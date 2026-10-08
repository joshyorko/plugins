use luna_factoryd::{backends, config::Config};
use serde_json::{Value, json};

fn config() -> Config {
    serde_json::from_value(json!({
        "listen": "127.0.0.1:8787",
        "database": "/deliberately-absent/state.sqlite",
        "codex_binary": "/deliberately-absent/codex",
        "skill_path": "/deliberately-absent/SKILL.md",
        "repositories": {},
        "profiles": {"default": {"effort": "low"}},
        "limits": {"capacity": 1, "repair_attempts": 0, "wall_seconds": 30}
    }))
    .unwrap()
}

fn native(value: &Value) -> &Value {
    &value["targets"][0]
}

#[test]
fn absent_binary_does_not_prevent_planning_or_prove_execution() {
    let config = config();
    let result = backends::capabilities(&config);
    assert_eq!(result["discovery"], "configuration_only");
    assert_eq!(result["policy"], "subscription_only");
    assert_eq!(native(&result)["id"], "native-local");
    assert_eq!(native(&result)["operator_enabled"], true);
    assert_eq!(native(&result)["planning_eligible"], true);
    assert_eq!(native(&result)["execution_eligible"], false);
    assert_eq!(native(&result)["authentication"], "unknown");
    assert_eq!(native(&result)["entitlement"], "unknown");
    assert_eq!(native(&result)["qualification"], "unverified");
    backends::validate_preference(&config, "native-local").unwrap();
    assert!(serde_json::to_vec(&result).unwrap().len() < 8192);
}

#[test]
fn discovery_ignores_operator_paths_and_never_executes_present_binary() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("codex");
    let marker = dir.path().join("invoked");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\ntouch '{}'\nexit 99\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = config();
    let before = backends::capabilities(&config);
    config.codex_binary = binary;
    config.skill_path = dir.path().join("credential-file-must-not-be-read");
    std::fs::write(&config.skill_path, "secret-sentinel").unwrap();
    config.native_transport = "existing_daemon".into();
    config.native_socket = Some(dir.path().join("nonexistent-native-socket"));
    assert_eq!(before, backends::capabilities(&config));
    assert!(!marker.exists());
    assert!(!before.to_string().contains("secret-sentinel"));
}

#[test]
fn discovery_has_no_environment_credential_or_process_dependencies() {
    let source = include_str!("../src/backends.rs");
    for forbidden in [
        "std::env",
        "std::fs",
        "std::process",
        "tokio::process",
        "NativeClient",
        "Config::load",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected discovery dependency: {forbidden}"
        );
    }
}

#[test]
fn only_recognized_native_transport_can_be_a_planning_preference() {
    let mut config = config();
    for target in [
        "cas-native",
        "codex-cloud-cli",
        "codex-cloud",
        "github-codex",
        "unknown",
        "",
        "native-local/../other",
    ] {
        assert!(backends::validate_preference(&config, target).is_err());
    }
    config.native_transport = "unrecognized".into();
    assert!(backends::validate_preference(&config, "native-local").is_err());
    assert_eq!(
        native(&backends::capabilities(&config))["operator_enabled"],
        false
    );
}

#[test]
fn catalog_distinguishes_upstream_advertising_from_qualification() {
    let result = backends::capabilities(&config());
    let targets = result["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 5);
    for (index, target) in targets.iter().enumerate() {
        assert_eq!(target["execution_eligible"], false);
        assert_eq!(target["entitlement"], "unknown");
        for name in ["start", "observe", "steer", "stop", "reconcile"] {
            assert_eq!(target["operations"][name]["qualified"], false);
            if index > 0 {
                assert_eq!(target["operations"][name]["enabled"], false);
            }
        }
        if index > 0 {
            assert_eq!(target["planning_eligible"], false);
            assert_eq!(target["qualification"], "unsupported");
        }
    }
    assert_eq!(targets[1]["operations"]["stop"]["advertised"], true);
    assert_eq!(targets[2]["operations"]["stop"]["advertised"], false);
}
