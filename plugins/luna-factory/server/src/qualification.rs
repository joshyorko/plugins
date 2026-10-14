//! Persisted exact-profile execution qualification (#71).
//!
//! A qualification is an operator-recorded grant bound to the sha256 fingerprint of one
//! exact execution profile: factory version, native transport, Codex binary identity,
//! native socket, Luna model, profile alias and effort. Grants are written only by the
//! local operator CLI; there is no MCP write path. Every native effect path checks the
//! grant before writing intent, and the pre-wire dispatch guard re-checks it through a
//! fresh read-only SQLite connection so a revocation by another process takes effect
//! before the next byte. Missing, unreadable or malformed state always denies.
use crate::config::Config;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub const NOT_QUALIFIED: &str = "execution_not_qualified";
pub const PROFILE_MISMATCH: &str = "execution_profile_mismatch";
pub const REVOKED: &str = "execution_qualification_revoked";
pub const EXPIRED: &str = "execution_qualification_expired";
pub const SCOPE: &str = "execution_qualification_scope";
pub const CAPACITY: &str = "execution_capacity_not_qualified";
/// The evidence producer an ordinary grant requires. The canary harness that emits it is
/// not implemented yet; see docs/execution-qualification.md.
pub const HARNESS: &str = "luna-factory.qualification-canary.v1";
pub const UNAVAILABLE: &str = "unavailable";

/// Additive table. `user_version` is unchanged so older binaries still open the DB;
/// they never read this table and therefore never execute on its behalf.
pub const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS execution_qualifications (fingerprint TEXT PRIMARY KEY, profile_alias TEXT NOT NULL, payload TEXT NOT NULL);";

const MAX_EVIDENCE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfile {
    pub factory_version: String,
    pub transport: String,
    pub codex_binary: String,
    pub codex_binary_identity: String,
    pub native_socket: Option<String>,
    pub model: String,
    pub profile: String,
    pub effort: String,
}

impl ExecutionProfile {
    pub fn from_config(config: &Config, alias: &str) -> Result<Self> {
        let profile = config
            .profiles
            .get(alias)
            .context("qualification_profile_unknown")?;
        let absolute =
            std::path::absolute(&config.codex_binary).context("codex_binary_path_invalid")?;
        let binary = absolute.canonicalize().unwrap_or(absolute);
        Ok(Self {
            factory_version: env!("CARGO_PKG_VERSION").into(),
            transport: config.native_transport.clone(),
            codex_binary: binary.to_string_lossy().into_owned(),
            codex_binary_identity: binary_identity(&binary),
            native_socket: config
                .native_socket
                .as_ref()
                .map(|socket| socket.to_string_lossy().into_owned()),
            model: crate::native::LUNA_MODEL.into(),
            profile: alias.into(),
            effort: profile.effort.clone(),
        })
    }
    /// Sorted-key JSON; the fingerprint input.
    pub fn canonical_json(&self) -> String {
        let value = serde_json::to_value(self).expect("profile serializes");
        let sorted: BTreeMap<String, Value> = value
            .as_object()
            .expect("profile object")
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        serde_json::to_string(&sorted).expect("profile serializes")
    }
    pub fn fingerprint(&self) -> String {
        format!("{:x}", Sha256::digest(self.canonical_json().as_bytes()))
    }
}

/// File size plus modification time. A replaced or rebuilt binary changes the profile.
fn binary_identity(path: &Path) -> String {
    let Ok(metadata) = std::fs::metadata(path) else {
        return UNAVAILABLE.into();
    };
    let Some(modified) = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
    else {
        return UNAVAILABLE.into();
    };
    if !metadata.is_file() {
        return UNAVAILABLE.into();
    }
    format!(
        "size={};mtime={}.{:09}",
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// May authorize ordinary Factory runs.
    Ordinary,
    /// Authorizes only the operator canary harness; never ordinary runs.
    QualificationOnly,
}
impl Scope {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "ordinary" => Ok(Self::Ordinary),
            "qualification_only" => Ok(Self::QualificationOnly),
            _ => anyhow::bail!("invalid_qualification_scope"),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::QualificationOnly => "qualification_only",
        }
    }
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Qualification {
    pub profile: ExecutionProfile,
    pub scope: Scope,
    #[serde(default = "one")]
    pub max_capacity: u32,
    pub evidence_sha256: String,
    pub evidence_ref: String,
    pub recorded_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Qualified,
    Denied(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evaluation {
    pub status: Status,
    /// Present only for a current grant of the requested scope.
    pub max_capacity: Option<u32>,
}
impl Evaluation {
    fn denied(code: &'static str) -> Self {
        Self {
            status: Status::Denied(code),
            max_capacity: None,
        }
    }
}

/// Typed refusal. Display is the stable code only, so MCP errors stay exact and path-free.
/// Returned from a dispatch guard it proves the guard refused before any byte was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionDenied {
    pub code: &'static str,
}
impl std::fmt::Display for ExecutionDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for ExecutionDenied {}

pub fn denied_code(error: &anyhow::Error) -> Option<&'static str> {
    error
        .downcast_ref::<ExecutionDenied>()
        .map(|denied| denied.code)
}
/// Honest run blocker for a refusal that happened before any native send.
pub fn blocker(code: &str) -> String {
    format!("execution_not_qualified: {code}; nothing was dispatched")
}

