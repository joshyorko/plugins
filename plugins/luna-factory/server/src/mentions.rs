//! Bounded, local-only composer mention results and revision-fenced resources.

use anyhow::{Context, Result, bail, ensure};
use axum::http::Uri;
use serde_json::{Value, json};

const MAX_QUERY_CHARS: usize = 160;
const MAX_RESULTS: usize = 12;
const MAX_RUNS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionRef {
    pub run_id: String,
    pub revision: u64,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Match {
    score: u8,
    run_id: String,
    task_id: Option<String>,
    revision: u64,
    name: String,
    description: String,
}

/// Search the same bounded run projection available to the local Factory tools.
/// This function reads JSON projections only; it has no native client or dispatch access.
pub fn search_items(runs: &[Value], query: &str) -> Result<Value> {
    ensure!(
        query.chars().count() <= MAX_QUERY_CHARS,
        "mention_query_too_long"
    );
    let query = query.trim().to_lowercase();
    let mut matches = Vec::new();
    for run in runs.iter().take(MAX_RUNS) {
        let Some(run_id) = bounded_string(run.get("id"), 64) else {
            continue;
        };
        let Some(revision) = run
            .get("control")
            .and_then(|control| control.get("revision"))
            .and_then(Value::as_u64)
        else {
            continue;
        };
        let state = bounded_string(run.get("state"), 64).unwrap_or_else(|| "UNKNOWN".into());
        let objective = bounded_string(run.get("objective"), 1000).unwrap_or_default();
        let blocker = bounded_string(run.get("blocker"), 1000).unwrap_or_default();
        let remaining = bounded_string(run.get("remaining_gap"), 1000).unwrap_or_default();
        let delta = bounded_string(run.get("delta"), 1000).unwrap_or_default();
        let summary = format!("{state} {objective} {blocker} {remaining} {delta}");
        if let Some(score) = match_score(&query, &run_id, &summary, true) {
            matches.push(Match {
                score,
                run_id: run_id.clone(),
                task_id: None,
                revision,
                name: format!("Run {run_id}"),
                description: bounded_description(&format!(
                    "{state} · {}",
                    first_nonempty(&[&blocker, &remaining, &objective, &delta])
                )),
            });
        }
        if query.is_empty() {
            continue;
        }
        if let Some(tasks) = run
            .get("control")
            .and_then(|control| control.get("tasks"))
            .and_then(Value::as_array)
        {
            for task in tasks.iter().take(128) {
                let Some(task_id) = bounded_string(task.get("id"), 256) else {
                    continue;
                };
                let title = bounded_string(task.get("title"), 1000).unwrap_or_default();
                let task_state = bounded_string(task.get("state"), 64).unwrap_or_default();
                let haystack = format!("{title} {task_state} {objective} {blocker} {remaining}");
                let Some(score) = match_score(&query, &task_id, &haystack, false) else {
                    continue;
                };
                matches.push(Match {
                    score: score.saturating_add(1),
                    run_id: run_id.clone(),
                    task_id: Some(task_id.clone()),
                    revision,
                    name: format!("Task {task_id} · {}", bounded_description(&title)),
                    description: bounded_description(&format!(
                        "Run {run_id} · {state} · {task_state}"
                    )),
                });
            }
        }
    }
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.run_id.cmp(&right.run_id))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    let items = matches
        .into_iter()
        .take(MAX_RESULTS)
        .map(|item| {
            let reference = MentionRef {
                run_id: item.run_id,
                revision: item.revision,
                task_id: item.task_id,
            };
            json!({
                "type":"resource_link",
                "uri": mention_uri(&reference),
                "name": item.name.chars().take(180).collect::<String>(),
                "description": item.description,
                "mimeType":"application/json"
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"items":items}))
}

/// Parse only this server's exact run/task reference format.
pub fn parse_mention_uri(uri: &str) -> Result<MentionRef> {
    let parsed: Uri = uri.parse().context("invalid_mention_uri")?;
    ensure!(
        parsed.scheme_str() == Some("luna-factory"),
        "invalid_mention_uri"
    );
    ensure!(
        parsed
            .authority()
            .is_some_and(|authority| authority.as_str() == "mention"),
        "invalid_mention_uri"
    );
    let path = parsed.path();
    let encoded_run_id = path.strip_prefix('/').context("invalid_mention_uri")?;
    ensure!(
        !encoded_run_id.is_empty() && !encoded_run_id.contains('/'),
        "invalid_mention_uri"
    );
    let run_id = decode_component(encoded_run_id)?;
    ensure!(valid_id(&run_id, 64), "invalid_mention_uri");
    let mut revision = None;
    let mut task_id = None;
    for pair in parsed.query().unwrap_or_default().split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').context("invalid_mention_uri")?;
        match key {
            "revision" if revision.is_none() => {
                revision = Some(value.parse::<u64>().context("invalid_mention_uri")?);
            }
            "task" if task_id.is_none() => {
                let decoded = decode_component(value)?;
                ensure!(valid_id(&decoded, 256), "invalid_mention_uri");
                task_id = Some(decoded);
            }
            _ => bail!("invalid_mention_uri"),
        }
    }
    Ok(MentionRef {
        run_id,
        revision: revision.context("invalid_mention_uri")?,
        task_id,
    })
}

