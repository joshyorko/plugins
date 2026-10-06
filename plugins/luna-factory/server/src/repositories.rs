//! Bounded discovery and local-only approval. Remote inputs never contain paths.
use crate::{
    config::{Config, Repository, finish_rank, valid_alias},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const MAX_REGISTRATIONS: usize = 100;
const MAX_ENTRIES: usize = 2048;
const MAX_DEPTH: usize = 4;

#[derive(Clone, Serialize)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub root_alias: String,
    pub max_finish: String,
    #[serde(skip)]
    pub root: PathBuf,
    #[serde(skip)]
    pub relative: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    Pending,
    Approved,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub id: String,
    pub alias: String,
    pub candidate_id: String,
    pub root_alias: String,
    pub relative: String,
    pub max_finish: String,
    pub status: Approval,
}
impl Registration {
    pub fn public(&self) -> Value {
        json!({"id":self.id,"alias":self.alias,"root_alias":self.root_alias,"name":self.relative,
               "max_finish":self.max_finish,"status":self.status})
    }
    pub fn same_request(&self, other: &Self) -> bool {
        self.alias == other.alias
            && self.candidate_id == other.candidate_id
            && self.max_finish == other.max_finish
            && self.root_alias == other.root_alias
            && self.relative == other.relative
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationRequest {
    pub candidate_id: String,
    pub alias: String,
    pub max_finish: String,
}

fn identity(hash: &mut Sha256, metadata: &fs::Metadata) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hash.update(metadata.dev().to_le_bytes());
        hash.update(metadata.ino().to_le_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        hash.update(metadata.creation_time().to_le_bytes());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (hash, metadata);
    }
}
fn candidate(alias: &str, policy: &Repository, relative: &Path) -> Result<Option<Candidate>> {
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Ok(None);
    }
    let root = policy.root.join(relative);
    // Every component must remain an ordinary directory inside the current root.
    if policy.root.canonicalize().ok().as_ref() != Some(&policy.root) {
        return Ok(None);
    }
    let mut current = policy.root.clone();
    for part in relative.components() {
        current.push(part);
        if !fs::symlink_metadata(&current).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        {
            return Ok(None);
        }
    }
    if root.canonicalize().ok().as_ref() != Some(&root) {
        return Ok(None);
    }
    let marker = root.join(".git");
    let Ok(metadata) = fs::symlink_metadata(&marker) else {
        return Ok(None);
    };
    // Linked worktree metadata outside a discovery root is deliberately excluded.
    // Explicit operator-configured worktree aliases keep their existing behavior.
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !fs::symlink_metadata(marker.join("HEAD"))
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        return Ok(None);
    }
    let name = relative.to_str().context("non_utf8_discovery_name")?;
    if name.is_empty() || name.len() > 256 || name.chars().any(|c| c.is_control()) {
        return Ok(None);
    }
    let mut hash = Sha256::new();
    for value in [
        alias.as_bytes(),
        policy.root.as_os_str().as_encoded_bytes(),
        policy.max_finish.as_bytes(),
        relative.as_os_str().as_encoded_bytes(),
    ] {
        hash.update(value);
        hash.update([0]);
    }
    identity(&mut hash, &fs::metadata(&policy.root)?);
    identity(&mut hash, &fs::metadata(&root)?);
    identity(&mut hash, &metadata);
    Ok(Some(Candidate {
        id: format!("{:x}", hash.finalize()),
        name: name.replace('\\', "/"),
        root_alias: alias.into(),
        max_finish: policy.max_finish.clone(),
        root,
        relative: name.into(),
    }))
}

