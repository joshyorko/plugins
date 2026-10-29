use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    #[serde(default)]
    pub published_origin: Option<String>,
    #[serde(default = "default_transport")]
    pub native_transport: String,
    #[serde(default)]
    pub native_socket: Option<PathBuf>,
    /// Operator-declared shared paths: Factory namespace root -> daemon namespace root.
    /// Every daemon execution path requires an explicit mapping, including identity maps.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native_paths: BTreeMap<PathBuf, PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cas_targets: BTreeMap<String, crate::cas::CasTarget>,
    /// Optional, operator-only: repository alias -> CAS target alias whose exact
    /// target and CWD may be read for that repository's Factory-known threads.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cas_timelines: BTreeMap<String, String>,
    pub database: PathBuf,
    pub codex_binary: PathBuf,
    pub skill_path: PathBuf,
    pub repositories: BTreeMap<String, Repository>,
    #[serde(default)]
    pub discovery_roots: BTreeMap<String, Repository>,
    pub profiles: BTreeMap<String, Profile>,
    pub limits: Limits,
    /// Optional read-only GitHub App. Never serialized or projected to tools.
    #[serde(default, skip_serializing)]
    pub github_app: Option<crate::github::GithubAppConfig>,
    /// Optional workspace root for connected and created repositories. Both
    /// flags default to false; tools only ever see the flags, never the path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<crate::workspaces::Workspaces>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub root: PathBuf,
    pub max_finish: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub effort: String,
    #[serde(default)]
    pub codex_profile: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub capacity: u32,
    pub repair_attempts: u32,
    pub wall_seconds: u64,
}

pub fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn finish_rank(value: &str) -> Result<u8> {
    match value {
        "local_candidate" => Ok(0),
        "push" => Ok(1),
        "pr" => Ok(2),
        _ => bail!("unsupported_finish_authority"),
    }
}
/// Resolve through the nearest existing ancestor, including symlinks, before any write.
pub fn canonical_destination(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "absolute_operator_path_required");
    ensure!(
        !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "traversal_rejected"
    );
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("destination_has_no_parent")?;
    Ok(canonical_destination(parent)?.join(path.file_name().context("destination_has_no_name")?))
}
fn default_transport() -> String {
    "stdio".into()
}
impl Config {
    pub fn native_path(&self, path: &Path) -> Result<PathBuf> {
        ensure!(
            path.is_absolute()
                && !path.components().any(|c| matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )),
            "invalid_native_execution_path"
        );
        if self.native_transport == "stdio" {
            return Ok(path.to_path_buf());
        }
        let (local, native) = self
            .native_paths
            .iter()
            .find(|(local, _)| path.starts_with(local))
            .context("native_execution_path_unmapped")?;
        Ok(native.join(path.strip_prefix(local)?))
    }
    pub fn validate_execution_paths(&self, root: &Path) -> Result<()> {
        self.native_path(root)?;
        self.native_path(&self.skill_path)?;
        self.native_path(&std::env::current_exe()?)?;
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).context("read_operator_config")?;
        ensure!(bytes.len() <= 65536, "configuration_too_large");
        let config: Self = serde_json::from_slice(&bytes).context("invalid_operator_config")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        match &self.published_origin {
            Some(origin) => {
                ensure!(
                    self.listen.ip().is_unspecified(),
                    "published_origin_requires_unspecified_bind"
                );
                ensure!(valid_published_origin(origin), "invalid_published_origin");
            }
            None => ensure!(self.listen.ip().is_loopback(), "loopback_only"),
        }
        if let Some(github_app) = &self.github_app {
            github_app.validate()?;
        }
        ensure!(self.cas_targets.len() <= 8, "too_many_cas_targets");
        for (alias, target) in &self.cas_targets {
            ensure!(valid_alias(alias), "invalid_cas_alias");
            target.validate()?;
        }
        ensure!(self.cas_timelines.len() <= 64, "too_many_cas_timelines");
        for (repository, alias) in &self.cas_timelines {
            ensure!(valid_alias(repository), "invalid_cas_timeline_repository");
            ensure!(
                self.cas_targets.contains_key(alias),
                "cas_timeline_target_not_configured"
            );
        }
        ensure!(
            ["stdio", "existing_daemon"].contains(&self.native_transport.as_str()),
            "unsupported_native_transport"
        );
        if let Some(socket) = &self.native_socket {
            ensure!(
                socket.is_absolute() && self.native_transport == "existing_daemon",
                "invalid_native_socket"
            );
        }
        ensure!(
            self.native_paths.len() <= 16,
            "too_many_native_path_mappings"
        );
        for (local, native) in &self.native_paths {
            for path in [local, native] {
                ensure!(
                    path.is_absolute()
                        && path.parent().is_some()
                        && !path.components().any(|c| matches!(
                            c,
                            std::path::Component::ParentDir | std::path::Component::CurDir
                        )),
                    "invalid_native_path_mapping"
                );
            }
            ensure!(
                self.native_paths.keys().all(|other| other == local
                    || (!local.starts_with(other) && !other.starts_with(local))),
                "overlapping_native_path_mappings"
            );
            ensure!(
                canonical_destination(local)? == *local,
                "native_path_mapping_symlink"
            );
        }
        ensure!(
            self.limits.capacity > 0 && self.limits.capacity <= 8,
            "invalid_capacity_limit"
        );
        ensure!(self.limits.repair_attempts <= 10, "invalid_repair_limit");
        ensure!(
            (30..=86400).contains(&self.limits.wall_seconds),
            "invalid_wall_limit"
        );
        let database = canonical_destination(&self.database)?;
        if let Some(workspaces) = &self.workspaces {
            workspaces.validate(&database)?;
        }
        ensure!(self.discovery_roots.len() <= 8, "too_many_discovery_roots");
        for (alias, repo) in self.repositories.iter().chain(self.discovery_roots.iter()) {
            ensure!(valid_alias(alias), "invalid_repository_alias");
            let root = repo
                .root
                .canonicalize()
                .context("repository_root_unavailable")?;
            ensure!(
                root == repo.root && root.is_absolute(),
                "repository_symlink_or_traversal_rejected"
            );
            ensure!(
                !database.starts_with(&root),
                crate::qualification::STORE_EXPOSED
            );
            finish_rank(&repo.max_finish)?;
            ensure!(root.parent().is_some(), "filesystem_root_not_allowed");
        }
        for (alias, profile) in &self.profiles {
            ensure!(valid_alias(alias), "invalid_profile_alias");
            ensure!(
                ["low", "medium", "high", "xhigh", "max"].contains(&profile.effort.as_str()),
                "invalid_luna_effort"
            );
            if let Some(name) = &profile.codex_profile {
                ensure!(valid_alias(name), "invalid_codex_profile");
            }
        }
        Ok(())
    }
}

