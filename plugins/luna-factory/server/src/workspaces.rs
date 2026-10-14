//! Operator-opt-in repository workspaces: connect a GitHub repository visible to
//! Luna's GitHub App, or create a new local repository, under one operator root.
//!
//! Turning a flag on in `operator.json` is the operator's approval. A repository
//! connected or created this way is an approved alias with recorded provenance
//! (`github_app_installation` or `local_create`) and the lowest finish authority.
//! Nothing here pushes, branches, opens a pull request, starts a run, dispatches
//! native work or qualifies execution.
use crate::{
    config::{Config, Repository},
    repositories::{Approval, Candidate, Registration},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

/// Not a valid alias, so it can never collide with an operator discovery root.
pub const ROOT_ALIAS: &str = "@workspaces";
/// Connected and created repositories never exceed local candidates.
pub const MAX_FINISH: &str = "local_candidate";
pub const MAX_WORKSPACE_REPOSITORIES: usize = 100;
const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workspaces {
    /// Absolute, canonical directory that receives clones and new repositories.
    pub root: PathBuf,
    #[serde(default)]
    pub connect_github: bool,
    #[serde(default)]
    pub create_local: bool,
    /// Optional repository-size ceiling, defaulting to two GiB.
    #[serde(default)]
    pub max_clone_bytes: Option<u64>,
    #[serde(skip)]
    root_identity: Arc<OnceLock<RootIdentity>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RootIdentity {
    canonical: PathBuf,
    device: u64,
    inode: u64,
}
pub const DEFAULT_MAX_CLONE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MIN_MAX_CLONE_BYTES: u64 = 1024 * 1024;
pub const MAX_MAX_CLONE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
impl Workspaces {
    pub fn validate(&self, database: &Path) -> Result<()> {
        ensure!(self.root.is_absolute(), "invalid_workspace_root");
        let root = self
            .root
            .canonicalize()
            .context("workspace_root_unavailable")?;
        ensure!(
            root == self.root,
            "workspace_root_symlink_or_traversal_rejected"
        );
        ensure!(
            std::fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir()),
            "workspace_root_not_a_directory"
        );
        ensure!(root.parent().is_some(), "filesystem_root_not_allowed");
        ensure!(
            !database.starts_with(&root),
            "database_must_be_outside_workspace_root"
        );
        let metadata = std::fs::symlink_metadata(&root)?;
        let identity = RootIdentity {
            canonical: root,
            device: metadata_device(&metadata),
            inode: metadata_inode(&metadata),
        };
        let _ = self.root_identity.set(identity);
        if let Some(maximum) = self.max_clone_bytes {
            ensure!(
                (MIN_MAX_CLONE_BYTES..=MAX_MAX_CLONE_BYTES).contains(&maximum),
                "invalid_max_clone_bytes"
            );
        }
        Ok(())
    }
    /// Detect root replacement and symlink changes immediately before filesystem writes.
    pub fn check_root(&self) -> Result<()> {
        let expected = self
            .root_identity
            .get()
            .context("workspace_root_identity_unavailable")?;
        let metadata = std::fs::symlink_metadata(&self.root).context("workspace_root_changed")?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "workspace_root_changed"
        );
        let canonical = self.root.canonicalize().context("workspace_root_changed")?;
        ensure!(
            canonical == expected.canonical
                && metadata_device(&metadata) == expected.device
                && metadata_inode(&metadata) == expected.inode,
            "workspace_root_changed"
        );
        Ok(())
    }
    pub fn max_clone_bytes(&self) -> u64 {
        self.max_clone_bytes.unwrap_or(DEFAULT_MAX_CLONE_BYTES)
    }
    /// The flag that approved repositories of this provenance. Turning it off
    /// withdraws that approval as well as stopping new connects or creates.
    pub fn enabled(&self, provenance: Provenance) -> bool {
        match provenance {
            Provenance::GithubAppInstallation => self.connect_github,
            Provenance::LocalCreate => self.create_local,
        }
    }
    pub fn policy(&self) -> Repository {
        Repository {
            root: self.root.clone(),
            max_finish: MAX_FINISH.into(),
        }
    }
}
#[cfg(unix)]
fn metadata_device(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}
#[cfg(not(unix))]
fn metadata_device(_: &std::fs::Metadata) -> u64 {
    0
}
#[cfg(unix)]
fn metadata_inode(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}
#[cfg(not(unix))]
fn metadata_inode(_: &std::fs::Metadata) -> u64 {
    0
}
/// `capabilities.workspaces`: flags only, never a path.
pub fn capability(config: &Config) -> Value {
    let workspaces = config.workspaces.as_ref();
    json!({
        "connect_github":workspaces.is_some_and(|w| w.connect_github),
        "create_local":workspaces.is_some_and(|w| w.create_local)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    GithubAppInstallation,
    LocalCreate,
}

/// The durable record. The directory is relative to the workspace root and is
/// never projected; tools see only the alias and the opaque repository identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRepository {
    pub id: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub created_by_luna: bool,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub device: Option<u64>,
    #[serde(default)]
    pub inode: Option<u64>,
    pub provenance: Provenance,
    pub alias: String,
    pub directory: String,
    pub candidate_id: String,
    /// Lowercase `owner/repo` for a connected GitHub repository.
    pub full_name: Option<String>,
    /// The requested name for a created local repository.
    pub name: Option<String>,
    pub default_branch: Option<String>,
    pub identity: String,
    pub created_at: u64,
}
impl WorkspaceRepository {
    /// An approved registration that resolves only under the workspace root and
    /// only while the flag for its provenance stays on.
    pub fn registration(&self) -> Registration {
        Registration {
            id: self.id.clone(),
            alias: self.alias.clone(),
            candidate_id: self.candidate_id.clone(),
            root_alias: ROOT_ALIAS.into(),
            relative: self.directory.clone(),
            max_finish: MAX_FINISH.into(),
            status: Approval::Approved,
            provenance: Some(self.provenance),
        }
    }
    pub fn connect_result(&self, status: &str) -> Value {
        json!({"schema_version":1,"status":status,"alias":self.alias,
            "repository":{"alias":self.alias,"identity":self.identity},
            "full_name":self.full_name,"default_branch":self.default_branch,"provenance":"github_app"})
    }
    pub fn create_result(&self, status: &str) -> Value {
        json!({"schema_version":1,"status":status,"alias":self.alias,
            "repository":{"alias":self.alias,"identity":self.identity}})
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListGithubRepositories {
    #[serde(default)]
    pub cursor: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectGithubRepository {
    pub repository: String,
    pub idempotency_key: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLocalRepository {
    pub name: String,
    pub idempotency_key: String,
}

pub fn valid_idempotency_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
/// `^[a-z0-9][a-z0-9._-]{0,63}$`.
pub fn valid_local_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}
/// `<owner>__<repo>`, lowercase.
pub fn github_directory(repository: &crate::github::RepoName) -> String {
    format!(
        "{}__{}",
        repository.owner.to_ascii_lowercase(),
        repository.name.to_ascii_lowercase()
    )
}
/// Lowercase, with anything outside `[a-z0-9_-]` mapped to `-`, at most 64 bytes.
pub fn alias_from(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect()
}

/// The discovery candidate for a workspace directory: a plain, non-symlinked
/// directory directly under the root with its own `.git` directory.
pub fn candidate(workspaces: &Workspaces, directory: &str) -> Result<Option<Candidate>> {
    ensure!(
        !directory.is_empty() && !directory.contains(['/', '\\']) && !directory.starts_with('.'),
        "invalid_workspace_directory"
    );
    crate::repositories::candidate(ROOT_ALIAS, &workspaces.policy(), Path::new(directory))
}

/// `git` with no user or system configuration, no prompts, no hooks and only
/// the environment it needs. Proxy and CA variables pass through so an operator
/// egress proxy (GitHub App Option A) keeps working.
pub(crate) fn git_command() -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command.env_clear();
    for name in [
        "PATH",
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "commit.gpgsign=false",
        ]);
    command
}
async fn run_local_git(mut command: std::process::Command) -> Result<()> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let status = tokio::time::timeout(LOCAL_GIT_TIMEOUT, command.status())
        .await
        .map_err(|_| anyhow::anyhow!("local_repository_create_timeout"))?
        .context("git_unavailable")?;
    ensure!(status.success(), "local_repository_create_failed");
    Ok(())
}
/// `git init` on `main` plus one empty initial commit. No remote, no GitHub call.
pub async fn init_local(target: &Path) -> Result<()> {
    let mut init = git_command();
    init.args(["init", "--quiet", "--template="]).arg(target);
    run_local_git(init).await?;
    let mut head = git_command();
    head.arg("-C")
        .arg(target)
        .args(["symbolic-ref", "HEAD", "refs/heads/main"]);
    run_local_git(head).await?;
    let mut commit = git_command();
    commit.arg("-C").arg(target).args([
        "-c",
        "user.name=Luna Factory",
        "-c",
        "user.email=luna-factory@localhost.invalid",
        "commit",
        "--quiet",
        "--allow-empty",
        "--no-verify",
        "-m",
        "Initial commit",
    ]);
    run_local_git(commit).await
}

/// What sits at a workspace target path. Symlinks are never followed.
pub enum Target {
    Missing,
    Directory,
    Other,
}
pub fn target_state(path: &Path) -> Result<Target> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Ok(Target::Directory)
        }
        Ok(_) => Ok(Target::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Target::Missing),
        Err(_) => anyhow::bail!("workspace_root_unavailable"),
    }
}
/// A private staging directory inside the root, renamed into place on success.
pub fn staging_path(workspaces: &Workspaces, kind: &str) -> PathBuf {
    workspaces
        .root
        .join(format!(".luna-{kind}-{}", uuid::Uuid::new_v4().simple()))
}
/// Move a finished staging directory to its target, refusing to replace anything.
pub fn publish(staging: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().context("workspace_root_changed")?;
    ensure!(
        parent == staging.parent().context("workspace_staging_invalid")?,
        "workspace_staging_invalid"
    );
    let stage = std::fs::symlink_metadata(staging).context("workspace_staging_invalid")?;
    ensure!(
        stage.is_dir() && !stage.file_type().is_symlink(),
        "workspace_staging_invalid"
    );
    if !matches!(target_state(target)?, Target::Missing) {
        let _ = std::fs::remove_dir_all(staging);
        anyhow::bail!("workspace_path_conflict");
    }
    if std::fs::rename(staging, target).is_err() {
        let _ = std::fs::remove_dir_all(staging);
        anyhow::bail!("workspace_path_conflict");
    }
    Ok(())
}