fn load(connection: &Connection, fingerprint: &str) -> Result<Option<Qualification>> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload FROM execution_qualifications WHERE fingerprint=?1",
            [fingerprint],
            |row| row.get(0),
        )
        .optional()?;
    payload
        .map(|payload| serde_json::from_str(&payload).context("qualification_payload_invalid"))
        .transpose()
}

pub fn evaluate(
    connection: &Connection,
    config: &Config,
    alias: &str,
    scope: Scope,
    capacity: u32,
    now: u64,
) -> Evaluation {
    let Ok(profile) = ExecutionProfile::from_config(config, alias) else {
        return Evaluation::denied(NOT_QUALIFIED);
    };
    let fingerprint = profile.fingerprint();
    match load(connection, &fingerprint) {
        // Missing table, unreadable or malformed state: fail closed.
        Err(_) => Evaluation::denied(NOT_QUALIFIED),
        Ok(Some(record)) => {
            if record.profile != profile {
                Evaluation::denied(PROFILE_MISMATCH)
            } else if record.revoked_at.is_some() {
                Evaluation::denied(REVOKED)
            } else if record.expires_at.is_some_and(|expires| expires <= now) {
                Evaluation::denied(EXPIRED)
            } else if record.scope != scope {
                Evaluation::denied(SCOPE)
            } else if capacity == 0 || capacity > record.max_capacity {
                Evaluation::denied(CAPACITY)
            } else {
                Evaluation {
                    status: Status::Qualified,
                    max_capacity: Some(record.max_capacity),
                }
            }
        }
        Ok(None) => {
            let other = connection
                .query_row(
                    "SELECT 1 FROM execution_qualifications WHERE profile_alias=?1 LIMIT 1",
                    [alias],
                    |_| Ok(()),
                )
                .optional();
            match other {
                Ok(Some(())) => Evaluation::denied(PROFILE_MISMATCH),
                _ => Evaluation::denied(NOT_QUALIFIED),
            }
        }
    }
}

/// Fresh read-only connection; safe inside synchronous pre-wire guards.
pub fn open_read_only(database: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(connection)
}

pub fn check(config: &Config, alias: &str, scope: Scope, capacity: u32) -> Evaluation {
    match open_read_only(&config.database) {
        Ok(connection) => evaluate(
            &connection,
            config,
            alias,
            scope,
            capacity,
            crate::store::now(),
        ),
        Err(_) => Evaluation::denied(NOT_QUALIFIED),
    }
}

/// Ordinary-run gate: `Err(ExecutionDenied)` unless a current ordinary grant covers
/// this exact profile and capacity.
pub fn require(config: &Config, alias: &str, capacity: u32) -> Result<()> {
    require_scope(config, alias, Scope::Ordinary, capacity)
}
pub fn require_scope(config: &Config, alias: &str, scope: Scope, capacity: u32) -> Result<()> {
    match check(config, alias, scope, capacity).status {
        Status::Qualified => Ok(()),
        Status::Denied(code) => Err(ExecutionDenied { code }.into()),
    }
}

/// Bounded, path-free per-profile status for capabilities.
pub fn capability(config: &Config) -> Value {
    let connection = open_read_only(&config.database).ok();
    let now = crate::store::now();
    let profiles = config
        .profiles
        .keys()
        .map(|alias| {
            let evaluation = match &connection {
                Some(connection) => evaluate(connection, config, alias, Scope::Ordinary, 1, now),
                None => Evaluation::denied(NOT_QUALIFIED),
            };
            let (status, reason) = match evaluation.status {
                Status::Qualified => ("qualified", "execution_qualified"),
                Status::Denied(code) => ("denied", code),
            };
            json!({"alias":alias,"status":status,"reason":reason,"max_capacity":evaluation.max_capacity})
        })
        .collect::<Vec<_>>();
    let eligible = profiles.iter().any(|p| p["status"] == "qualified");
    let reason = if eligible {
        "execution_qualified".to_string()
    } else if config.profiles.is_empty() {
        NOT_QUALIFIED.to_string()
    } else {
        profiles[0]["reason"]
            .as_str()
            .unwrap_or(NOT_QUALIFIED)
            .to_string()
    };
    json!({"eligible":eligible,"reason":reason,"profiles":profiles})
}

