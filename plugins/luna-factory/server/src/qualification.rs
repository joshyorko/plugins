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
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub const NOT_QUALIFIED: &str = "execution_not_qualified";
pub const PROFILE_MISMATCH: &str = "execution_profile_mismatch";
pub const REVOKED: &str = "execution_qualification_revoked";
pub const EXPIRED: &str = "execution_qualification_expired";
pub const SCOPE: &str = "execution_qualification_scope";
pub const CAPACITY: &str = "execution_capacity_not_qualified";
pub const PROVIDER_MISMATCH: &str = "qualification_provider_mismatch";
pub const STORE_EXPOSED: &str = "qualification_store_exposed";
pub const CLOCK_SKEW: &str = "qualification_clock_skew";
/// The evidence producer an ordinary grant requires. The canary harness that emits it is
/// not implemented yet; see docs/execution-qualification.md.
pub const HARNESS: &str = "luna-factory.qualification-canary.v1";
pub const UNAVAILABLE: &str = "unavailable";
const MAX_BINARY_HASH_BYTES: u64 = 512 * 1024 * 1024;
const CLOCK_SKEW_TOLERANCE_SECONDS: u64 = 60;

#[cfg(test)]
mod tests {
    use super::{CLOCK_SKEW, TABLE_SQL, advance_high_water, binary_identity, sandbox_indicated};
    use std::fs::File;

    #[test]
    fn binary_identity_hashes_contents_when_size_and_mtime_match() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::write(&first, b"codex-build-a").unwrap();
        let modified = std::fs::metadata(&first).unwrap().modified().unwrap();
        std::fs::write(&second, b"codex-build-b").unwrap();
        File::open(&second)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();

        let first_metadata = std::fs::metadata(&first).unwrap();
        let second_metadata = std::fs::metadata(&second).unwrap();
        assert_eq!(first_metadata.len(), second_metadata.len());
        assert_eq!(
            first_metadata.modified().unwrap(),
            second_metadata.modified().unwrap()
        );
        assert_ne!(binary_identity(&first), binary_identity(&second));
    }

    #[test]
    fn qualification_clock_rejects_rollbacks_over_sixty_seconds_and_keeps_high_water() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(TABLE_SQL).unwrap();
        advance_high_water(&connection, 1000).unwrap();
        advance_high_water(&connection, 940).unwrap();
        assert_eq!(
            advance_high_water(&connection, 939)
                .unwrap_err()
                .to_string(),
            CLOCK_SKEW
        );
        let high_water: u64 = connection
            .query_row(
                "SELECT high_water FROM qualification_clock WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(high_water, 1000);
    }

    #[test]
    fn sandbox_confirmation_rejects_codex_thread_and_network_markers() {
        for name in [
            "CODEX_SANDBOX",
            "CODEX_SANDBOX_NETWORK_DISABLED",
            "CODEX_THREAD_ID",
            "CODEX_SANDBOX_PROFILE",
        ] {
            assert!(sandbox_indicated(std::ffi::OsStr::new(name)), "{name}");
        }
        assert!(!sandbox_indicated(std::ffi::OsStr::new("CODEX_HOME")));
    }

    #[test]
    fn qualification_time_before_epoch_is_not_coerced_to_zero() {
        let before_epoch = UNIX_EPOCH - std::time::Duration::from_secs(1);
        assert_eq!(
            qualification_timestamp(before_epoch).unwrap_err().to_string(),
            CLOCK_SKEW
        );
    }
}

/// Additive table. `user_version` is unchanged so older binaries still open the DB;
/// they never read this table and therefore never execute on its behalf.
pub const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS execution_qualifications (fingerprint TEXT PRIMARY KEY, profile_alias TEXT NOT NULL, payload TEXT NOT NULL); CREATE TABLE IF NOT EXISTS qualification_clock (id INTEGER PRIMARY KEY CHECK(id=1), high_water INTEGER NOT NULL);";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeServerIdentity {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_hash: Option<String>,
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct BinaryCacheKey {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: u64,
    modified_nanos: u32,
}

