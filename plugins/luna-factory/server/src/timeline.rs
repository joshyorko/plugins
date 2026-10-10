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
const CACHED_THREADS: usize = 64;
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
            let mut entries = self.entries.lock().await;
            let entry = entries.entry(key.clone()).or_default();
            entry.used = Some(now);
            if let Some(cursor) = cursor {
                ensure!(entry.issued(cursor), "timeline_cursor_not_issued");
            }
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
        };
        let outcome = fetch(target, thread_id, cursor, revision.as_deref()).await;
        let mut entries = self.entries.lock().await;
        if entries.len() >= CACHED_THREADS
            && !entries.contains_key(&key)
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
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
                entry.failure = None;
                let Some(mut page) = prior else {
                    entry.failure = Some((Instant::now(), "cas_invalid_response"));
                    return Ok(unavailable(run_id, thread_id, "cas_invalid_response"));
                };
                page.fetched = Instant::now();
                page.observed_at = crate::store::now();
                let value = observed(run_id, thread_id, &page, "revalidated");
                entry.head = Some(page);
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

fn bounded(text: &str) -> String {
    // Long opaque tokens look like credentials; withhold them before the shared redaction.
    let text = text
        .split_whitespace()
        .map(|word| {
            if word.len() >= 32
                && word.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-')
                })
            {
                "[withheld]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let safe = safe_summary(&text, SUMMARY_LIMIT + 1);
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
/// Only these tools show a subcommand; for anything else an argument could be private.
const SUBCOMMAND_TOOLS: [&str; 20] = [
    "cargo", "git", "gh", "npm", "pnpm", "yarn", "bun", "npx", "go", "make", "uv", "pip", "pip3",
    "docker", "podman", "kubectl", "rustup", "deno", "just", "poetry",
];
fn label(segment: &[String]) -> String {
    let Some(name) = program(segment) else {
        return "a command".into();
    };
    let mut parts = vec![name.clone()];
    let next = segment.get(1).map(String::as_str);
    if matches!(name.as_str(), "python" | "python3") {
        // Only `python -m <module>` is shown; a script path could be private.
        if next == Some("-m")
            && let Some(module) = segment.get(2).filter(|word| simple(word, 24))
        {
            parts.extend(["-m".to_owned(), module.clone()]);
        }
        return parts.join(" ");
    }
    if !SUBCOMMAND_TOOLS.contains(&name.as_str()) {
        return name;
    }
    if let Some(sub) = next.filter(|word| simple(word, 24)) {
        parts.push(sub.into());
        if matches!(sub, "run" | "exec" | "pr" | "x")
            && let Some(third) = segment.get(2).filter(|word| simple(word, 24))
        {
            parts.push(third.clone());
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