pub fn discover(config: &Config) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    let mut entries = 0;
    for (alias, policy) in &config.discovery_roots {
        let mut pending = vec![(PathBuf::new(), 0)];
        while let Some((relative, depth)) = pending.pop() {
            let directory = policy.root.join(&relative);
            if directory.canonicalize().ok().as_ref() != Some(&directory) {
                continue;
            }
            if !relative.as_os_str().is_empty()
                && let Some(found) = candidate(alias, policy, &relative)?
            {
                ensure!(
                    candidates.len() < MAX_REGISTRATIONS,
                    "discovery_repository_limit"
                );
                candidates.push(found);
                continue;
            }
            if depth >= MAX_DEPTH {
                continue;
            }
            let mut children = Vec::new();
            for entry in fs::read_dir(directory).context("discovery_root_unavailable")? {
                entries += 1;
                ensure!(entries <= MAX_ENTRIES, "discovery_entry_limit");
                let entry = entry.context("discovery_entry_unavailable")?;
                let name = entry.file_name();
                if name.to_string_lossy().starts_with('.')
                    || matches!(name.to_str(), Some("node_modules" | "target"))
                {
                    continue;
                }
                if entry.file_type()?.is_dir() {
                    children.push(relative.join(name));
                }
            }
            children.sort();
            pending.extend(children.into_iter().rev().map(|path| (path, depth + 1)));
        }
    }
    Ok(candidates)
}
fn resolve(config: &Config, registration: &Registration) -> Result<Candidate> {
    let policy = config
        .discovery_roots
        .get(&registration.root_alias)
        .context("discovery_root_revoked")?;
    ensure!(
        finish_rank(&registration.max_finish)? <= finish_rank(&policy.max_finish)?,
        "repository_authority_exceeded"
    );
    let candidate = candidate(
        &registration.root_alias,
        policy,
        Path::new(&registration.relative),
    )?
    .context("repository_candidate_changed")?;
    ensure!(
        candidate.id == registration.candidate_id,
        "repository_candidate_changed"
    );
    Ok(candidate)
}
pub fn effective_config(config: &Config, registrations: &[Registration]) -> Config {
    let mut effective = config.clone();
    for registration in registrations
        .iter()
        .filter(|r| r.status == Approval::Approved)
    {
        if effective.repositories.contains_key(&registration.alias) {
            continue;
        }
        if let Ok(candidate) = resolve(config, registration) {
            effective.repositories.insert(
                registration.alias.clone(),
                Repository {
                    root: candidate.root,
                    max_finish: registration.max_finish.clone(),
                },
            );
        }
    }
    effective
}
pub fn request(config: &Config, store: &mut Store, input: RegistrationRequest) -> Result<Value> {
    ensure!(valid_alias(&input.alias), "invalid_repository_alias");
    ensure!(
        !config.repositories.contains_key(&input.alias),
        "repository_alias_already_configured"
    );
    ensure!(
        input.candidate_id.len() == 64 && input.candidate_id.bytes().all(|b| b.is_ascii_hexdigit()),
        "unknown_repository_candidate"
    );
    let found = discover(config)?
        .into_iter()
        .find(|c| c.id == input.candidate_id)
        .context("unknown_repository_candidate")?;
    ensure!(
        finish_rank(&input.max_finish)? <= finish_rank(&found.max_finish)?,
        "repository_authority_exceeded"
    );
    let registration = Registration {
        id: uuid::Uuid::new_v4().to_string(),
        alias: input.alias,
        candidate_id: found.id,
        root_alias: found.root_alias,
        relative: found.relative,
        max_finish: input.max_finish,
        status: Approval::Pending,
    };
    Ok(store.request_repository(&registration)?.public())
}
/// A local CLI entrypoint, deliberately absent from MCP and safe settings.
pub fn approve(config: &Config, request_id: &str) -> Result<Value> {
    config.validate()?;
    ensure!(valid_alias(request_id), "invalid_repository_request_id");
    let mut store = Store::open(config)?;
    let registration = store.repository_request(request_id)?;
    ensure!(
        !config.repositories.contains_key(&registration.alias),
        "repository_alias_already_configured"
    );
    resolve(config, &registration)?;
    Ok(store.approve_repository(&registration)?.public())
}