/// Return a model-visible projection only when the mention identity and revision remain current.
pub fn resource_payload(run: &Value, mention: &MentionRef) -> Result<Value> {
    ensure!(
        run.get("id").and_then(Value::as_str) == Some(mention.run_id.as_str()),
        "mention_identity_mismatch"
    );
    let revision = run
        .get("control")
        .and_then(|control| control.get("revision"))
        .and_then(Value::as_u64)
        .context("mention_state_unavailable")?;
    ensure!(revision == mention.revision, "stale_mention_revision");
    let mut payload = json!({
        "identity":{"run_id":mention.run_id,"revision":revision},
        "state":bounded_string(run.get("state"),64).unwrap_or_else(||"UNKNOWN".into()),
        "objective":bounded_string(run.get("objective"),1000).unwrap_or_default(),
        "blocker":nullable_text(run.get("blocker"),1000),
        "remaining_gap":nullable_text(run.get("remaining_gap"),1000),
        "delta":nullable_text(run.get("delta"),1000)
    });
    if let Some(task_id) = &mention.task_id {
        let tasks = run
            .get("control")
            .and_then(|control| control.get("tasks"))
            .and_then(Value::as_array)
            .context("mention_task_unavailable")?;
        let task = tasks
            .iter()
            .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id.as_str()))
            .context("mention_task_unavailable")?;
        payload["task"] = json!({
            "id":task_id,
            "title":bounded_string(task.get("title"),1000).unwrap_or_default(),
            "state":bounded_string(task.get("state"),64).unwrap_or_default(),
            "dependencies":task.get("dependencies").and_then(Value::as_array).map(|items|items.iter().filter_map(Value::as_str).take(128).collect::<Vec<_>>()).unwrap_or_default()
        });
    }
    Ok(payload)
}

pub fn mention_uri(reference: &MentionRef) -> String {
    let task = reference
        .task_id
        .as_ref()
        .map(|id| format!("&task={}", encode_component(id)))
        .unwrap_or_default();
    format!(
        "luna-factory://mention/{}?revision={}{}",
        encode_component(&reference.run_id),
        reference.revision,
        task
    )
}

fn match_score(query: &str, id: &str, text: &str, allow_empty: bool) -> Option<u8> {
    if query.is_empty() {
        return allow_empty.then_some(8);
    }
    let folded_id = id.to_lowercase();
    let folded_text = text.to_lowercase();
    if folded_id == query {
        return Some(0);
    }
    if folded_id.starts_with(query) {
        return Some(1);
    }
    if folded_text.contains(query) {
        return Some(2);
    }
    let tokens = query.split_whitespace().collect::<Vec<_>>();
    if !tokens.is_empty()
        && tokens.iter().all(|token| {
            folded_id.contains(token)
                || folded_text.contains(token)
                || (token.chars().count() >= 4
                    && folded_text
                        .split(|character: char| !character.is_alphanumeric())
                        .any(|word| distance_at_most_one(token, word)))
        })
    {
        return Some(3);
    }
    None
}

fn distance_at_most_one(left: &str, right: &str) -> bool {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    let (shorter, longer) = if left.len() <= right.len() {
        (&left, &right)
    } else {
        (&right, &left)
    };
    let mut short_index = 0;
    let mut long_index = 0;
    let mut differences = 0;
    while short_index < shorter.len() && long_index < longer.len() {
        if shorter[short_index] == longer[long_index] {
            short_index += 1;
            long_index += 1;
            continue;
        }
        differences += 1;
        if differences > 1 {
            return false;
        }
        if shorter.len() == longer.len() {
            short_index += 1;
        }
        long_index += 1;
    }
    differences + usize::from(long_index < longer.len()) <= 1
}

fn first_nonempty<'a>(values: &'a [&'a str]) -> &'a str {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .copied()
        .unwrap_or("No additional status is available.")
}

fn bounded_description(value: &str) -> String {
    value.chars().take(240).collect()
}

fn bounded_string(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(max).collect())
}

fn nullable_text(value: Option<&Value>, max: usize) -> Value {
    match bounded_string(value, max) {
        Some(text) => Value::String(text),
        None => Value::Null,
    }
}

fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn encode_component(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}

fn decode_component(value: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = chars
                .next()
                .and_then(hex_value)
                .context("invalid_mention_uri")?;
            let low = chars
                .next()
                .and_then(hex_value)
                .context("invalid_mention_uri")?;
            bytes.push((high << 4) | low);
        } else {
            ensure!(
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'),
                "invalid_mention_uri"
            );
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).context("invalid_mention_uri")
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