/// Captures the staging inode so a retry can recover an atomic rename that
/// completed immediately before process termination.
pub fn directory_identity(path: &Path) -> Result<(u64, u64)> {
    let metadata = std::fs::symlink_metadata(path).context("workspace_staging_invalid")?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "workspace_staging_invalid"
    );
    Ok((metadata_device(&metadata), metadata_inode(&metadata)))
}

/// Confirm a repository is still the exact ordinary directory Luna recorded,
/// with only the local Git config Luna's init/clone commands produce.
pub fn verify_recorded_repository(
    path: &Path,
    record: &WorkspaceRepository,
    expected_origin: Option<&str>,
) -> Result<()> {
    ensure!(record.created_by_luna, "workspace_path_conflict");
    let metadata = std::fs::symlink_metadata(path).context("workspace_path_conflict")?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && record.device == Some(metadata_device(&metadata))
            && record.inode == Some(metadata_inode(&metadata)),
        "workspace_path_conflict"
    );
    let git_dir = path.join(".git");
    let git_meta = std::fs::symlink_metadata(&git_dir).context("workspace_path_conflict")?;
    ensure!(
        git_meta.is_dir() && !git_meta.file_type().is_symlink(),
        "workspace_path_conflict"
    );
    let hooks = git_dir.join("hooks");
    match std::fs::symlink_metadata(&hooks) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "workspace_path_conflict"
            );
            for entry in std::fs::read_dir(&hooks).context("workspace_path_conflict")? {
                let entry = entry.context("workspace_path_conflict")?;
                let metadata =
                    std::fs::symlink_metadata(entry.path()).context("workspace_path_conflict")?;
                ensure!(
                    entry.file_name().to_string_lossy().ends_with(".sample")
                        && metadata.is_file()
                        && !metadata.file_type().is_symlink(),
                    "workspace_path_conflict"
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => anyhow::bail!("workspace_path_conflict"),
    }
    let config_path = git_dir.join("config");
    let config_metadata =
        std::fs::symlink_metadata(&config_path).context("workspace_path_conflict")?;
    ensure!(
        config_metadata.is_file() && !config_metadata.file_type().is_symlink(),
        "workspace_path_conflict"
    );
    let config = std::fs::read_to_string(config_path).context("workspace_path_conflict")?;
    validate_workspace_git_config(&config, expected_origin)
}

