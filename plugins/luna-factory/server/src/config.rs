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
    #[serde(default = "default_transport")]
    pub native_transport: String,
    #[serde(default)]
    pub native_socket: Option<PathBuf>,
    pub database: PathBuf,
    pub codex_binary: PathBuf,
    pub skill_path: PathBuf,
    pub repositories: BTreeMap<String, Repository>,
    #[serde(default)]
    pub discovery_roots: BTreeMap<String, Repository>,
    pub profiles: BTreeMap<String, Profile>,
    pub limits: Limits,
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
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).context("read_operator_config")?;
        ensure!(bytes.len() <= 65536, "configuration_too_large");
        let config: Self = serde_json::from_slice(&bytes).context("invalid_operator_config")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.listen.ip().is_loopback(), "loopback_only");
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
            self.limits.capacity > 0 && self.limits.capacity <= 8,
            "invalid_capacity_limit"
        );
        ensure!(self.limits.repair_attempts <= 10, "invalid_repair_limit");
        ensure!(
            (30..=86400).contains(&self.limits.wall_seconds),
            "invalid_wall_limit"
        );
        let database = canonical_destination(&self.database)?;
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
                "database_must_be_outside_repositories"
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
