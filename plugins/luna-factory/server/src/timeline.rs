//! App-only per-agent observation of native Codex history through the read-only
//! CAS adapter. Everything here is "Observed in Codex": it never writes the
//! Factory ledger, satisfies a criterion, creates attention or certifies
//! execution. Native message bodies, command lines and output, file paths and
//! contents stay inside this module; only a small redacted vocabulary leaves it.
use crate::cas::{CasClient, CasTarget, ItemsPage, SnapshotObservation, native_time};
use crate::lifecycle::safe_summary;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const SOURCE: &str = "codex-action-server";
pub const MAX_EVENTS: usize = 100;
pub const MAX_CHILDREN: usize = 64;
pub const SUMMARY_LIMIT: usize = 160;
/// Reads inside this window are answered from memory without any CAS request.
pub const FRESH_FOR: Duration = Duration::from_secs(20);
pub const CACHED_THREADS: usize = 64;
const CACHED_OLDER_PAGES: usize = 4;
pub const KINDS: [&str; 10] = [
    "assigned",
    "command",
    "file_change",
    "commit",
    "pr_opened",
    "check_result",
    "subagent_spawned",
    "message",
    "waiting",
    "error",
];
pub const REASONS: [&str; 6] = [
    "cas_timeline_not_configured",
    "cas_unavailable",
    "cas_action_failed",
    "cas_binding_mismatch",
    "cas_response_too_large",
    "cas_invalid_response",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadAgentTimeline {
    pub run_id: String,
    pub thread_id: String,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub at: u64,
    pub kind: &'static str,
    pub summary: String,
    pub thread_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Child {
    pub parent_thread: String,
    pub receiver_thread: String,
}

pub fn valid_thread_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
/// Factory-known identities only: the run's owner, or one of its owned or active workers.
pub fn run_owns_thread(run: &crate::store::Run, thread_id: &str) -> bool {
    run.thread_id.as_deref() == Some(thread_id)
        || run
            .owned_threads
            .iter()
            .chain(&run.active_threads)
            .any(|known| known == thread_id)
}

#[derive(Clone)]
struct Page {
    fetched: Instant,
    observed_at: u64,
    thread_status: &'static str,
    native_updated_at: Option<u64>,
    events: Vec<Event>,
    children: Vec<Child>,
    omitted: usize,
    next_cursor: Option<String>,
}
#[derive(Default)]
struct Entry {
    /// Snapshot revision of `head`; the change-detection cursor for polling.
    revision: Option<String>,
    head: Option<Page>,
    /// Older pages, keyed by the cursor this server issued for them.
    older: Vec<(String, Page)>,
    failure: Option<(Instant, &'static str)>,
    used: Option<Instant>,
}
impl Entry {
    fn issued(&self, cursor: &str) -> bool {
        self.head
            .iter()
            .chain(self.older.iter().map(|(_, page)| page))
            .any(|page| page.next_cursor.as_deref() == Some(cursor))
    }
}
enum Fetched {
    Unchanged,
    Page(String, Page),
}

/// Bounded in-memory cache, one entry per configured CAS alias and thread.
/// Observations are disposable: keeping them out of SQLite means they can never
/// reach the control journal, and a restart costs only one fresh read.
pub struct AgentTimelines {
    fresh_for: Duration,
    entries: Mutex<HashMap<(String, String), Entry>>,
}
impl AgentTimelines {
    pub fn new(fresh_for: Duration) -> Self {
        Self {
            fresh_for,
            entries: Mutex::new(HashMap::new()),
        }
    }
    /// One bounded CAS client per call: a snapshot for the exact binding and
    /// change detection, then one item page only when something changed. The
    /// cache lock is never held across network IO.
    pub async fn read(
        &self,
        alias: &str,
        target: &CasTarget,
        run_id: &str,
        thread_id: &str,
        cursor: Option<&str>,
    ) -> Result<Value> {
        let key = (alias.to_owned(), thread_id.to_owned());
        let now = Instant::now();
        let (revision, prior) = {
            // Look up without inserting: only a completed read creates an entry,
            // so the thread cap below is enforced against real entries.
            let mut entries = self.entries.lock().await;
            let entry = entries.get_mut(&key);
            if let Some(cursor) = cursor {
                ensure!(
                    entry.as_ref().is_some_and(|entry| entry.issued(cursor)),
                    "timeline_cursor_not_issued"
                );
            }
            match entry {
                None => (None, None),
                Some(entry) => {
                    entry.used = Some(now);
                    if let Some((at, reason)) = entry.failure
                        && now.duration_since(at) < self.fresh_for
                    {
                        return Ok(unavailable(run_id, thread_id, reason));
                    }
                    let cached = match cursor {
                        None => entry.head.as_ref(),
                        Some(cursor) => entry
                            .older
                            .iter()
                            .find(|(key, _)| key == cursor)
                            .map(|(_, page)| page),
                    };
                    if let Some(page) =
                        cached.filter(|page| now.duration_since(page.fetched) < self.fresh_for)
                    {
                        return Ok(observed(run_id, thread_id, page, "hit"));
                    }
                    match (cursor, &entry.head) {
                        (None, Some(head)) => (entry.revision.clone(), Some(head.clone())),
                        _ => (None, None),
                    }
                }
            }
        };
        let outcome = fetch(target, thread_id, cursor, revision.as_deref()).await;
        let mut entries = self.entries.lock().await;
        if !entries.contains_key(&key) {
            while entries.len() >= CACHED_THREADS {
                let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.used)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                entries.remove(&oldest);
            }
        }
        let entry = entries.entry(key).or_default();
        entry.used = Some(Instant::now());
        match outcome {
            Err(error) => {
                let reason = reason(&error);
                entry.failure = Some((Instant::now(), reason));
                Ok(unavailable(run_id, thread_id, reason))
            }
            Ok(Fetched::Unchanged) => {
                let Some(mut page) = prior else {
                    entry.failure = Some((Instant::now(), "cas_invalid_response"));
                    return Ok(unavailable(run_id, thread_id, "cas_invalid_response"));
                };
                page.fetched = Instant::now();
                page.observed_at = crate::store::now();
                let value = observed(run_id, thread_id, &page, "revalidated");
                // A concurrent read may have stored a newer head meanwhile; only
                // refresh the head this revision check actually confirmed.
                if entry.revision == revision {
                    entry.failure = None;
                    entry.head = Some(page);
                }
                Ok(value)
            }
            Ok(Fetched::Page(revision, page)) => {
                entry.failure = None;
                let value = observed(run_id, thread_id, &page, "miss");
                match cursor {
                    None => {
                        entry.revision = Some(revision);
                        entry.head = Some(page);
                    }
                    Some(cursor) => {
                        entry.older.retain(|(key, _)| key != cursor);
                        if entry.older.len() >= CACHED_OLDER_PAGES {
                            entry.older.remove(0);
                        }
                        entry.older.push((cursor.to_owned(), page));
                    }
                }
                Ok(value)
            }
        }
    }
    /// Number of cached threads; bounded by the thread cap.
    pub async fn cached_threads(&self) -> usize {
        self.entries.lock().await.len()
    }
}

async fn fetch(
    target: &CasTarget,
    thread_id: &str,
    cursor: Option<&str>,
    revision: Option<&str>,
) -> Result<Fetched> {
    let client = CasClient::new(target.clone())?;
    let status = client.thread_snapshot(thread_id, revision).await?;
    if !status.changed {
        ensure!(revision.is_some(), "cas_invalid_snapshot");
        return Ok(Fetched::Unchanged);
    }
    let items = client.thread_items(thread_id, cursor).await?;
    Ok(Fetched::Page(
        status.revision.clone(),
        normalize(thread_id, &status, &items, cursor.is_none()),
    ))
}

/// Collapse adapter errors to a closed set of reasons; remote text never leaves.
fn reason(error: &anyhow::Error) -> &'static str {
    match error.to_string().as_str() {
        "cas_transport_unavailable"
        | "cas_http_failure"
        | "cas_response_unavailable"
        | "cas_client_unavailable" => "cas_unavailable",
        "cas_action_failed" => "cas_action_failed",
        "cas_target_mismatch"
        | "cas_cwd_mismatch"
        | "cas_thread_mismatch"
        | "cas_operation_mismatch" => "cas_binding_mismatch",
        "cas_response_too_large" => "cas_response_too_large",
        _ => "cas_invalid_response",
    }
}
fn detail(reason: &str) -> &'static str {
    match reason {
        "cas_timeline_not_configured" => {
            "Codex observation is not configured for this repository on this server."
        }
        "cas_unavailable" => {
            "The configured Codex Action Server could not be reached. A later read will retry."
        }
        "cas_action_failed" => {
            "The Codex Action Server declined this read, for example because the thread is outside its configured workspace."
        }
        "cas_binding_mismatch" => {
            "Codex reported a different target, workspace or thread, so the observation was withheld."
        }
        "cas_response_too_large" => {
            "The Codex timeline page exceeded its size limit and was withheld."
        }
        _ => "Codex returned a timeline this server does not recognize, so it was withheld.",
    }
}
fn envelope(run_id: &str, thread_id: &str) -> serde_json::Map<String, Value> {
    let Value::Object(map) = json!({"schema_version":1,"run_id":run_id,"thread_id":thread_id,
        "source":SOURCE,"binding_verified":false,"execution_eligible":false})
    else {
        unreachable!("static object")
    };
    map
}
pub fn unavailable(run_id: &str, thread_id: &str, reason: &'static str) -> Value {
    let mut value = envelope(run_id, thread_id);
    value.extend(
        json!({"observed":false,"status":"unavailable","reason":reason,"detail":detail(reason),
            "events":[],"children":[],"omitted":0,"next_cursor":null,"freshness":null})
        .as_object()
        .cloned()
        .unwrap_or_default(),
    );
    Value::Object(value)
}
fn observed(run_id: &str, thread_id: &str, page: &Page, cache: &'static str) -> Value {
    let mut value = envelope(run_id, thread_id);
    value.extend(
        json!({"observed":true,"status":"observed","reason":null,"detail":null,
            "events":page.events,"children":page.children,"omitted":page.omitted,"next_cursor":page.next_cursor,
            "freshness":{"observed_at":page.observed_at,"age_seconds":page.fetched.elapsed().as_secs(),"cache":cache,
                "native_updated_at":page.native_updated_at,"thread_status":page.thread_status}})
        .as_object()
        .cloned()
        .unwrap_or_default(),
    );
    Value::Object(value)
}