fn open_write(config: &Config) -> Result<Connection> {
    // Store::open validates the config, enforces private permissions and the schema.
    drop(crate::store::Store::open(config)?);
    let connection = Connection::open(&config.database)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(TABLE_SQL)?;
    Ok(connection)
}

/// The evidence contract the canary harness must emit for an ordinary grant.
fn validate_passed_run(run: &Value) -> Result<()> {
    ensure!(
        run["harness"].as_str() == Some(HARNESS),
        "qualification_evidence_harness_unrecognized"
    );
    ensure!(
        run["outcome"].as_str() == Some("passed"),
        "qualification_evidence_not_passed"
    );
    let started = run["started_at"].as_u64();
    let finished = run["finished_at"].as_u64();
    ensure!(
        started.zip(finished).is_some_and(|(s, f)| s <= f),
        "qualification_evidence_timing_invalid"
    );
    let checks = run["checks"]
        .as_array()
        .context("qualification_evidence_checks_missing")?;
    ensure!(
        !checks.is_empty()
            && checks.len() <= 64
            && checks.iter().all(|check| check["passed"] == true),
        "qualification_evidence_checks_not_passed"
    );
    ensure!(
        run["cessation"]["verified"] == true,
        "qualification_evidence_cessation_unverified"
    );
    Ok(())
}

pub struct RecordRequest<'a> {
    pub alias: &'a str,
    pub evidence: &'a Path,
    pub scope: Scope,
    pub max_capacity: u32,
    pub expires_in_days: Option<u64>,
}

/// Operator-only. Never reachable through MCP.
pub fn record(config: &Config, request: RecordRequest<'_>) -> Result<Value> {
    ensure!(
        (1..=8).contains(&request.max_capacity),
        "invalid_qualification_capacity"
    );
    if let Some(days) = request.expires_in_days {
        ensure!((1..=365).contains(&days), "invalid_qualification_expiry");
    }
    let settings = config
        .profiles
        .get(request.alias)
        .context("qualification_profile_unknown")?;
    ensure!(
        settings.codex_profile.is_none(),
        "native_profile_override_unsupported"
    );
    let profile = ExecutionProfile::from_config(config, request.alias)?;
    ensure!(
        profile.codex_binary_identity != UNAVAILABLE
            || config.native_transport == "existing_daemon",
        "codex_binary_identity_unavailable"
    );
    let fingerprint = profile.fingerprint();
    let evidence_path = std::path::absolute(request.evidence)
        .context("qualification_evidence_missing")?
        .canonicalize()
        .context("qualification_evidence_missing")?;
    let metadata = std::fs::metadata(&evidence_path).context("qualification_evidence_missing")?;
    ensure!(metadata.is_file(), "qualification_evidence_missing");
    ensure!(
        metadata.len() <= MAX_EVIDENCE_BYTES,
        "qualification_evidence_too_large"
    );
    let bytes = std::fs::read(&evidence_path).context("qualification_evidence_missing")?;
    let evidence: Value =
        serde_json::from_slice(&bytes).context("qualification_evidence_invalid")?;
    ensure!(
        evidence["profile_fingerprint"].as_str() == Some(fingerprint.as_str()),
        "qualification_evidence_fingerprint_mismatch"
    );
    if request.scope == Scope::Ordinary {
        validate_passed_run(&evidence["qualification_run"])?;
    }
    let recorded_at = crate::store::now();
    let record = Qualification {
        profile,
        scope: request.scope,
        max_capacity: request.max_capacity,
        evidence_sha256: format!("{:x}", Sha256::digest(&bytes)),
        evidence_ref: evidence_path.to_string_lossy().into_owned(),
        recorded_at,
        expires_at: request
            .expires_in_days
            .map(|days| recorded_at + days * 86400),
        revoked_at: None,
    };
    let connection = open_write(config)?;
    connection.execute(
        "INSERT INTO execution_qualifications(fingerprint,profile_alias,payload) VALUES (?1,?2,?3)
         ON CONFLICT(fingerprint) DO UPDATE SET profile_alias=excluded.profile_alias,payload=excluded.payload",
        params![fingerprint, request.alias, serde_json::to_string(&record)?],
    )?;
    Ok(public(&fingerprint, &record, Some(config)))
}

