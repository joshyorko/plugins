//! Configuration-only backend discovery. A planning preference grants no execution authority.
use crate::config::Config;
use anyhow::{Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct Operation {
    advertised: bool,
    enabled: bool,
    qualified: bool,
}

#[derive(Serialize)]
struct Operations {
    discover: Operation,
    start: Operation,
    observe: Operation,
    steer: Operation,
    stop: Operation,
    reconcile: Operation,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Qualification {
    Unverified,
    Unsupported,
}

#[derive(Serialize)]
struct Target {
    id: &'static str,
    label: &'static str,
    kind: &'static str,
    namespace: &'static str,
    operator_enabled: bool,
    planning_eligible: bool,
    execution_eligible: bool,
    qualification: Qualification,
    authentication: &'static str,
    entitlement: &'static str,
    reason: &'static str,
    operations: Operations,
    limits: &'static [&'static str],
}

fn native_enabled(config: &Config) -> bool {
    matches!(
        config.native_transport.as_str(),
        "stdio" | "existing_daemon"
    )
}

fn operations(advertised: [bool; 5], enabled: bool) -> Operations {
    let [start, observe, steer, stop, reconcile] = advertised.map(|advertised| Operation {
        advertised,
        enabled: enabled && advertised,
        qualified: false,
    });
    Operations {
        // This operation describes this local catalog, never a native/account probe.
        discover: Operation {
            advertised: true,
            enabled: true,
            qualified: true,
        },
        start,
        observe,
        steer,
        stop,
        reconcile,
    }
}

fn unsupported(
    id: &'static str,
    label: &'static str,
    namespace: &'static str,
    advertised: [bool; 5],
    limits: &'static [&'static str],
) -> Target {
    Target {
        id,
        label,
        kind: id,
        namespace,
        operator_enabled: false,
        planning_eligible: false,
        execution_eligible: false,
        qualification: Qualification::Unsupported,
        authentication: "unknown",
        entitlement: "unknown",
        reason: "adapter_not_qualified",
        operations: operations(advertised, false),
        limits,
    }
}

/// Return a fixed, bounded catalog without accessing paths, environment, credentials or native RPC.
/// Advertised operations describe upstream contracts, not verified target availability.
pub fn capabilities(config: &Config) -> Value {
    let enabled = native_enabled(config);
    let targets = [
        Target {
            id: "native-local",
            label: "Native Codex",
            kind: "native_app_server",
            namespace: "native_target/thread/turn",
            operator_enabled: enabled,
            planning_eligible: enabled,
            execution_eligible: false,
            qualification: Qualification::Unverified,
            authentication: "unknown",
            entitlement: "unknown",
            reason: "subscription_entitlement_unverified",
            operations: operations([true; 5], enabled),
            limits: &[
                "Discovery reads only the supplied operator configuration; binary presence and daemon connectivity are unverified.",
                "The model catalog and configured provider are not subscription or model entitlement proof.",
                "Subscription-only dispatch requires independently verified authentication, entitlement and adapter qualification.",
                "Steering acceptance does not prove compliance; interruption does not prove descendant or terminal cessation.",
                "Existing stdio and existing_daemon configuration remains operator-owned; discovery never launches, restarts or authenticates Codex.",
            ],
        },
        unsupported(
            "cas-native",
            "Codex Action Server",
            "cas_target/cwd/thread/turn/request_id",
            [true; 5],
            &[
                "CAS resolves operator-owned targets and controls existing native daemons; this service has no qualified CAS adapter.",
                "CAS dispatch receipts prove acknowledgement and replay protection, not task completion or cessation.",
                "CAS does not expose account/read or a durable callback response broker; subscription entitlement remains unknown.",
            ],
        ),
        unsupported(
            "codex-cloud-cli",
            "Codex Cloud CLI",
            "cloud_environment/task/attempt",
            [true, true, false, false, false],
            &[
                "Official CLI documentation describes cloud submission and listing with CLI credentials.",
                "No qualified steering, stop, replay protection or reconciliation contract is established here.",
                "Cloud task identifiers are not native app-server thread identifiers.",
            ],
        ),
        unsupported(
            "codex-cloud",
            "Codex Cloud",
            "published_environment/cloud_chat",
            [true, true, false, false, false],
            &[
                "The published-environment product documents UI task creation and follow-ups; no callable adapter is qualified here.",
                "New Cloud and legacy GitHub-integrated Cloud are distinct products; CLI parity is unverified.",
            ],
        ),
        unsupported(
            "github-codex",
            "Codex on GitHub",
            "github_repository/pull_request/comment/legacy_cloud_task",
            [true, true, false, false, false],
            &[
                "GitHub review comments and non-review legacy Cloud tasks have different contracts.",
                "A GitHub comment is an external mutation and cannot provide native request-id replay protection or cancellation proof.",
                "Connected repository access is not subscription entitlement or Factory admission.",
            ],
        ),
    ];
    serde_json::json!({
        "schema_version": 1,
        "discovery": "configuration_only",
        "policy": "subscription_only",
        "targets": targets,
    })
}

/// Validate a planning-only preference. Callers must separately gate execution.
pub fn validate_preference(config: &Config, target: &str) -> Result<()> {
    match target {
        "native-local" => ensure!(native_enabled(config), "backend_not_operator_enabled"),
        "cas-native" | "codex-cloud-cli" | "codex-cloud" | "github-codex" => {
            bail!("backend_adapter_not_qualified")
        }
        _ => bail!("unknown_backend_target"),
    }
    Ok(())
}