fn normalize(thread_id: &str, status: &SnapshotObservation, items: &ItemsPage, head: bool) -> Page {
    let mut events = Vec::new();
    let mut children = BTreeSet::new();
    let mut omitted = 0;
    // Pages are newest first; walk oldest first so equal timestamps keep native order.
    for entry in items.entries.iter().rev() {
        let item = &entry["item"];
        if item["type"] == "collabAgentToolCall" && item["tool"] == "spawnAgent" {
            let parent = item["senderThreadId"]
                .as_str()
                .filter(|parent| valid_thread_id(parent))
                .unwrap_or(thread_id);
            for receiver in item["receiverThreadIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|receiver| valid_thread_id(receiver) && *receiver != parent)
            {
                if children.len() < MAX_CHILDREN {
                    children.insert(Child {
                        parent_thread: parent.into(),
                        receiver_thread: receiver.into(),
                    });
                }
            }
        }
        let at = native_time(entry.get("completedAtMs"), 1000)
            .or_else(|| native_time(entry.get("startedAtMs"), 1000));
        match (at, describe(item)) {
            (Some(at), Some((kind, summary))) => events.push(Event {
                at,
                kind,
                summary,
                thread_id: thread_id.into(),
            }),
            _ => omitted += 1,
        }
    }
    if head && let Some(at) = status.native_updated_at {
        for waiting in &status.waiting_on {
            events.push(Event {
                at,
                kind: "waiting",
                summary: if *waiting == "approval" {
                    "Waiting on a native approval in Codex"
                } else {
                    "Waiting on user input in Codex"
                }
                .into(),
                thread_id: thread_id.into(),
            });
        }
        if let Some(turn) = status
            .latest_turn
            .as_ref()
            .filter(|turn| turn.status == "failed")
        {
            events.push(Event {
                at,
                kind: "error",
                summary: format!(
                    "Latest turn failed: {}",
                    words(turn.error_code.as_deref().unwrap_or("unclassified"))
                ),
                thread_id: thread_id.into(),
            });
        }
    }
    events.sort_by_key(|event| event.at);
    if events.len() > MAX_EVENTS {
        omitted += events.len() - MAX_EVENTS;
        events.drain(..events.len() - MAX_EVENTS);
    }
    Page {
        fetched: Instant::now(),
        observed_at: crate::store::now(),
        thread_status: status.thread_status,
        native_updated_at: status.native_updated_at,
        events,
        children: children.into_iter().collect(),
        omitted,
        next_cursor: items.next_cursor.clone(),
    }
}

/// Map one native item to the redacted vocabulary. Reasoning, plans,
/// compaction, images, review markers and unknown types are dropped.
fn describe(item: &Value) -> Option<(&'static str, String)> {
    Some(match item["type"].as_str()? {
        "userMessage" => ("assigned", "Received instructions".into()),
        "agentMessage" => {
            if item["questions"].as_array().is_some_and(|q| !q.is_empty()) {
                ("waiting", "Asked for input".into())
            } else {
                ("message", message(item["text"].as_str().unwrap_or("")))
            }
        }
        "commandExecution" => command(item),
        "fileChange" => {
            let count = item["changes"].as_array().map_or(0, Vec::len);
            let files = if count == 1 {
                "1 file".to_owned()
            } else {
                format!("{} files", count.min(999))
            };
            let summary = match item["status"].as_str() {
                Some("declined") => format!("File change declined ({files})"),
                Some("failed") => format!("File change failed ({files})"),
                Some("inProgress") => format!("Changing {files}"),
                _ => format!("Changed {files}"),
            };
            ("file_change", summary)
        }
        "collabAgentToolCall" => {
            let count = item["receiverThreadIds"].as_array().map_or(0, Vec::len);
            match item["tool"].as_str() {
                Some("spawnAgent") => (
                    "subagent_spawned",
                    match count {
                        0 => "Requested a helper agent".into(),
                        1 => "Spawned a helper agent".into(),
                        n => format!("Spawned {} helper agents", n.min(99)),
                    },
                ),
                Some("wait") => ("waiting", "Waiting on helper agents".into()),
                Some("sendInput") => ("message", "Sent input to a helper agent".into()),
                Some("closeAgent") => ("message", "Closed a helper agent".into()),
                Some("resumeAgent") => ("message", "Resumed a helper agent".into()),
                _ => ("message", "Coordinated helper agents".into()),
            }
        }
        "mcpToolCall" | "dynamicToolCall" => {
            if item["status"] == "failed" {
                ("error", "A tool call failed".into())
            } else {
                ("message", "Used a tool".into())
            }
        }
        "webSearch" => ("message", "Searched the web".into()),
        _ => return None,
    })
}

/// Same wording as the shared `safe_summary` redaction.
const WITHHELD: &str = "Sensitive details withheld. Review the native owner thread locally.";
/// Credential shapes the shared markers miss: query tokens, Slack tokens, JWTs and AWS key IDs.
fn credential_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["token=", "xox"]
        .iter()
        .any(|marker| lower.contains(marker))
        || ["eyJ", "AKIA"].iter().any(|marker| text.contains(marker))
}
fn bounded(text: &str) -> String {
    if credential_marker(text) {
        return WITHHELD.into();
    }
    // Withhold every run of 32+ token characters, however punctuation splits the words.
    let token = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-');
    let mut text_out = String::new();
    let mut run = String::new();
    let flush = |out: &mut String, run: &mut String| {
        out.push_str(if run.len() >= 32 { "[withheld]" } else { run });
        run.clear();
    };
    for c in text.chars() {
        if token(c) {
            run.push(c);
        } else {
            flush(&mut text_out, &mut run);
            text_out.push(c);
        }
    }
    flush(&mut text_out, &mut run);
    let safe = safe_summary(&text_out, SUMMARY_LIMIT + 1);
    if safe.chars().count() > SUMMARY_LIMIT {
        safe.chars().take(SUMMARY_LIMIT - 1).chain(['…']).collect()
    } else {
        safe
    }
}
/// The first prose line of an agent message only; code blocks are never shown.
fn message(text: &str) -> String {
    if text.trim_start().starts_with("```") {
        return "Posted a message".into();
    }
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .trim_start_matches(['#', '*', '>', '-', '_', '`', ' '])
        .trim();
    if line.is_empty() {
        "Posted a message".into()
    } else {
        bounded(line)
    }
}
fn words(identifier: &str) -> String {
    let mut out = String::new();
    for c in identifier.chars().take(64) {
        if c.is_ascii_uppercase() && !out.is_empty() {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Program words that are safe to show: a bare name and at most two plain subcommands.
fn simple(word: &str, max: usize) -> bool {
    !word.is_empty()
        && word.len() <= max
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-' | b':'))
        && !word.starts_with('-')
}
fn segments(command: &str) -> Vec<Vec<String>> {
    let mut words: Vec<&str> = command.split_whitespace().take(256).collect();
    // Unwrap one shell layer such as `/bin/bash -lc 'cargo test'`.
    if words.len() >= 3
        && matches!(
            words[0].rsplit('/').next(),
            Some("bash" | "sh" | "zsh" | "dash")
        )
        && matches!(words[1], "-c" | "-lc" | "-ic")
    {
        words.drain(..2);
    }
    let joined = words.join(" ");
    joined
        .split("&&")
        .flat_map(|part| part.split(';'))
        .flat_map(|part| part.split("||"))
        .map(|part| {
            part.split_whitespace()
                .map(|word| {
                    word.trim_matches(|c: char| matches!(c, '\'' | '"' | '(' | ')'))
                        .to_owned()
                })
                .filter(|word| !word.is_empty())
                .skip_while(|word| {
                    matches!(word.as_str(), "env" | "sudo" | "time" | "command" | "exec")
                        || (word.contains('=') && !word.starts_with('-'))
                })
                .collect::<Vec<_>>()
        })
        .filter(|segment| {
            segment.first().is_some_and(|program| {
                !matches!(
                    program.as_str(),
                    "cd" | "export" | "source" | "." | "set" | "pushd"
                )
            })
        })
        .collect()
}
fn program(segment: &[String]) -> Option<String> {
    let name = segment.first()?.rsplit('/').next()?.to_owned();
    simple(&name, 32).then_some(name)
}
/// Closed allowlists of displayable subcommands. Anything else (targets, scripts,
/// pod names, branches, paths) could be private and is never shown.
fn safe_subcommand(tool: &str, word: &str) -> bool {
    let allowed: &[&str] = match tool {
        "cargo" => &[
            "build", "check", "clippy", "doc", "fmt", "nextest", "run", "test", "update",
        ],
        "git" => &[
            "add",
            "apply",
            "branch",
            "checkout",
            "cherry-pick",
            "clone",
            "commit",
            "diff",
            "fetch",
            "init",
            "log",
            "merge",
            "pull",
            "push",
            "rebase",
            "reset",
            "restore",
            "rev-parse",
            "rm",
            "show",
            "stash",
            "status",
            "switch",
            "tag",
            "worktree",
        ],
        "gh" => &["api", "auth", "issue", "pr", "release", "repo", "run"],
        "npm" | "pnpm" | "yarn" | "bun" => &[
            "add", "audit", "build", "ci", "exec", "install", "run", "test",
        ],
        "npx" => &["eslint", "jest", "playwright", "prettier", "tsc", "vitest"],
        "go" => &["build", "fmt", "generate", "mod", "run", "test", "vet"],
        "make" | "just" => &[
            "all", "build", "check", "clean", "fmt", "format", "lint", "test",
        ],
        "uv" | "poetry" => &["add", "build", "install", "lock", "pip", "run", "sync"],
        "pip" | "pip3" => &["freeze", "install", "list", "show"],
        "docker" | "podman" => &[
            "build", "compose", "exec", "images", "inspect", "logs", "ps", "pull", "push", "run",
        ],
        "kubectl" => &[
            "apply", "config", "delete", "describe", "exec", "get", "logs", "rollout",
        ],
        "rustup" => &["component", "show", "target", "toolchain", "update"],
        "deno" => &["check", "fmt", "lint", "run", "task", "test"],
        "python" | "python3" => &["-m"],
        _ => &[],
    };
    allowed.contains(&word)
}
fn safe_third(tool: &str, sub: &str, word: &str) -> bool {
    let allowed: &[&str] = match (tool, sub) {
        ("npm" | "pnpm" | "yarn" | "bun", "run") => &[
            "build",
            "check",
            "dev",
            "format",
            "lint",
            "test",
            "typecheck",
        ],
        ("uv" | "poetry", "run") => &["mypy", "pytest", "python", "ruff"],
        ("gh", "pr") => &[
            "checkout", "checks", "create", "diff", "edit", "list", "merge", "review", "status",
            "view",
        ],
        ("gh", "issue") => &["close", "comment", "create", "edit", "list", "view"],
        ("gh", "run") => &["list", "rerun", "view", "watch"],
        ("python" | "python3", "-m") => &[
            "black", "build", "mypy", "pip", "pytest", "ruff", "unittest", "venv",
        ],
        _ => &[],
    };
    allowed.contains(&word)
}
/// Programs whose name is safe to show. A script or binary name outside this list
/// (for example `./deploy-customer-x.sh`) could itself be private.
const KNOWN_PROGRAMS: [&str; 64] = [
    "apply_patch",
    "awk",
    "bash",
    "black",
    "bun",
    "cargo",
    "cat",
    "check",
    "chmod",
    "cp",
    "curl",
    "deno",
    "diff",
    "docker",
    "echo",
    "eslint",
    "fd",
    "find",
    "flake8",
    "gh",
    "git",
    "go",
    "grep",
    "head",
    "helm",
    "jest",
    "jq",
    "just",
    "kubectl",
    "ls",
    "make",
    "mkdir",
    "mv",
    "mypy",
    "node",
    "npm",
    "npx",
    "patch",
    "pip",
    "pip3",
    "pnpm",
    "podman",
    "poetry",
    "prettier",
    "printf",
    "pytest",
    "python",
    "python3",
    "rg",
    "rm",
    "ruff",
    "rustc",
    "rustup",
    "sed",
    "sh",
    "shellcheck",
    "sort",
    "tail",
    "tar",
    "tsc",
    "uv",
    "vitest",
    "wc",
    "yarn",
];
fn label(segment: &[String]) -> String {
    let Some(name) = program(segment).filter(|name| KNOWN_PROGRAMS.contains(&name.as_str())) else {
        return "a command".into();
    };
    let mut parts = vec![name.as_str()];
    if let Some(sub) = segment.get(1).filter(|word| safe_subcommand(&name, word)) {
        parts.push(sub);
        if let Some(third) = segment.get(2).filter(|word| safe_third(&name, sub, word)) {
            parts.push(third);
        }
    }
    parts.join(" ")
}
fn is(segment: &[String], name: &str, sub: &str) -> bool {
    program(segment).as_deref() == Some(name) && segment.get(1).map(String::as_str) == Some(sub)
}
fn is_check(segment: &[String]) -> bool {
    let Some(name) = program(segment) else {
        return false;
    };
    let sub = segment.get(1).map(String::as_str).unwrap_or("");
    let third = segment.get(2).map(String::as_str).unwrap_or("");
    match name.as_str() {
        "pytest" | "vitest" | "jest" | "tsc" | "ruff" | "mypy" | "eslint" | "flake8"
        | "shellcheck" | "check" => true,
        "cargo" => matches!(
            sub,
            "test" | "clippy" | "fmt" | "check" | "nextest" | "build"
        ),
        "go" => matches!(sub, "test" | "vet" | "build"),
        "make" => matches!(sub, "test" | "check" | "lint"),
        "npx" => matches!(sub, "vitest" | "jest" | "tsc" | "eslint"),
        "npm" | "pnpm" | "yarn" | "bun" => {
            sub == "test"
                || (sub == "run"
                    && matches!(third, "test" | "typecheck" | "lint" | "check" | "build"))
        }
        "python" | "python3" => {
            sub == "-m" && matches!(third, "pytest" | "unittest" | "mypy" | "ruff")
        }
        _ => false,
    }
}
/// Classify a native command from its program words only. Arguments, paths,
/// environment values and output are never shown.
fn command(item: &Value) -> (&'static str, String) {
    let segments = segments(item["command"].as_str().unwrap_or(""));
    let shown = segments
        .last()
        .map_or_else(|| "a command".into(), |segment| label(segment));
    let exit = item["exitCode"].as_i64();
    let status = item["status"].as_str().unwrap_or("");
    let exit_text = exit.map_or_else(|| "exit unknown".to_owned(), |code| format!("exit {code}"));
    let passed = status == "completed" && exit == Some(0);
    let failed = status == "failed" || exit.is_some_and(|code| code != 0);
    match status {
        "declined" => return ("command", format!("Declined {shown}")),
        "inProgress" => return ("command", format!("Running {shown}")),
        "completed" | "failed" => {}
        _ => return ("command", format!("Ran {shown} (status unknown)")),
    }
    if segments.iter().any(|segment| is(segment, "git", "commit")) {
        return if passed {
            ("commit", "Committed changes with git commit".into())
        } else {
            (
                "command",
                format!("git commit did not complete ({exit_text})"),
            )
        };
    }
    if segments.iter().any(|segment| {
        is(segment, "gh", "pr") && segment.get(2).map(String::as_str) == Some("create")
    }) {
        return if passed {
            (
                "pr_opened",
                "Opened a pull request with gh pr create".into(),
            )
        } else {
            (
                "command",
                format!("gh pr create did not complete ({exit_text})"),
            )
        };
    }
    if segments.last().is_some_and(|segment| is_check(segment)) {
        return (
            "check_result",
            if passed {
                format!("Check passed: {shown}")
            } else if failed {
                format!("Check failed: {shown} ({exit_text})")
            } else {
                format!("Check finished: {shown} ({exit_text})")
            },
        );
    }
    ("command", format!("Ran {shown} ({exit_text})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cmd(command: &str, status: &str, exit: Option<i64>) -> (&'static str, String) {
        command_item(
            json!({"type":"commandExecution","command":command,"status":status,"exitCode":exit,"aggregatedOutput":"private-output"}),
        )
    }
    fn command_item(item: Value) -> (&'static str, String) {
        describe(&item).unwrap()
    }
    #[test]
    fn commands_classify_from_program_words_without_arguments_or_output() {
        assert_eq!(
            cmd("/bin/bash -lc 'cargo test --locked'", "completed", Some(0)),
            ("check_result", "Check passed: cargo test".into())
        );
        assert_eq!(
            cmd("cd /work/repo && npm run typecheck", "completed", Some(2)),
            (
                "check_result",
                "Check failed: npm run typecheck (exit 2)".into()
            )
        );
        assert_eq!(
            cmd("git commit -m 'fix secret-sentinel'", "completed", Some(0)),
            ("commit", "Committed changes with git commit".into())
        );
        assert_eq!(
            cmd("gh pr create --title x --body y", "completed", Some(0)),
            (
                "pr_opened",
                "Opened a pull request with gh pr create".into()
            )
        );
        assert_eq!(
            cmd("gh pr create --title x", "failed", Some(1)),
            ("command", "gh pr create did not complete (exit 1)".into())
        );
        assert_eq!(
            cmd(
                "TOKEN=ghp_secret curl -H x https://example.invalid",
                "completed",
                Some(0)
            ),
            ("command", "Ran curl (exit 0)".into())
        );
        assert_eq!(
            cmd("python3 -m pytest tests", "inProgress", None),
            ("command", "Running python3 -m pytest".into())
        );
        assert_eq!(
            cmd("/tmp/$(cat secret)", "declined", None),
            ("command", "Declined a command".into())
        );
        for (_, summary) in [
            cmd("rm -rf /private/path", "completed", Some(0)),
            cmd("echo private-output", "completed", Some(0)),
            cmd("python3 private.py", "completed", Some(0)),
        ] {
            assert!(!summary.contains("private"), "{summary}");
        }
    }
    #[test]
    fn labels_show_only_allowlisted_programs_and_subcommands() {
        for (command, shown) in [
            ("uv run private.py", "Ran uv run (exit 0)"),
            ("kubectl exec prod-db-0 -- sh", "Ran kubectl exec (exit 0)"),
            ("make deploy-customer-x", "Ran make (exit 0)"),
            ("npm run deploy-secret", "Ran npm run (exit 0)"),
            ("git checkout private-branch", "Ran git checkout (exit 0)"),
            ("gh pr view 123", "Ran gh pr view (exit 0)"),
            (
                "docker run registry.invalid/private:1.2",
                "Ran docker run (exit 0)",
            ),
            ("/usr/bin/git status --short", "Ran git status (exit 0)"),
            ("./deploy-customer-x.sh --prod", "Ran a command (exit 0)"),
            ("private-tool sub", "Ran a command (exit 0)"),
            ("python3 -m http.server 8000", "Ran python3 -m (exit 0)"),
        ] {
            assert_eq!(
                cmd(command, "completed", Some(0)),
                ("command", shown.into()),
                "{command}"
            );
        }
        assert_eq!(
            cmd("uv run pytest -q", "completed", Some(0)).1,
            "Ran uv run pytest (exit 0)"
        );
    }
    #[test]
    fn credential_shapes_are_withheld_even_inside_punctuation() {
        for text in [
            "Use key `AKIAABCDEFGHIJKLMNOP`.",
            "Bearer-less eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJlLXNlbnRpbmVs works",
            "Fetched https://h.invalid/x?token=abc123&y=1 fine",
            "Slack xoxb-1234-5678-abcdef posted",
        ] {
            assert_eq!(message(text), WITHHELD, "{text}");
        }
        let opaque = "a".repeat(20) + "Z9" + &"b".repeat(20);
        for text in [
            format!("key `{opaque}`."),
            format!("value \"{opaque}\", then more"),
            format!("path /{opaque}/x"),
            format!("dotted {opaque}.{opaque}"),
        ] {
            let summary = message(&text);
            assert!(
                summary.contains("[withheld]") && !summary.contains(&opaque),
                "{text} -> {summary}"
            );
        }
        assert_eq!(
            message("short words stay visible."),
            "short words stay visible."
        );
    }
    #[test]
    fn messages_are_single_line_bounded_and_redacted() {
        let long = "word ".repeat(100);
        let summary = message(&long);
        assert!(summary.chars().count() <= SUMMARY_LIMIT && summary.ends_with('…'));
        assert_eq!(message("```rust\nfn private() {}\n```"), "Posted a message");
        assert_eq!(message("## Plan\nsecond line private"), "Plan");
        assert!(message("Using sk-live-secret now").contains("Sensitive details withheld"));
        assert_eq!(
            message("token AbCdEfGhIjKlMnOpQrStUvWxYz0123456789 here"),
            "token [withheld] here"
        );
        assert_eq!(words("rateLimitExceeded"), "rate limit exceeded");
    }
    #[test]
    fn spawn_items_derive_children_and_unknown_items_are_dropped() {
        let status = SnapshotObservation {
            revision: "a".repeat(64),
            changed: true,
            thread_status: "active",
            waiting_on: vec!["approval"],
            native_updated_at: Some(1_760_000_900),
            latest_turn: None,
        };
        let items = ItemsPage {
            entries: vec![
                json!({"turnId":"t","completedAtMs":1_760_000_100_000_u64,"item":{"id":"c","type":"collabAgentToolCall","tool":"spawnAgent","senderThreadId":"owner","receiverThreadIds":["child-1","child-2"],"prompt":"private-prompt","agentsStates":{}}}),
                json!({"turnId":"t","completedAtMs":1_760_000_050_000_u64,"item":{"id":"r","type":"reasoning","summary":["private-reasoning"]}}),
                json!({"turnId":"t","item":{"id":"u","type":"agentMessage","text":"untimed"}}),
            ],
            next_cursor: Some("older".into()),
        };
        let page = normalize("owner", &status, &items, true);
        assert_eq!(
            page.children,
            vec![
                Child {
                    parent_thread: "owner".into(),
                    receiver_thread: "child-1".into()
                },
                Child {
                    parent_thread: "owner".into(),
                    receiver_thread: "child-2".into()
                }
            ]
        );
        assert_eq!(
            page.events
                .iter()
                .map(|e| (e.at, e.kind))
                .collect::<Vec<_>>(),
            vec![
                (1_760_000_100, "subagent_spawned"),
                (1_760_000_900, "waiting")
            ]
        );
        assert_eq!(page.omitted, 2);
        let text = serde_json::to_string(&page.events).unwrap();
        assert!(!text.contains("private"));
        assert!(page.events.iter().all(|event| KINDS.contains(&event.kind)));
    }
}
