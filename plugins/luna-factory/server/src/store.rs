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
pub struct Run {
    pub id: String,
    pub request: StartRequest,
    pub canonical_root: String,
    pub repository_identity: String,
    pub state: String,
    pub base_head: String,
    pub current_subject: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub owned_threads: Vec<String>,
    pub generation: u64,
    pub repairs_used: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub deadline_at: u64,
    pub delta: String,
    pub blocker: Option<String>,
    pub observed_model: Option<String>,
    pub observed_effort: Option<String>,
    pub configured_model: Option<String>,
    pub configured_effort: Option<String>,
    pub claim_held: bool,
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
    let output = Command::new("git")
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
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
pub fn repository_subject(root: &Path) -> Result<String> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    let status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let diff = git(root, &["diff", "HEAD", "--binary"])?;
    let mut hash = Sha256::new();
    hash.update(status);
    hash.update(diff);
    // Hash untracked file bytes without retaining or transmitting them.
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for file in untracked.split('\0').filter(|s| !s.is_empty()) {
        let path = root.join(file);
        ensure!(
            path.canonicalize()?.starts_with(root),
            "untracked_symlink_escape"
        );
        let metadata = std::fs::metadata(&path)?;
        ensure!(
            metadata.len() <= 8 * 1024 * 1024,
            "untracked_file_too_large"
        );
        hash.update(file);
        hash.update(std::fs::read(path)?);
    }
    Ok(format!("{head}:{:x}", hash.finalize()))
}
fn validate_request(config: &Config, request: &StartRequest) -> Result<()> {
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
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
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
          CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL);")?;
        Ok(Self { connection })
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
                run: serde_json::from_str(&old)?,
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
            turn_id: None,
            owned_threads: vec![],
            generation: 1,
            repairs_used: 0,
            created_at: timestamp,
            updated_at: timestamp,
            deadline_at: timestamp + request.wall_seconds,
            delta: "Run admitted. Native owner has not started.".into(),
            blocker: None,
            observed_model: None,
            observed_effort: None,
            configured_model: None,
            configured_effort: None,
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
        Ok(serde_json::from_str(&value)?)
    }
    pub fn list(&self, limit: u32) -> Result<Vec<Run>> {
        let mut stmt = self
            .connection
            .prepare("SELECT payload FROM runs ORDER BY rowid DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit.clamp(1, 100)], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn save(&mut self, run: &Run) -> Result<()> {
        ensure!(
            self.get(&run.id)?.request.idempotency_key == run.request.idempotency_key,
            "immutable_run_request"
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
            run.state = "INTERRUPTED".into();
            run.blocker = Some("Runtime restarted. Reconcile native ownership before resuming; no mutation is replayed.".into());
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
            .map(|r| Ok(serde_json::from_str(&r?)?))
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