fn validate_workspace_git_config(config: &str, expected_origin: Option<&str>) -> Result<()> {
    let mut section = String::new();
    let mut origin_url = false;
    let mut origin_fetch = false;
    let mut bare = false;
    let mut repository_format = false;
    for line in config.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            ensure!(
                section == "core"
                    || section == "remote \"origin\""
                    || (section.starts_with("branch \"") && section.ends_with('"')),
                "workspace_path_conflict"
            );
            continue;
        }
        let (key, value) = line.split_once('=').context("workspace_path_conflict")?;
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"');
        match (section.as_str(), key.as_str()) {
            ("core", "repositoryformatversion") if value == "0" => repository_format = true,
            ("core", "bare") if value.eq_ignore_ascii_case("false") => bare = true,
            ("core", "filemode") if matches!(value, "true" | "false") => {}
            ("core", "logallrefupdates") if matches!(value, "true" | "false") => {}
            ("remote \"origin\"", "url") if expected_origin == Some(value) => {
                ensure!(!origin_url, "workspace_path_conflict");
                origin_url = true;
            }
            ("remote \"origin\"", "fetch")
                if value.starts_with("+refs/heads/") && value.contains(":refs/remotes/origin/") =>
            {
                ensure!(!origin_fetch, "workspace_path_conflict");
                origin_fetch = true;
            }
            ("remote \"origin\"", "tagopt") if value == "--no-tags" => {}
            (branch, "remote") if branch.starts_with("branch \"") && value == "origin" => {}
            (branch, "merge")
                if branch.starts_with("branch \"") && value.starts_with("refs/heads/") => {}
            _ => anyhow::bail!("workspace_path_conflict"),
        }
    }
    ensure!(repository_format && bare, "workspace_path_conflict");
    match expected_origin {
        Some(_) => ensure!(origin_url && origin_fetch, "workspace_path_conflict"),
        None => ensure!(!origin_url && !origin_fetch, "workspace_path_conflict"),
    }
    Ok(())
}