pub fn revoke(config: &Config, fingerprint: &str) -> Result<Value> {
    ensure!(
        fingerprint.len() == 64 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid_qualification_fingerprint"
    );
    let mut connection = open_write(config)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut record = load(&tx, fingerprint)?.context("qualification_not_found")?;
    if record.revoked_at.is_none() {
        record.revoked_at = Some(crate::store::now());
        tx.execute(
            "UPDATE execution_qualifications SET payload=?2 WHERE fingerprint=?1",
            params![fingerprint, serde_json::to_string(&record)?],
        )?;
    }
    tx.commit()?;
    Ok(public(fingerprint, &record, Some(config)))
}

pub fn list(config: &Config) -> Result<Vec<Value>> {
    let connection = open_write(config)?;
    let mut statement = connection.prepare(
        "SELECT fingerprint,payload FROM execution_qualifications ORDER BY profile_alias,fingerprint LIMIT 256",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(
            |(fingerprint, payload)| match serde_json::from_str::<Qualification>(&payload) {
                Ok(record) => public(&fingerprint, &record, Some(config)),
                Err(_) => json!({"fingerprint":fingerprint,"status":"unreadable"}),
            },
        )
        .collect())
}

/// Bounded operator view. Evidence content is never echoed; only its digest and path.
fn public(fingerprint: &str, record: &Qualification, config: Option<&Config>) -> Value {
    let now = crate::store::now();
    let current = config
        .and_then(|config| ExecutionProfile::from_config(config, &record.profile.profile).ok())
        .is_some_and(|profile| profile == record.profile);
    let status = if record.revoked_at.is_some() {
        "revoked"
    } else if record.expires_at.is_some_and(|expires| expires <= now) {
        "expired"
    } else if !current {
        "profile_changed"
    } else {
        "current"
    };
    json!({"fingerprint":fingerprint,"profile_alias":record.profile.profile,"status":status,
        "scope":record.scope.as_str(),"max_capacity":record.max_capacity,
        "transport":record.profile.transport,"model":record.profile.model,"effort":record.profile.effort,
        "factory_version":record.profile.factory_version,
        "evidence_sha256":record.evidence_sha256,"evidence_ref":record.evidence_ref,
        "recorded_at":record.recorded_at,"expires_at":record.expires_at,"revoked_at":record.revoked_at})
}

/// Canary preconditions. Requires a current `qualification_only` grant for this exact
/// profile and a disposable git repository distinct from every configured root. Uses no
/// repository alias and admits nothing.
pub fn canary_preconditions(config: &Config, alias: &str, workspace: &Path) -> Result<Value> {
    require_scope(config, alias, Scope::QualificationOnly, 1)?;
    ensure!(workspace.is_absolute(), "canary_workspace_must_be_absolute");
    let root = workspace
        .canonicalize()
        .context("canary_workspace_unavailable")?;
    ensure!(
        root == workspace,
        "canary_workspace_symlink_or_traversal_rejected"
    );
    ensure!(root.parent().is_some(), "canary_workspace_filesystem_root");
    let top = crate::store::git(&root, &["rev-parse", "--show-toplevel"])
        .context("canary_workspace_not_git_repository")?;
    ensure!(
        Path::new(&top).canonicalize()? == root,
        "canary_workspace_not_repository_root"
    );
    ensure!(
        crate::store::git(&root, &["remote"])?.is_empty(),
        "canary_workspace_has_remotes"
    );
    let common = |path: &Path| -> Result<std::path::PathBuf> {
        let identity = crate::store::git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        Ok(Path::new(&identity).canonicalize()?)
    };
    let identity = common(&root)?;
    for repository in config
        .repositories
        .values()
        .chain(config.discovery_roots.values())
    {
        let configured = repository
            .root
            .canonicalize()
            .unwrap_or(repository.root.clone());
        ensure!(
            !root.starts_with(&configured) && !configured.starts_with(&root),
            "canary_workspace_overlaps_configured_repository"
        );
        if let Ok(other) = common(&configured) {
            ensure!(
                other != identity,
                "canary_workspace_shares_repository_identity"
            );
        }
    }
    let database = crate::config::canonical_destination(&config.database)?;
    ensure!(
        !database.starts_with(&root),
        "canary_workspace_contains_database"
    );
    let fingerprint = ExecutionProfile::from_config(config, alias)?.fingerprint();
    Ok(json!({"profile_fingerprint":fingerprint,"preconditions":"passed"}))
}