fn binary_cache_key(metadata: &std::fs::Metadata) -> Option<BinaryCacheKey> {
    use std::os::unix::fs::MetadataExt;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(BinaryCacheKey {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_seconds: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

/// SHA-256 of the executable, cached only for an unchanged filesystem identity.
fn binary_identity(path: &Path) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return UNAVAILABLE.into();
    };
    let Ok(metadata) = file.metadata() else {
        return UNAVAILABLE.into();
    };
    if !metadata.is_file() || metadata.len() > MAX_BINARY_HASH_BYTES {
        return UNAVAILABLE.into();
    }
    let Some(key) = binary_cache_key(&metadata) else {
        return UNAVAILABLE.into();
    };
    static CACHE: OnceLock<Mutex<HashMap<BinaryCacheKey, String>>> = OnceLock::new();
    if let Some(identity) = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("binary identity cache lock")
        .get(&key)
        .cloned()
    {
        return identity;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take(MAX_BINARY_HASH_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 != key.size
    {
        return UNAVAILABLE.into();
    }
    let Ok(after) = std::fs::metadata(path) else {
        return UNAVAILABLE.into();
    };
    if binary_cache_key(&after).as_ref() != Some(&key) {
        return UNAVAILABLE.into();
    }
    let identity = format!("sha256={:x}", Sha256::digest(&bytes));
    CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("binary identity cache lock")
        .insert(key, identity.clone());
    identity
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_identity: Option<NativeServerIdentity>,
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
    pub recorded_by_uid: u32,
    pub tty_name: String,
}

/// The only authorization accepted by native effect methods. Its fields and constructor
/// remain private to this module; the server gets it through `dispatch_authorization`.
pub(crate) struct DispatchAuthorization {
    check: Arc<dyn Fn() -> Result<()> + Send + Sync>,
}

impl DispatchAuthorization {
    pub(crate) fn check(&self) -> Result<()> {
        (self.check)()
    }
}

pub(crate) fn dispatch_authorization(
    config: &Config,
    alias: &str,
    capacity: u32,
    provider_identity: Option<NativeServerIdentity>,
) -> DispatchAuthorization {
    dispatch_authorization_with(config, alias, capacity, provider_identity, || Ok(()))
}

pub(crate) fn dispatch_authorization_with(
    config: &Config,
    alias: &str,
    capacity: u32,
    provider_identity: Option<NativeServerIdentity>,
    before_qualification: impl Fn() -> Result<()> + Send + Sync + 'static,
) -> DispatchAuthorization {
    let config = config.clone();
    let alias = alias.to_owned();
    DispatchAuthorization {
        check: Arc::new({
            move || {
                before_qualification()?;
                require_with_identity(&config, &alias, capacity, provider_identity.as_ref())
            }
        }),
    }
}

#[cfg(debug_assertions)]
pub(crate) fn test_dispatch_authorization(
    check: impl Fn() -> Result<()> + Send + Sync + 'static,
) -> DispatchAuthorization {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    assert!(executable.rsplit_once('-').is_some_and(|(_, suffix)| {
        suffix.len() >= 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
    DispatchAuthorization {
        check: Arc::new(check),
    }
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

fn evaluate_with_identity(
    connection: &Connection,
    config: &Config,
    alias: &str,
    scope: Scope,
    capacity: u32,
    now: u64,
    provider_identity: Option<&NativeServerIdentity>,
    compare_provider_identity: bool,
) -> Evaluation {
    let Ok(profile) = ExecutionProfile::from_config(config, alias) else {
        return Evaluation::denied(NOT_QUALIFIED);
    };
    if profile.codex_binary_identity == UNAVAILABLE {
        return Evaluation::denied(PROVIDER_MISMATCH);
    }
    let fingerprint = profile.fingerprint();
    match load(connection, &fingerprint) {
        // Missing table, unreadable or malformed state: fail closed.
        Err(_) => Evaluation::denied(NOT_QUALIFIED),
        Ok(Some(record)) => {
            if record.profile != profile {
                Evaluation::denied(PROFILE_MISMATCH)
            } else if now < record.recorded_at {
                Evaluation::denied(CLOCK_SKEW)
            } else if config.native_transport == "existing_daemon"
                && (record.provider_identity.is_none()
                    || (compare_provider_identity
                        && record.provider_identity.as_ref() != provider_identity))
            {
                Evaluation::denied(PROVIDER_MISMATCH)
            } else if config.native_transport == "stdio" && record.provider_identity.is_some() {
                Evaluation::denied(PROVIDER_MISMATCH)
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

pub fn evaluate(
    connection: &Connection,
    config: &Config,
    alias: &str,
    scope: Scope,
    capacity: u32,
    now: u64,
) -> Evaluation {
    evaluate_with_identity(connection, config, alias, scope, capacity, now, None, true)
}

pub fn now_qualification() -> Result<u64> {
    qualification_timestamp(SystemTime::now())
}

fn qualification_timestamp(time: SystemTime) -> Result<u64> {
    time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| anyhow::anyhow!(CLOCK_SKEW))
}

pub(crate) fn validate_store_boundary(config: &Config, additional_roots: &[PathBuf]) -> Result<()> {
    let database = crate::config::canonical_destination(&config.database)
        .map_err(|_| anyhow::anyhow!(STORE_EXPOSED))?;
    let directory =
        crate::config::canonical_destination(config.database.parent().context(STORE_EXPOSED)?)
            .map_err(|_| anyhow::anyhow!(STORE_EXPOSED))?;
    let mut roots = config
        .repositories
        .values()
        .chain(config.discovery_roots.values())
        .map(|repository| repository.root.clone())
        .chain(
            config
                .cas_targets
                .values()
                .map(|target| PathBuf::from(&target.cwd)),
        )
        .chain(additional_roots.iter().cloned())
        .collect::<Vec<_>>();
    for variable in ["CODEX_HOME", "CODEX_WORKSPACES_ROOT"] {
        if let Some(path) = std::env::var_os(variable) {
            roots.push(PathBuf::from(path));
        }
    }
    for root in roots {
        let root = crate::config::canonical_destination(&root)
            .map_err(|_| anyhow::anyhow!(STORE_EXPOSED))?;
        ensure!(
            !database.starts_with(&root) && !directory.starts_with(&root),
            STORE_EXPOSED
        );
    }
    Ok(())
}

pub(crate) fn validate_store_boundary_from_connection(
    config: &Config,
    connection: &Connection,
) -> Result<()> {
    let has_registrations: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='repository_registrations')",
        [],
        |row| row.get(0),
    )?;
    let mut roots = Vec::new();
    if has_registrations {
        let mut statement =
            connection.prepare("SELECT payload FROM repository_registrations LIMIT 101")?;
        for payload in statement.query_map([], |row| row.get::<_, String>(0))? {
            let registration: crate::repositories::Registration = serde_json::from_str(&payload?)?;
            let repository = config
                .repositories
                .get(&registration.root_alias)
                .or_else(|| config.discovery_roots.get(&registration.root_alias))
                .context(STORE_EXPOSED)?;
            roots.push(repository.root.join(&registration.relative));
        }
        ensure!(
            roots.len() <= crate::repositories::MAX_REGISTRATIONS,
            STORE_EXPOSED
        );
    }
    validate_store_boundary(config, &roots)
}

fn advance_high_water(connection: &Connection, now: u64) -> Result<()> {
    let high_water: Option<u64> = connection
        .query_row(
            "SELECT high_water FROM qualification_clock WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(high_water) = high_water {
        ensure!(
            now >= high_water.saturating_sub(CLOCK_SKEW_TOLERANCE_SECONDS),
            CLOCK_SKEW
        );
        connection.execute(
            "UPDATE qualification_clock SET high_water=?1 WHERE id=1",
            [high_water.max(now)],
        )?;
    } else {
        connection.execute(
            "INSERT INTO qualification_clock(id,high_water) VALUES (1,?1)",
            [now],
        )?;
    }
    Ok(())
}

fn check_with_identity(
    config: &Config,
    alias: &str,
    scope: Scope,
    capacity: u32,
    provider_identity: Option<&NativeServerIdentity>,
    compare_provider_identity: bool,
) -> Evaluation {
    if validate_store_boundary(config, &[]).is_err() {
        return Evaluation::denied(STORE_EXPOSED);
    }
    let mut connection = match Connection::open_with_flags(
        &config.database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(connection) => connection,
        Err(_) => return Evaluation::denied(NOT_QUALIFIED),
    };
    if connection
        .busy_timeout(std::time::Duration::from_secs(2))
        .is_err()
    {
        return Evaluation::denied(NOT_QUALIFIED);
    }
    if validate_store_boundary_from_connection(config, &connection).is_err() {
        return Evaluation::denied(STORE_EXPOSED);
    }
    let now = match now_qualification() {
        Ok(now) => now,
        Err(_) => return Evaluation::denied(CLOCK_SKEW),
    };
    let Ok(tx) = connection.transaction_with_behavior(TransactionBehavior::Immediate) else {
        return Evaluation::denied(NOT_QUALIFIED);
    };
    if advance_high_water(&tx, now).is_err() {
        return Evaluation::denied(CLOCK_SKEW);
    }
    let result = evaluate_with_identity(
        &tx,
        config,
        alias,
        scope,
        capacity,
        now,
        provider_identity,
        compare_provider_identity,
    );
    if tx.commit().is_err() {
        return Evaluation::denied(NOT_QUALIFIED);
    }
    result
}

/// Current exact provider identity is required for status and effect-path checks.
pub fn check(config: &Config, alias: &str, scope: Scope, capacity: u32) -> Evaluation {
    check_with_identity(config, alias, scope, capacity, None, true)
}

pub fn check_with_provider_identity(
    config: &Config,
    alias: &str,
    scope: Scope,
    capacity: u32,
    provider_identity: Option<&NativeServerIdentity>,
) -> Evaluation {
    check_with_identity(config, alias, scope, capacity, provider_identity, true)
}

/// Before a client exists, accept only a grant that records an identity. The transport
/// compares that identity with its live initialize response immediately before writes.
pub fn require(config: &Config, alias: &str, capacity: u32) -> Result<()> {
    require_scope(config, alias, Scope::Ordinary, capacity)
}
pub fn require_scope(config: &Config, alias: &str, scope: Scope, capacity: u32) -> Result<()> {
    match check_with_identity(config, alias, scope, capacity, None, false).status {
        Status::Qualified => Ok(()),
        Status::Denied(code) => Err(ExecutionDenied { code }.into()),
    }
}

pub fn require_with_identity(
    config: &Config,
    alias: &str,
    capacity: u32,
    provider_identity: Option<&NativeServerIdentity>,
) -> Result<()> {
    match check_with_identity(
        config,
        alias,
        Scope::Ordinary,
        capacity,
        provider_identity,
        true,
    )
    .status
    {
        Status::Qualified => Ok(()),
        Status::Denied(code) => Err(ExecutionDenied { code }.into()),
    }
}

/// Bounded, path-free per-profile status for capabilities.
pub fn capability(config: &Config) -> Value {
    let profiles = config
        .profiles
        .keys()
        .map(|alias| {
            let evaluation = check(config, alias, Scope::Ordinary, 1);
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
    pub provider_identity: Option<NativeServerIdentity>,
}

pub struct OperatorConfirmation {
    uid: u32,
    tty_name: String,
}

fn sandbox_indicated(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    if matches!(
        name.as_ref(),
        "CODEX_SANDBOX" | "CODEX_SANDBOX_NETWORK_DISABLED" | "CODEX_THREAD_ID"
    ) {
        return true;
    }
    name.starts_with("CODEX_")
        && ["SANDBOX", "THREAD", "NETWORK_DISABLED", "EXECUTION_ENV"]
            .iter()
            .any(|marker| name.contains(marker))
}

fn operator_uid() -> Result<u32> {
    #[cfg(unix)]
    {
        Ok(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("qualification_operator_identity_unavailable")
    }
}

fn tty_name(file: &std::fs::File) -> Result<String> {
    #[cfg(unix)]
    {
        use std::{ffi::CStr, os::fd::AsRawFd};
        let mut buffer = [0i8; 4096];
        let result =
            unsafe { libc::ttyname_r(file.as_raw_fd(), buffer.as_mut_ptr(), buffer.len()) };
        ensure!(result == 0, "qualification_operator_tty_unavailable");
        let name = unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .context("qualification_operator_tty_unavailable")?;
        ensure!(
            !name.is_empty() && name.len() <= 4096,
            "qualification_operator_tty_unavailable"
        );
        Ok(name.to_owned())
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        anyhow::bail!("qualification_operator_tty_unavailable")
    }
}

/// CLI confirmation reads only from `/dev/tty`; piped or sandboxed invocation is refused.
pub fn ensure_operator_context() -> Result<()> {
    ensure!(
        !std::env::vars_os().any(|(name, _)| sandbox_indicated(&name)),
        "qualification_operator_sandboxed"
    );
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "qualification_operator_tty_required"
    );
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| anyhow::anyhow!("qualification_operator_tty_unavailable"))?;
    ensure!(tty.is_terminal(), "qualification_operator_tty_unavailable");
    Ok(())
}

pub fn confirm_operator_action(summary: &str, profile_name: &str) -> Result<OperatorConfirmation> {
    ensure_operator_context()?;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| anyhow::anyhow!("qualification_operator_tty_unavailable"))?;
    ensure!(tty.is_terminal(), "qualification_operator_tty_unavailable");
    println!("{summary}");
    let terminal = tty_name(&tty)?;
    writeln!(tty, "Type profile name '{profile_name}' to confirm:")
        .map_err(|_| anyhow::anyhow!("qualification_operator_tty_unavailable"))?;
    tty.flush()
        .map_err(|_| anyhow::anyhow!("qualification_operator_tty_unavailable"))?;
    let mut answer = String::new();
    std::io::BufReader::new(tty)
        .read_line(&mut answer)
        .map_err(|_| anyhow::anyhow!("qualification_operator_tty_unavailable"))?;
    ensure!(
        answer.trim() == profile_name,
        "qualification_operator_confirmation_mismatch"
    );
    Ok(OperatorConfirmation {
        uid: operator_uid()?,
        tty_name: terminal,
    })
}

/// Test-binary-only authorization for isolated temporary databases.
#[cfg(debug_assertions)]
pub fn test_operator_confirmation() -> OperatorConfirmation {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    assert!(executable.rsplit_once('-').is_some_and(|(_, suffix)| {
        suffix.len() >= 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
    OperatorConfirmation {
        uid: operator_uid().expect("test process uid"),
        tty_name: "test-only".into(),
    }
}

/// Operator-only library primitive. The CLI must provide a confirmed operator identity;
/// no MCP route exposes this function.
pub fn record(
    config: &Config,
    request: RecordRequest<'_>,
    confirmation: &OperatorConfirmation,
) -> Result<Value> {
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
        profile.codex_binary_identity != UNAVAILABLE,
        "codex_binary_identity_unavailable"
    );
    ensure!(
        (config.native_transport == "existing_daemon") == request.provider_identity.is_some(),
        PROVIDER_MISMATCH
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
    validate_store_boundary(config, &[])?;
    let mut connection = open_write(config)?;
    validate_store_boundary_from_connection(config, &connection)
        .map_err(|_| anyhow::anyhow!(STORE_EXPOSED))?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let recorded_at = now_qualification()?;
    advance_high_water(&tx, recorded_at)?;
    let record = Qualification {
        profile,
        provider_identity: request.provider_identity,
        scope: request.scope,
        max_capacity: request.max_capacity,
        evidence_sha256: format!("{:x}", Sha256::digest(&bytes)),
        evidence_ref: evidence_path.to_string_lossy().into_owned(),
        recorded_at,
        expires_at: request
            .expires_in_days
            .map(|days| recorded_at + days * 86400),
        revoked_at: None,
        recorded_by_uid: confirmation.uid,
        tty_name: confirmation.tty_name.clone(),
    };
    tx.execute(
        "INSERT INTO execution_qualifications(fingerprint,profile_alias,payload) VALUES (?1,?2,?3)
         ON CONFLICT(fingerprint) DO UPDATE SET profile_alias=excluded.profile_alias,payload=excluded.payload",
        params![fingerprint, request.alias, serde_json::to_string(&record)?],
    )?;
    tx.commit()?;
    Ok(public(&fingerprint, &record, Some(config)))
}

pub fn profile_alias(config: &Config, fingerprint: &str) -> Result<String> {
    let connection = open_write(config)?;
    let record = load(&connection, fingerprint)?.context("qualification_not_found")?;
    Ok(record.profile.profile)
}

pub fn revoke(
    config: &Config,
    fingerprint: &str,
    _confirmation: &OperatorConfirmation,
) -> Result<Value> {
    ensure!(
        fingerprint.len() == 64 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid_qualification_fingerprint"
    );
    let mut connection = open_write(config)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut record = load(&tx, fingerprint)?.context("qualification_not_found")?;
    if record.revoked_at.is_none() {
        let now = now_qualification()?;
        advance_high_water(&tx, now)?;
        record.revoked_at = Some(now);
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
    let now = now_qualification().ok();
    let current = config
        .and_then(|config| ExecutionProfile::from_config(config, &record.profile.profile).ok())
        .is_some_and(|profile| profile == record.profile);
    let status = if now.is_none() {
        "qualification_clock_skew"
    } else if record.revoked_at.is_some() {
        "revoked"
    } else if record
        .expires_at
        .zip(now)
        .is_some_and(|(expires, now)| expires <= now)
    {
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
        "provider_identity":record.provider_identity,
        "recorded_by_uid":record.recorded_by_uid,"tty_name":record.tty_name,
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