/// Removes the private staging directory on errors, panics, and cancellation.
pub struct StagingGuard(PathBuf);
impl StagingGuard {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_aliases_and_directories_are_bounded() {
        for good in ["a", "0", "my-app", "x.y_z", &"a".repeat(64)] {
            assert!(valid_local_name(good), "{good}");
        }
        for bad in [
            "",
            "-a",
            ".a",
            "_a",
            "A",
            "a/b",
            "a b",
            &"a".repeat(65),
            "ä",
        ] {
            assert!(!valid_local_name(bad), "{bad}");
        }
        assert_eq!(alias_from("My.Repo_x"), "my-repo_x");
        assert!(crate::config::valid_alias(&alias_from(&"x.".repeat(80))));
        let repository = crate::github::RepoName::parse("JoshYorko/Plugins").unwrap();
        assert_eq!(github_directory(&repository), "joshyorko__plugins");
        assert!(valid_idempotency_key("k-1"));
        assert!(!valid_idempotency_key("") && !valid_idempotency_key("a\nb"));
        assert!(!crate::config::valid_alias(ROOT_ALIAS));
    }

    #[cfg(unix)]
    #[test]
    fn replaced_workspace_root_is_rejected_before_staging_or_publish() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let moved = temp.path().join("moved");
        let elsewhere = temp.path().join("elsewhere");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        let workspaces = Workspaces {
            root: root.clone(),
            connect_github: true,
            create_local: true,
            max_clone_bytes: None,
            root_identity: Arc::default(),
        };
        workspaces
            .validate(&temp.path().join("state.sqlite"))
            .unwrap();
        std::fs::rename(&root, &moved).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &root).unwrap();
        assert_eq!(
            workspaces.check_root().unwrap_err().to_string(),
            "workspace_root_changed"
        );
    }

    #[test]
    fn staging_guard_removes_unpublished_data_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let stage = temp.path().join(".luna-connect-test");
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("partial"), "partial clone").unwrap();
        drop(StagingGuard::new(stage.clone()));
        assert!(!stage.exists());
    }
}
