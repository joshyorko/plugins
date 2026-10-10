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
    #[serde(default)]
    pub planning_only: bool,
    #[serde(default)]
    pub graph_repository_stamp: Option<String>,
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
    #[serde(default)]
    pub control: Option<crate::control::Control>,
    #[serde(skip)]
    pub observed_claim: ObservedClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObservedClaim {
    Owned,
    Released,
    Foreign,
    #[default]
    Unknown,
}
impl Run {
    pub fn set_state(&mut self, state: crate::control::RunControl) {
        self.control.as_mut().expect("decoded_control").run_control = state;
        self.state = state.legacy().into();
    }
}

fn decode_run(payload: &str) -> Result<Run> {
    decode_versioned_run(payload, 2)
}
fn decode_versioned_run(payload: &str, version: u32) -> Result<Run> {
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
    if run.control.is_none() {
        let mut control = crate::control::Control::new(
            &run.current_subject,
            &run.request.acceptance,
            run.request.repair_attempts,
            run.deadline_at,
        )?;
        control.migrated = true;
        control.repairs_used = run.repairs_used;
        control.dispatch_generation = if run.thread_id.is_some() || run.turn_id.is_some() {
            run.generation
        } else {
            run.generation.saturating_sub(1)
        };
        if let Some(id) = &run.dispatch_id {
            control.dispatch_generation = run.generation;
            control.attempts.push(crate::control::Attempt {
                id: id.clone(),
                task_id: "objective".into(),
                parent: None,
                intent_generation: 1,
                dispatch_generation: run.generation,
                source_subject: run.current_subject.clone(),
                assumptions: Default::default(),
                phase: if run.dispatch_phase == "terminal_observed" {
                    "returned"
                } else if run.turn_id.is_some() {
                    "active"
                } else {
                    "intent_unknown"
                }
                .into(),
                turn_id: run.turn_id.clone(),
                certified_before: 0,
                diagnosis: None,
            });
            control.tasks.get_mut("objective").unwrap().state = crate::control::TaskState::Verify;
        }
        for child in &run.owned_threads {
            control.observe_child(child, false)?;
        }
        if run.state == "CONVERGED" && run.request.finish != "local_candidate" {
            control.observe_effect(
                &format!("legacy-delivery:{}", run.id),
                if run.request.finish == "push" {
                    crate::control::EffectKind::Push
                } else {
                    crate::control::EffectKind::Pr
                },
            )?;
        }
        if run.state == "CONVERGED" {
            run.state = "QUIESCENT".into();
            run.remaining_gap =
                Some("Legacy owner prose has no independently observed check references.".into());
        }
        control.run_control = crate::control::RunControl::from_legacy(&run.state)?;
        if run.dispatch_phase == "terminal_observed" {
            control.settlement = crate::control::Settlement::Stopped;
            control.owner_liveness = crate::control::Settlement::Stopped;
        }
        run.control = Some(control);
    }
    if version == 1 {
        let control = run.control.as_mut().context("control_missing")?;
        ensure!(control.schema_version == 1, "unsupported_control_schema");
        control.schema_version = crate::control::SCHEMA_VERSION;
        // V1 has no authoritative typed census fact. Preserve observed checks,
        // bindings and unknown effects, but require fresh native reconciliation.
        control.settlement = crate::control::Settlement::Unknown;
        control.owner_liveness = crate::control::Settlement::Unknown;
        for thread in &run.owned_threads {
            control
                .child_liveness
                .insert(thread.clone(), crate::control::Settlement::Unknown);
        }
        control.run_control = crate::control::RunControl::from_legacy(&run.state)?;
        if control.run_control == crate::control::RunControl::Converged {
            control.run_control = crate::control::RunControl::Quiescent;
            run.state = "QUIESCENT".into();
            run.remaining_gap = Some(
                "Imported v1 proof needs current native settlement and file revalidation.".into(),
            );
        }
    }
    let control = run.control.as_ref().context("control_missing")?;
    ensure!(
        control.run_control.legacy() == run.state,
        "legacy_control_projection_mismatch"
    );
    control.validate()?;
    validate_planning_run(&run)?;
    Ok(run)
}
fn validate_planning_run(run: &Run) -> Result<()> {
    if run.planning_only {
        let control = run.control.as_ref().context("control_missing")?;
        ensure!(
            !run.claim_held
                && run.thread_id.is_none()
                && run.turn_id.is_none()
                && run.dispatch_id.is_none()
                && run.owned_threads.is_empty()
                && run.active_threads.is_empty()
                && run.owned_commands.is_empty()
                && control.attempts.is_empty()
                && control.effects.is_empty()
                && control.dispatch_generation == 0
                && control
                    .tasks
                    .values()
                    .all(|task| task.state == crate::control::TaskState::Candidate
                        && task.effects.is_empty()
                        && task.claim == "unknown"),
            "planning_graph_execution_denied"
        );
    }
    Ok(())
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
          CREATE TABLE IF NOT EXISTS repository_registrations (id TEXT PRIMARY KEY, alias TEXT NOT NULL UNIQUE, payload TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS campaigns (id TEXT PRIMARY KEY, idem TEXT NOT NULL UNIQUE, fingerprint TEXT NOT NULL, repository_identity TEXT NOT NULL, parent_provider TEXT NOT NULL, parent_item TEXT NOT NULL, planning_run_id TEXT NOT NULL UNIQUE REFERENCES runs(id), payload TEXT NOT NULL, UNIQUE(repository_identity, parent_provider, parent_item));")?;
        let mut store = Self { connection };
        store.migrate_control()?;
        Ok(store)
    }
    fn migrate_control(&mut self) -> Result<()> {
        let version: u32 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        ensure!(version <= 2, "unsupported_database_schema");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS control_events (run_id TEXT NOT NULL REFERENCES runs(id),event_id TEXT NOT NULL,revision INTEGER NOT NULL,fingerprint TEXT NOT NULL,envelope TEXT NOT NULL,snapshot_sha256 TEXT NOT NULL,PRIMARY KEY(run_id,event_id),UNIQUE(run_id,revision));")?;
        let rows = {
            let mut statement = tx.prepare("SELECT id,payload FROM runs")?;
            statement
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, payload) in rows {
            let raw: serde_json::Value = serde_json::from_str(&payload)?;
            ensure!(
                version == 0 || raw.get("control").is_some_and(|v| !v.is_null()),
                "control_state_missing"
            );
            if version > 0 {
                let (digest,revision):(String,i64)=tx.query_row("SELECT snapshot_sha256,revision FROM control_events WHERE run_id=?1 ORDER BY revision DESC LIMIT 1",[&id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.context("control_journal_missing")?;
                ensure!(
                    digest == format!("{:x}", Sha256::digest(payload.as_bytes()))
                        && raw["control"]["revision"].as_u64() == u64::try_from(revision).ok(),
                    "control_snapshot_corrupt"
                );
            }
            let mut run = decode_versioned_run(&payload, version)?;
            ensure!(run.id == id, "run_identity_corrupt");
            if version < 2 {
                if version == 1 {
                    run.control.as_mut().unwrap().revision += 1;
                }
                let next = serde_json::to_string(&run)?;
                let digest = format!("{:x}", Sha256::digest(next.as_bytes()));
                tx.execute(
                    "UPDATE runs SET payload=?2,state=?3 WHERE id=?1",
                    params![id, next, run.state],
                )?;
                tx.execute("INSERT INTO control_events(run_id,event_id,revision,fingerprint,envelope,snapshot_sha256) VALUES (?1,?2,?3,?4,'{\"kind\":\"schema_import_no_effects\"}',?4)",params![id,if version==0{"migration"}else{"schema-v2"},i64::try_from(run.control.as_ref().unwrap().revision)?,digest])?;
            } else {
                ensure!(
                    raw["control"]["schema_version"] == 2,
                    "unsupported_control_schema"
                );
            }
        }
        tx.execute_batch("PRAGMA user_version=2;")?;
        tx.commit()?;
        Ok(())
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
        self.admit_mode(config, request, false)
    }
    pub fn admit_planning(&mut self, config: &Config, request: &StartRequest) -> Result<Admission> {
        self.admit_mode(config, request, true)
    }
    fn admit_mode(
        &mut self,
        config: &Config,
        request: &StartRequest,
        planning_only: bool,
    ) -> Result<Admission> {
        validate_request(config, request)?;
        // Preserve legacy executable fingerprints; planning uses a distinct domain.
        let payload = if planning_only {
            serde_json::to_string(&serde_json::json!({"planning_only":true,"request":request}))?
        } else {
            serde_json::to_string(request)?
        };
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
            let run = decode_run(&old)?;
            ensure!(run.planning_only == planning_only, "idempotency_conflict");
            ensure!(
                run.canonical_root == root_text && run.repository_identity == identity,
                "repository_alias_was_remapped"
            );
            if let Some(stamp) = &run.graph_repository_stamp {
                ensure!(
                    crate::graph::repository_stamp(root, &identity)? == *stamp,
                    "repository_identity_changed"
                );
            }
            return Ok(Admission {
                run,
                created: false,
            });
        }
        if !planning_only
            && tx
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
        let mut run = Run {
            id: Uuid::new_v4().to_string(),
            planning_only,
            graph_repository_stamp: Some(crate::graph::repository_stamp(root, &identity)?),
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
            claim_held: !planning_only,
            control: None,
            observed_claim: if planning_only {
                ObservedClaim::Released
            } else {
                ObservedClaim::Owned
            },
        };
        run.control = Some(crate::control::Control::new(
            &run.current_subject,
            &request.acceptance,
            request.repair_attempts,
            run.deadline_at,
        )?);
        if planning_only {
            run.set_state(crate::control::RunControl::Quiescent);
            run.dispatch_phase = "planning_only".into();
            run.delta = "Planning graph created; candidates have no execution authority.".into();
            let task = run
                .control
                .as_mut()
                .unwrap()
                .tasks
                .get_mut("objective")
                .unwrap();
            task.state = crate::control::TaskState::Candidate;
            task.effects.clear();
            task.claim = "unknown".into();
            task.reason = Some("planning_candidate_no_authority".into());
        }
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
        if !planning_only {
            tx.execute(
                "INSERT INTO claims(identity,run_id) VALUES (?1,?2)",
                params![identity, run.id],
            )?;
        }
        let snapshot = serde_json::to_string(&run)?;
        let digest = format!("{:x}", Sha256::digest(snapshot.as_bytes()));
        tx.execute("INSERT INTO control_events(run_id,event_id,revision,fingerprint,envelope,snapshot_sha256) VALUES (?1,'admission',0,?2,'{\"kind\":\"admission\"}',?2)",params![run.id,digest])?;
        tx.commit()?;
        Ok(Admission { run, created: true })
    }
    pub fn get(&self, id: &str) -> Result<Run> {
        let tx = self.connection.unchecked_transaction()?;
        let payload: String = tx
            .query_row("SELECT payload FROM runs WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .context("run_not_found")?;
        let raw: serde_json::Value = serde_json::from_str(&payload)?;
        ensure!(
            raw["control"]["schema_version"] == 2,
            "unsupported_control_schema"
        );
        let (digest,revision):(String,i64)=tx.query_row("SELECT snapshot_sha256,revision FROM control_events WHERE run_id=?1 ORDER BY revision DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.context("control_journal_missing")?;
        ensure!(
            digest == format!("{:x}", Sha256::digest(payload.as_bytes()))
                && raw["control"]["revision"].as_u64() == u64::try_from(revision).ok(),
            "control_snapshot_corrupt"
        );
        let mut run = decode_run(&payload)?;
        ensure!(run.id == id, "run_identity_corrupt");
        let owner: Option<String> = tx
            .query_row(
                "SELECT run_id FROM claims WHERE identity=?1",
                [&run.repository_identity],
                |r| r.get(0),
            )
            .optional()?;
        run.observed_claim = match owner {
            Some(owner) if owner == run.id => ObservedClaim::Owned,
            Some(_) => ObservedClaim::Foreign,
            None if !run.claim_held => ObservedClaim::Released,
            None => ObservedClaim::Unknown,
        };
        tx.commit()?;
        Ok(run)
    }
    pub fn list(&self, limit: u32) -> Result<Vec<Run>> {
        let ids = {
            let mut stmt = self
                .connection
                .prepare("SELECT id FROM runs ORDER BY rowid DESC LIMIT ?1")?;
            stmt.query_map([limit.clamp(1, 100)], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        ids.iter().map(|id| self.get(id)).collect()
    }
    pub fn save(&mut self, run: &mut Run) -> Result<()> {
        self.persist(run, None)
    }
    pub fn apply_event(
        &mut self,
        run: &mut Run,
        event: &crate::control::EventEnvelope,
    ) -> Result<bool> {
        if let crate::control::Event::CasPlanned { request } = &event.event {
            ensure!(
                run.planning_only && !run.claim_held && run.thread_id.is_none(),
                "cas_plan_requires_planning_run"
            );
            ensure!(
                run.graph_repository_stamp.as_ref() == Some(&request.repository_stamp),
                "cas_plan_repository_mismatch"
            );
        }
        let fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(event)?));
        if let Some(previous) = self
            .connection
            .query_row(
                "SELECT fingerprint FROM control_events WHERE run_id=?1 AND event_id=?2",
                params![run.id, event.id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            ensure!(previous == fingerprint, "event_identity_conflict");
            *run = self.get(&run.id)?;
            return Ok(false);
        }
        let next = crate::control::reduce(run.control.as_ref().context("control_missing")?, event)?;
        let previous = run.control.replace(next);
        run.state = run.control.as_ref().unwrap().run_control.legacy().into();
        if let Err(error) = self.persist(run, Some((event, &fingerprint))) {
            run.control = previous;
            return Err(error);
        }
        Ok(true)
    }
    fn persist(
        &mut self,
        run: &mut Run,
        event: Option<(&crate::control::EventEnvelope, &str)>,
    ) -> Result<()> {
        let previous = self.get(&run.id)?;
        ensure!(
            serde_json::to_value(&previous.request)? == serde_json::to_value(&run.request)?
                && previous.canonical_root == run.canonical_root
                && previous.repository_identity == run.repository_identity
                && previous.base_head == run.base_head
                && previous.requested_effort == run.requested_effort
                && previous.deadline_at == run.deadline_at
                && previous.planning_only == run.planning_only
                && previous.graph_repository_stamp == run.graph_repository_stamp,
            "immutable_run_contract"
        );
        ensure!(
            previous
                .thread_id
                .as_ref()
                .is_none_or(|id| run.thread_id.as_ref() == Some(id)),
            "immutable_owner_identity"
        );
        let old_revision = previous
            .control
            .as_ref()
            .context("control_missing")?
            .revision;
        let expected = event.map_or(
            run.control.as_ref().context("control_missing")?.revision,
            |(event, _)| event.expected_revision,
        );
        ensure!(old_revision == expected, "stale_control_revision");
        ensure!(
            run.repairs_used >= previous.repairs_used && run.generation >= previous.generation,
            "budget_or_generation_reset"
        );
        let control = run.control.as_mut().context("control_missing")?;
        if event.is_none() {
            control.revision = old_revision.checked_add(1).context("revision_exhausted")?;
        }
        ensure!(
            control.revision == old_revision + 1
                && control.repair_limit == run.request.repair_attempts
                && control.deadline_at == run.deadline_at,
            "immutable_control_budget"
        );
        control.repairs_used = run.repairs_used;
        ensure!(
            control.run_control.legacy() == run.state
                && control.current_subject == run.current_subject
                && control.deadline_at == run.deadline_at,
            "legacy_control_projection_mismatch"
        );
        control.validate()?;
        ensure!(
            control.cas_requests.is_empty() || run.planning_only,
            "cas_plan_requires_planning_run"
        );
        for (id, request) in &previous.control.as_ref().unwrap().cas_requests {
            ensure!(
                control.cas_requests.get(id) == Some(request),
                "immutable_cas_request"
            );
        }
        for (id, source) in &previous.control.as_ref().unwrap().graph_sources {
            ensure!(
                control.graph_sources.get(id) == Some(source),
                "immutable_graph_source"
            );
        }
        validate_planning_run(run)?;
        let payload = serde_json::to_string(run)?;
        let snapshot = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let event_id =
            event.map_or_else(|| Uuid::new_v4().to_string(), |(event, _)| event.id.clone());
        let envelope = event.map_or_else(
            || {
                Ok(
                    serde_json::json!({"kind":"snapshot","expected_revision":old_revision})
                        .to_string(),
                )
            },
            |(event, _)| serde_json::to_string(event),
        )?;
        let fingerprint = event.map_or(snapshot.clone(), |(_, fingerprint)| fingerprint.to_owned());
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.query_row(
                "SELECT count(*) FROM control_events WHERE run_id=?1",
                [&run.id],
                |r| r.get::<_, i64>(0)
            )? < 10000,
            "control_event_bound_reached"
        );
        ensure!(tx.execute("UPDATE runs SET state=?2,payload=?3 WHERE id=?1 AND json_extract(payload,'$.control.revision')=?4",params![run.id,run.state,payload,i64::try_from(old_revision)?])?==1,"stale_control_revision");
        tx.execute("INSERT INTO control_events(run_id,event_id,revision,fingerprint,envelope,snapshot_sha256) VALUES (?1,?2,?3,?4,?5,?6)",params![run.id,event_id,i64::try_from(old_revision+1)?,fingerprint,envelope,snapshot])?;
        if run.claim_held {
            tx.execute(
                "INSERT OR IGNORE INTO claims(identity,run_id) VALUES (?1,?2)",
                params![run.repository_identity, run.id],
            )?;
            let owner: String = tx.query_row(
                "SELECT run_id FROM claims WHERE identity=?1",
                [&run.repository_identity],
                |r| r.get(0),
            )?;
            ensure!(owner == run.id, "repository_claimed");
        } else {
            tx.execute(
                "DELETE FROM claims WHERE run_id=?1 AND identity=?2",
                params![run.id, run.repository_identity],
            )?;
        }
        tx.commit()?;
        run.observed_claim = if run.claim_held {
            ObservedClaim::Owned
        } else {
            ObservedClaim::Released
        };
        Ok(())
    }
    pub fn claim_owner(&self, root: &str) -> Result<Option<String>> {
        Ok(self.connection.query_row("SELECT claims.run_id FROM claims JOIN runs ON runs.id=claims.run_id WHERE runs.root=?1",[root],|r|r.get(0)).optional()?)
    }
    pub fn mark_interrupted(&mut self) -> Result<()> {
        let runs = self.list_all_claimed()?;
        for mut run in runs {
            if let Some(decision) = run.pending_decision.clone() {
                run.set_state(crate::control::RunControl::NeedsInput);
                run.blocker = Some(decision.question.clone());
                run.delta = "Runtime restarted. The pending decision is preserved; native ownership will be reconciled before continuing.".into();
            } else {
                run.set_state(crate::control::RunControl::Interrupted);
                run.blocker = Some("Runtime restarted. Reconcile native ownership before resuming; no mutation is replayed.".into());
            }
            if run.control.as_ref().unwrap().settlement != crate::control::Settlement::Stopped {
                let control = run.control.as_mut().unwrap();
                control.settlement = crate::control::Settlement::Unknown;
                control.owner_liveness = crate::control::Settlement::Unknown;
                for fact in control.child_liveness.values_mut() {
                    *fact = crate::control::Settlement::Unknown;
                }
            }
            run.updated_at = now();
            self.save(&mut run)?;
        }
        Ok(())
    }
    pub fn list_all_claimed(&self) -> Result<Vec<Run>> {
        let ids = {
            let mut stmt = self
                .connection
                .prepare("SELECT runs.id FROM runs JOIN claims ON claims.run_id=runs.id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        ids.iter().map(|id| self.get(id)).collect()
    }
    /// Only the lifecycle reconciler may call this after observing every owned thread stopped.
    pub fn release_verified(&mut self, run: &mut Run) -> Result<()> {
        ensure!(
            ["CANCELLED", "CONVERGED", "QUIESCENT", "FAILED"].contains(&run.state.as_str()),
            "claim_release_requires_terminal_state"
        );
        ensure!(
            !matches!(
                self.get(&run.id)?.observed_claim,
                ObservedClaim::Foreign | ObservedClaim::Unknown
            ),
            "foreign_or_unknown_claim"
        );
        ensure!(
            run.control.as_ref().context("control_missing")?.settlement
                == crate::control::Settlement::Stopped
                || (run.thread_id.is_none() && run.dispatch_phase == "admitted"),
            "native_cessation_unproved"
        );
        if run.state == "CONVERGED" {
            ensure!(
                run.control.as_ref().is_some_and(|c| c.converged()),
                "unproved_mandatory_criterion"
            );
        }
        ensure!(
            !run.control
                .as_ref()
                .context("control_missing")?
                .unknown_effect(),
            "effect_outcome_unknown"
        );
        run.claim_held = false;
        self.persist(run, None)?;
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
        ensure!(!run.planning_only, "planning_graph_execution_denied");
        let owner: Option<String> = self
            .connection
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
        run.claim_held = true;
        self.persist(run, None)
    }
    fn query_campaigns(
        &self,
        clause: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<crate::campaign::Campaign>> {
        let mut statement = self.connection.prepare(&format!("SELECT id,idem,fingerprint,repository_identity,parent_provider,parent_item,planning_run_id,payload FROM campaigns {clause}"))?;
        let rows = statement
            .query_map(params, |row| {
                Ok((
                    [
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ],
                    row.get::<_, String>(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(columns, payload)| {
                let campaign: crate::campaign::Campaign = serde_json::from_str(&payload)?;
                campaign.validate()?;
                // Indexed columns must agree with the payload they constrain.
                ensure!(
                    columns
                        == [
                            campaign.id.clone(),
                            campaign.idempotency_key.clone(),
                            campaign.fingerprint.clone(),
                            campaign.repository_identity.clone(),
                            campaign.parent.provider.clone(),
                            campaign.parent.item_id.clone(),
                            campaign.planning_run_id.clone(),
                        ],
                    "campaign_record_corrupt"
                );
                Ok(campaign)
            })
            .collect()
    }
    pub fn campaign(&self, id: &str) -> Result<crate::campaign::Campaign> {
        self.query_campaigns("WHERE id=?1", [id])?
            .pop()
            .context("campaign_not_found")
    }
    pub fn campaign_by_key(&self, key: &str) -> Result<Option<crate::campaign::Campaign>> {
        Ok(self.query_campaigns("WHERE idem=?1", [key])?.pop())
    }
    pub fn campaign_by_parent(
        &self,
        identity: &str,
        provider: &str,
        item_id: &str,
    ) -> Result<Option<crate::campaign::Campaign>> {
        Ok(self
            .query_campaigns(
                "WHERE repository_identity=?1 AND parent_provider=?2 AND parent_item=?3",
                [identity, provider, item_id],
            )?
            .pop())
    }
    pub fn campaigns(&self, limit: u32) -> Result<Vec<crate::campaign::Campaign>> {
        self.query_campaigns("ORDER BY rowid DESC LIMIT ?1", [limit.clamp(1, 100)])
    }
    /// Links an existing planning-only graph. Never touches the run, its claim or journal.
    pub fn insert_campaign(&mut self, campaign: &crate::campaign::Campaign) -> Result<()> {
        campaign.validate()?;
        let planning = self.get(&campaign.planning_run_id)?;
        ensure!(
            planning.planning_only
                && !planning.claim_held
                && planning.request.repository == campaign.repository
                && planning.request.finish == campaign.finish
                && crate::graph::repository_id(&planning) == campaign.repository_identity,
            "campaign_planning_run_invalid"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (sql, value, reason) in [
            (
                "SELECT 1 FROM campaigns WHERE idem=?1",
                &campaign.idempotency_key,
                "campaign_idempotency_conflict",
            ),
            (
                "SELECT 1 FROM campaigns WHERE planning_run_id=?1",
                &campaign.planning_run_id,
                "campaign_planning_run_linked",
            ),
        ] {
            ensure!(
                tx.query_row(sql, [value], |_| Ok(())).optional()?.is_none(),
                reason
            );
        }
        ensure!(
            tx.query_row(
                "SELECT 1 FROM campaigns WHERE repository_identity=?1 AND parent_provider=?2 AND parent_item=?3",
                params![campaign.repository_identity, campaign.parent.provider, campaign.parent.item_id],
                |_| Ok(())
            )
            .optional()?
            .is_none(),
            "campaign_parent_exists"
        );
        tx.execute(
            "INSERT INTO campaigns(id,idem,fingerprint,repository_identity,parent_provider,parent_item,planning_run_id,payload) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                campaign.id,
                campaign.idempotency_key,
                campaign.fingerprint,
                campaign.repository_identity,
                campaign.parent.provider,
                campaign.parent.item_id,
                campaign.planning_run_id,
                serde_json::to_string(campaign)?
            ],
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