pub(crate) fn valid_published_origin(origin: &str) -> bool {
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return false;
    }
    let Ok(address) = authority.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let Ok(ip) = address.host().parse::<std::net::IpAddr>() else {
        return false;
    };
    address.port_u16().is_some_and(|port| port != 0) && ip.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::Config;
    use serde_json::json;

    #[test]
    fn container_bind_requires_an_explicit_loopback_published_origin() {
        let config: Config = serde_json::from_value(json!({
            "listen": "0.0.0.0:8787",
            "published_origin": "http://127.0.0.1:18788",
            "database": "/tmp/luna-factory-container-test/runs.sqlite",
            "codex_binary": "/usr/bin/false",
            "skill_path": "/opt/luna-factory/skills/luna-factory/SKILL.md",
            "repositories": {},
            "profiles": {},
            "limits": {"capacity": 1, "repair_attempts": 0, "wall_seconds": 300}
        }))
        .expect("valid loopback-published container config should deserialize");

        assert!(super::valid_published_origin("http://127.0.0.1:18788"));
        config
            .validate()
            .expect("valid loopback-published container config should validate");
    }

    #[test]
    fn container_publish_rejects_non_loopback_and_path_origins() {
        for origin in [
            "http://example.com:18788",
            "https://127.0.0.1:18788",
            "http://127.0.0.1:0",
            "http://127.0.0.1:18788/mcp",
            "http://127.0.0.1:18788?x=1",
        ] {
            assert!(!super::valid_published_origin(origin), "{origin}");
        }
    }

    #[test]
    fn non_loopback_bind_without_publish_origin_stays_rejected() {
        let config: Config = serde_json::from_value(json!({
            "listen": "0.0.0.0:8787",
            "database": "/tmp/luna-factory-container-test/runs.sqlite",
            "codex_binary": "/usr/bin/false",
            "skill_path": "/opt/luna-factory/skills/luna-factory/SKILL.md",
            "repositories": {},
            "profiles": {},
            "limits": {"capacity": 1, "repair_attempts": 0, "wall_seconds": 300}
        }))
        .expect("config without optional publish origin should deserialize");

        assert!(config.validate().is_err());
    }
}
