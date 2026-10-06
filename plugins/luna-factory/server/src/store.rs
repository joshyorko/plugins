use crate::config::{Config, finish_rank, valid_alias};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub repository: String,
    pub objective: String,
    pub acceptance: Vec<String>,
    pub non_goals: Vec<String>,
    pub finish: String,
    pub profile: String,
    pub capacity: u32,
    pub repair_attempts: u32,
    pub wall_seconds: u64,
    pub idempotency_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDecision {
    pub id: String,
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub request: StartRequest,
    pub canonical_root: String,
    pub repository_identity: String,
    pub state: String,
    pub base_head: String,
    pub current_subject: String,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub dispatch_phase: String,
    #[serde(default)]
    pub dispatch_id: Option<String>,
    pub turn_id: Option<String>,
    pub owned_threads: Vec<String>,
    #[serde(default)]
    pub active_threads: Vec<String>,
    #[serde(default)]
    pub owned_commands: std::collections::BTreeMap<String, bool>,
    #[serde(default)]
    pub command_processes: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub terminal_stop_attempts: std::collections::BTreeMap<String, String>,
    pub generation: u64,
    pub repairs_used: u32,
    #[serde(default)]
    pub counted_failures: std::collections::BTreeSet<(String, String)>,
    #[serde(default)]
    pub pending_decision: Option<PendingDecision>,
    #[serde(default)]
    pub answered_decisions: std::collections::BTreeMap<String, String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deadline_at: u64,
    pub delta: String,
    pub blocker: Option<String>,
    #[serde(default)]
    pub remaining_gap: Option<String>,
    #[serde(default)]
    pub skill_sha256: Option<String>,
    pub observed_model: Option<String>,
    pub observed_effort: Option<String>,
    pub configured_model: Option<String>,
    pub configured_effort: Option<String>,
    #[serde(default)]
    pub requested_effort: Option<String>,
    #[serde(default)]
    pub configured_provider: Option<String>,
    #[serde(default)]
    pub route_observations: Vec<crate::native::NativeRouteObservation>,
    pub claim_held: bool,
}

fn decode_run(payload: &str) -> Result<Run> {
    let mut run: Run = serde_json::from_str(payload)?;
    // Upgrade pre-decision-ID rows from their accepted, terminal owner result.
    // Active native approval requests never qualify for this migration.
    if run.pending_decision.is_none()
        && run.state == "NEEDS_INPUT"
        && run.dispatch_phase == "terminal_observed"
        && let Some(question) = run.blocker.clone()
    {
        run.pending_decision = Some(PendingDecision {
            id: format!("{}-{}", run.id, run.generation),
            question,
        });
    }
    Ok(run)
}
#[derive(Debug)]
pub struct Admission {
    pub run: Run,
    pub created: bool,
}
pub struct Store {
    connection: Connection,
}

pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git_output(root, args)?)?
        .trim()
        .to_owned())
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("git_unavailable")?;
    ensure!(output.status.success(), "repository_inspection_failed");
    ensure!(
        output.stdout.len() < 1024 * 1024,
        "repository_output_too_large"
    );
    Ok(output.stdout)
}
pub fn repository_subject(root: &Path) -> Result<String> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    // NUL-delimited filenames are data, not line-oriented command output.
    // Trimming here silently drops bytes from a leading-whitespace filename.
    let index = String::from_utf8(git_output(root, &["ls-files", "--stage", "-z"])?)?;
    let untracked = String::from_utf8(git_output(
        root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?)?;
    let mut hash = Sha256::new();
    hash.update(b"luna-source-subject-v2\0");
    hash.update(index.as_bytes());
    let mut files = std::collections::BTreeSet::new();
    for entry in index.split('\0').filter(|v| !v.is_empty()) {
        let (metadata, path) = entry.split_once('\t').context("invalid_index_record")?;
        // Nested repositories require their own approved alias and lifecycle claim.
        ensure!(
            !metadata.starts_with("160000 "),
            "submodule_subject_requires_explicit_support"
        );
        files.insert(path);
    }
    files.extend(untracked.split('\0').filter(|v| !v.is_empty()));
    ensure!(files.len() <= 25000, "candidate_file_count_exceeded");
    let mut total = 0u64;
    for name in files {
        let relative = Path::new(name);
        ensure!(
            !relative.is_absolute()
                && !relative
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "invalid_git_path"
        );
        let path = root.join(relative);
        let parent = crate::config::canonical_destination(
            path.parent().context("candidate_parent_missing")?,
        )?;
        ensure!(parent.starts_with(root), "candidate_parent_symlink_escape");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                hash.update(b"missing\0");
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if metadata.file_type().is_symlink() {
            hash.update(b"symlink\0");
            hash.update(std::fs::read_link(&path)?.as_os_str().as_encoded_bytes());
            continue;
        }
        ensure!(metadata.is_file(), "nonregular_candidate_file");
        total = total
            .checked_add(metadata.len())
            .context("candidate_size_overflow")?;
        ensure!(total <= 512 * 1024 * 1024, "candidate_bytes_exceeded");
        hash.update(b"file\0");
        hash.update(metadata.len().to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update((metadata.permissions().mode() & 0o111).to_le_bytes());
        }
        let mut file = std::fs::File::open(path)?;
        let mut buffer = [0u8; 65536];
        loop {
            let count = std::io::Read::read(&mut file, &mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
    }
    Ok(format!("{head}:{:x}", hash.finalize()))
}
pub(crate) fn validate_request(config: &Config, request: &StartRequest) -> Result<()> {
    config.validate()?;
    ensure!(valid_alias(&request.repository), "unknown_repository");
    let repo = config
        .repositories
        .get(&request.repository)
        .context("unknown_repository")?;
    ensure!(
        config.profiles.contains_key(&request.profile),
        "unknown_profile"
    );
    ensure!(
        !request.objective.trim().is_empty() && request.objective.len() <= 8000,
        "invalid_objective"
    );
    ensure!(
        !request.acceptance.is_empty() && request.acceptance.len() <= 32,
        "invalid_acceptance"
    );
    ensure!(request.non_goals.len() <= 32, "too_many_non_goals");
    ensure!(
        request
            .acceptance
            .iter()
            .chain(&request.non_goals)
            .all(|s| !s.trim().is_empty() && s.len() <= 1000),
        "invalid_criterion"
    );
    ensure!(
        finish_rank(&request.finish)? <= finish_rank(&repo.max_finish)?,
        "authority_exceeded"
    );
    ensure!(
        request.capacity > 0 && request.capacity <= config.limits.capacity,
        "capacity_exceeded"
    );
    ensure!(
        request.repair_attempts <= config.limits.repair_attempts,
        "repair_limit_exceeded"
    );
    ensure!(
        request.wall_seconds >= 30 && request.wall_seconds <= config.limits.wall_seconds,
        "time_limit_exceeded"
    );
    ensure!(
        !request.idempotency_key.is_empty() && request.idempotency_key.len() <= 128,
        "invalid_idempotency_key"
    );
    Ok(())
}
impl Store {
    pub fn open(config: &Config) -> Result<Self> {
        config.validate()?;
        let parent = config
            .database
            .parent()
            .context("database_parent_missing")?;
        let parent_existed = parent.exists();
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent_existed {
                ensure!(
                    std::fs::metadata(parent)?.permissions().mode() & 0o077 == 0,
                    "state_directory_must_be_private"
                );
            } else {
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let connection = Connection::open(&config.database)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&config.database, std::fs::Permissions::from_mode(0o600))?;
        }
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, idem TEXT NOT NULL UNIQUE, fingerprint TEXT NOT NULL, root TEXT NOT NULL, state TEXT NOT NULL, payload TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS claims (identity TEXT PRIMARY KEY, run_id TEXT NOT NULL UNIQUE REFERENCES runs(id));
          CREATE TABLE IF NOT EXISTS receipts (sequence INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL REFERENCES runs(id), subject TEXT NOT NULL, kind TEXT NOT NULL, summary TEXT NOT NULL, created_at INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS repository_registrations (id TEXT PRIMARY KEY, alias TEXT NOT NULL UNIQUE, payload TEXT NOT NULL);")?;
        Ok(Self { connection })
    }
    pub fn repository_registrations(&self) -> Result<Vec<crate::repositories::Registration>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM repository_registrations ORDER BY alias LIMIT 101")?;
        let mut registrations = Vec::new();
        for payload in statement.query_map([], |row| row.get::<_, String>(0))? {
            registrations.push(serde_json::from_str(&payload?)?);
        }
        ensure!(
            registrations.len() <= crate::repositories::MAX_REGISTRATIONS,
            "repository_registration_limit"
        );
        Ok(registrations)
    }
    pub fn repository_request(&self, id: &str) -> Result<crate::repositories::Registration> {
        let payload: String = self
            .connection
            .query_row(
                "SELECT payload FROM repository_registrations WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .context("unknown_repository_request")?;
        Ok(serde_json::from_str(&payload)?)
    }
    pub fn request_repository(
        &mut self,
        registration: &crate::repositories::Registration,
    ) -> Result<crate::repositories::Registration> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(payload) = tx
            .query_row(
                "SELECT payload FROM repository_registrations WHERE alias=?1",
                [&registration.alias],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            let existing: crate::repositories::Registration = serde_json::from_str(&payload)?;
            ensure!(
                existing.same_request(registration),
                "repository_alias_conflict"
            );
            return Ok(existing);
        }
        let count: i64 =
            tx.query_row("SELECT COUNT(*) FROM repository_registrations", [], |row| {
                row.get(0)
            })?;
        ensure!(
            count < i64::try_from(crate::repositories::MAX_REGISTRATIONS)?,
            "repository_registration_limit"
        );
        tx.execute(
            "INSERT INTO repository_registrations(id,alias,payload) VALUES(?1,?2,?3)",
            params![
                registration.id,
                registration.alias,
                serde_json::to_string(registration)?
            ],
        )?;
        tx.commit()?;
        Ok(registration.clone())
    }
    pub(crate) fn approve_repository(
        &mut self,
        expected: &crate::repositories::Registration,
    ) -> Result<crate::repositories::Registration> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload: String = tx
            .query_row(
                "SELECT payload FROM repository_registrations WHERE id=?1",
                [&expected.id],
                |row| row.get(0),
            )
            .optional()?
            .context("unknown_repository_request")?;
        let mut current: crate::repositories::Registration = serde_json::from_str(&payload)?;
        ensure!(current.same_request(expected), "repository_request_changed");
        current.status = crate::repositories::Approval::Approved;
        tx.execute(
            "UPDATE repository_registrations SET payload=?1 WHERE id=?2",
            params![serde_json::to_string(&current)?, current.id],
        )?;
        tx.commit()?;
        Ok(current)
    }
    pub fn admit(&mut self, config: &Config, request: &StartRequest) -> Result<Admission> {
        validate_request(config, request)?;
        let payload = serde_json::to_string(request)?;
        let fingerprint = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let root = &config.repositories[&request.repository].root;
        let root_text = root.to_str().context("non_utf8_repository")?.to_owned();
        let toplevel = git(root, &["rev-parse", "--show-toplevel"])?;
        ensure!(
            Path::new(&toplevel).canonicalize()? == *root,
            "repository_root_mismatch"
        );
        let identity = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let identity = Path::new(&identity)
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some((old_fingerprint, old)) = tx
            .query_row(
                "SELECT fingerprint,payload FROM runs WHERE idem=?1",
                [&request.idempotency_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure!(old_fingerprint == fingerprint, "idempotency_conflict");
            return Ok(Admission {
                run: decode_run(&old)?,
                created: false,
            });
        }
        if tx
            .query_row(
                "SELECT run_id FROM claims WHERE identity=?1",
                [&identity],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some()
        {
            bail!("repository_claimed");
        }
        let timestamp = now();
        let run = Run {
            id: Uuid::new_v4().to_string(),
            request: request.clone(),
            canonical_root: root_text,
            repository_identity: identity.clone(),
            state: "STARTING".into(),
            base_head: git(root, &["rev-parse", "HEAD"])?,
            current_subject: repository_subject(root)?,
            thread_id: None,
            dispatch_phase: "admitted".into(),
            dispatch_id: None,
            turn_id: None,
            owned_threads: vec![],
            active_threads: vec![],
            owned_commands: std::collections::BTreeMap::new(),
            command_processes: std::collections::BTreeMap::new(),
            terminal_stop_attempts: std::collections::BTreeMap::new(),
            generation: 1,
            repairs_used: 0,
            counted_failures: std::collections::BTreeSet::new(),
            pending_decision: None,
            answered_decisions: std::collections::BTreeMap::new(),
            created_at: timestamp,
            updated_at: timestamp,
            deadline_at: timestamp + request.wall_seconds,
            delta: "Run admitted. Native owner has not started.".into(),
            blocker: None,
            remaining_gap: Some("Mandatory acceptance needs current owner verification".into()),
            skill_sha256: None,
            observed_model: None,
            observed_effort: None,
            configured_model: None,
            configured_effort: None,
            requested_effort: Some(config.profiles[&request.profile].effort.clone()),
            configured_provider: None,
            route_observations: Vec::new(),
            claim_held: true,
        };
        tx.execute(
            "INSERT INTO runs(id,idem,fingerprint,root,state,payload) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                run.id,
                request.idempotency_key,
                fingerprint,
                run.canonical_root,
                run.state,
                serde_json::to_string(&run)?
            ],
        )?;
        tx.execute(
            "INSERT INTO claims(identity,run_id) VALUES (?1,?2)",
            params![identity, run.id],
        )?;
        tx.commit()?;
        Ok(Admission { run, created: true })
    }
    pub fn get(&self, id: &str) -> Result<Run> {
        let value: String = self
            .connection
            .query_row("SELECT payload FROM runs WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .context("run_not_found")?;
        decode_run(&value)
    }
    pub fn list(&self, limit: u32) -> Result<Vec<Run>> {
        let mut stmt = self
            .connection
            .prepare("SELECT payload FROM runs ORDER BY rowid DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit.clamp(1, 100)], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode_run(&r?)).collect()
    }
    pub fn save(&mut self, run: &Run) -> Result<()> {
        let previous = self.get(&run.id)?;
        ensure!(
            serde_json::to_value(&previous.request)? == serde_json::to_value(&run.request)?
                && previous.canonical_root == run.canonical_root
                && previous.repository_identity == run.repository_identity
                && previous.base_head == run.base_head
                && previous.requested_effort == run.requested_effort
                && previous.deadline_at == run.deadline_at,
            "immutable_run_contract"
        );
        ensure!(
            previous
                .thread_id
                .as_ref()
                .is_none_or(|id| run.thread_id.as_ref() == Some(id)),
            "immutable_owner_identity"
        );
        self.connection.execute(
            "UPDATE runs SET state=?2,payload=?3 WHERE id=?1",
            params![run.id, run.state, serde_json::to_string(run)?],
        )?;
        Ok(())
    }
    pub fn claim_owner(&self, root: &str) -> Result<Option<String>> {
        Ok(self.connection.query_row("SELECT claims.run_id FROM claims JOIN runs ON runs.id=claims.run_id WHERE runs.root=?1",[root],|r|r.get(0)).optional()?)
    }
    pub fn mark_interrupted(&mut self) -> Result<()> {
        let runs = self.list_all_claimed()?;
        for mut run in runs {
            if let Some(decision) = &run.pending_decision {
                run.state = "NEEDS_INPUT".into();
                run.blocker = Some(decision.question.clone());
                run.delta = "Runtime restarted. The pending decision is preserved; native ownership will be reconciled before continuing.".into();
            } else {
                run.state = "INTERRUPTED".into();
                run.blocker = Some("Runtime restarted. Reconcile native ownership before resuming; no mutation is replayed.".into());
            }
            run.updated_at = now();
            self.save(&run)?;
        }
        Ok(())
    }
    pub fn list_all_claimed(&self) -> Result<Vec<Run>> {
        let mut stmt = self
            .connection
            .prepare("SELECT runs.payload FROM runs JOIN claims ON claims.run_id=runs.id")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| decode_run(&r?))
            .collect()
    }
    /// Only the lifecycle reconciler may call this after observing every owned thread stopped.
    pub fn release_verified(&mut self, run: &mut Run) -> Result<()> {
        ensure!(
            ["CANCELLED", "CONVERGED", "QUIESCENT", "FAILED"].contains(&run.state.as_str()),
            "claim_release_requires_terminal_state"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM claims WHERE run_id=?1 AND identity=?2",
            params![run.id, run.repository_identity],
        )?;
        run.claim_held = false;
        tx.execute(
            "UPDATE runs SET state=?2,payload=?3 WHERE id=?1",
            params![run.id, run.state, serde_json::to_string(run)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn receipt(&mut self, run: &Run, kind: &str, summary: &str) -> Result<()> {
        ensure!(
            kind.len() <= 64 && summary.len() <= 2000,
            "receipt_too_large"
        );
        self.connection.execute(
            "INSERT INTO receipts(run_id,subject,kind,summary,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![run.id, run.current_subject, kind, summary, now() as i64],
        )?;
        self.connection.execute("DELETE FROM receipts WHERE run_id=?1 AND sequence NOT IN (SELECT sequence FROM receipts WHERE run_id=?1 ORDER BY sequence DESC LIMIT 100)",[&run.id])?;
        Ok(())
    }
    pub fn receipts(&self, run_id: &str) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.connection.prepare("SELECT subject,kind,summary,created_at FROM receipts WHERE run_id=?1 ORDER BY sequence DESC LIMIT 20")?;
        Ok(stmt.query_map([run_id],|r|Ok(serde_json::json!({"subject":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"summary":r.get::<_,String>(2)?,"created_at":r.get::<_,i64>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn reclaim(&mut self, run: &mut Run) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT run_id FROM claims WHERE identity=?1",
                [&run.repository_identity],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            owner.as_deref().is_none_or(|id| id == run.id),
            "repository_claimed"
        );
        tx.execute(
            "INSERT OR IGNORE INTO claims(identity,run_id) VALUES (?1,?2)",
            params![run.repository_identity, run.id],
        )?;
        run.claim_held = true;
        tx.execute(
            "UPDATE runs SET payload=?2 WHERE id=?1",
            params![run.id, serde_json::to_string(run)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn settings(&self) -> Result<serde_json::Value> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT payload FROM settings WHERE id=1", [], |r| r.get(0))
            .optional()?;
        Ok(value
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({})))
    }
    pub fn save_settings(&mut self, value: &serde_json::Value) -> Result<()> {
        self.connection.execute("INSERT INTO settings(id,payload) VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",[serde_json::to_string(value)?])?;
        Ok(())
    }
}
