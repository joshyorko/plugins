//! Read-only Codex Action Server adapter. A scoped observation is not execution
//! qualification, completion proof, or permission to release a Factory claim.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, net::IpAddr, time::Duration};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
const CREATE_OPERATION: &str = "create_thread_and_start_turn";
/// The complete set of CAS actions this adapter may call. Every one is a read.
pub const READ_ACTIONS: [&str; 7] = [
    "inspect-target",
    "read-dispatch-receipt",
    "read-thread",
    "get-thread-snapshot",
    "list-thread-items",
    "list-thread-turns",
    "list-thread-timeline",
];
/// One client serves one tool call; it never makes more requests than this.
pub const MAX_REQUESTS_PER_CALL: usize = 3;
/// Native page size for item, turn and timeline reads (CAS accepts 1..=100).
pub const PAGE_LIMIT: usize = 100;
const MAX_CURSOR_BYTES: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CasTarget {
    pub endpoint: String,
    pub package: String,
    pub target: String,
    pub cwd: String,
}
impl CasTarget {
    pub fn validate(&self) -> Result<()> {
        endpoint(&self.endpoint)?;
        ensure!(
            matches!(
                self.package.as_str(),
                "codex-action-server" | "codex-observe"
            ),
            "cas_package_not_readable"
        );
        ensure!(
            bounded(&self.target, 128) && !self.target.trim().is_empty(),
            "cas_invalid_target"
        );
        ensure!(
            bounded(&self.cwd, 512)
                && self.cwd.starts_with('/')
                && !self.cwd.split('/').any(|part| matches!(part, "." | "..")),
            "cas_invalid_cwd"
        );
        Ok(())
    }
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn endpoint(value: &str) -> Result<reqwest::Url> {
    ensure!(value.len() <= 2048, "cas_invalid_endpoint");
    let tail = value
        .strip_prefix("http://")
        .context("cas_loopback_http_required")?;
    let (authority, path) = tail.split_once('/').unwrap_or((tail, ""));
    ensure!(
        path.is_empty() && !authority.contains('@'),
        "cas_endpoint_origin_required"
    );
    // Validate the literal spelling before URL normalization (which accepts
    // decimal/octal/short IPv4 spellings and would otherwise turn them local).
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']').context("cas_invalid_endpoint")?;
        ensure!(
            suffix.is_empty()
                || suffix.strip_prefix(':').is_some_and(
                    |port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
                ),
            "cas_invalid_endpoint"
        );
        host
    } else {
        authority
            .split_once(':')
            .map_or(authority, |(host, _)| host)
    };
    let ip: IpAddr = host
        .parse()
        .map_err(|_| anyhow::anyhow!("cas_literal_loopback_required"))?;
    ensure!(ip.is_loopback(), "cas_literal_loopback_required");
    let url = reqwest::Url::parse(value).map_err(|_| anyhow::anyhow!("cas_invalid_endpoint"))?;
    ensure!(
        url.scheme() == "http"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/"
            && url.port_or_known_default().is_some_and(|port| port > 0),
        "cas_invalid_endpoint"
    );
    Ok(url)
}

/// A ledger preparation only. There is deliberately no adapter dispatch method.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparedDispatch {
    pub operation: String,
    pub request_id: String,
    pub target: CasTarget,
    pub payload_sha256: String,
}
impl PreparedDispatch {
    pub fn new(target: CasTarget, payload: Value) -> Result<Self> {
        target.validate()?;
        let payload: CreatePlanPayload = serde_json::from_value(payload)
            .map_err(|_| anyhow::anyhow!("cas_invalid_create_payload"))?;
        payload.validate(&target)?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut payload = serde_json::to_value(payload)
            .map_err(|_| anyhow::anyhow!("cas_invalid_create_payload"))?;
        payload["request_id"] = json!(request_id);
        let mut nodes = 0;
        let canonical = canonical(
            json!({"operation":CREATE_OPERATION,"payload":payload}),
            0,
            &mut nodes,
        )?;
        let bytes =
            serde_json::to_vec(&canonical).map_err(|_| anyhow::anyhow!("cas_invalid_payload"))?;
        ensure!(bytes.len() <= MAX_PAYLOAD_BYTES, "cas_payload_too_large");
        Ok(Self {
            operation: CREATE_OPERATION.into(),
            request_id,
            target,
            payload_sha256: format!("{:x}", Sha256::digest(bytes)),
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.operation == CREATE_OPERATION,
            "cas_invalid_prepared_operation"
        );
        self.target.validate()?;
        let id = uuid::Uuid::parse_str(&self.request_id)
            .map_err(|_| anyhow::anyhow!("cas_invalid_request_id"))?;
        ensure!(
            id.to_string() == self.request_id && id.get_version_num() == 4,
            "cas_invalid_request_id"
        );
        ensure!(
            self.payload_sha256.len() == 64
                && self
                    .payload_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "cas_invalid_payload_digest"
        );
        Ok(())
    }
}
/// Only the inspected CAS create action's exact payload is preparable. A caller
/// cannot choose another action, key, namespace or callback/wait behavior.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePlanPayload {
    target: String,
    cwd: String,
    text: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    wait_for_completion: bool,
    #[serde(default = "default_wait_seconds")]
    wait_seconds: f64,
    #[serde(default)]
    enable_list_threads_callback: bool,
}
fn default_wait_seconds() -> f64 {
    15.0
}
impl CreatePlanPayload {
    fn validate(&self, target: &CasTarget) -> Result<()> {
        ensure!(
            self.target == target.target && self.cwd == target.cwd,
            "cas_payload_binding_mismatch"
        );
        ensure!(
            !self.text.trim().is_empty() && self.text.len() <= MAX_PAYLOAD_BYTES,
            "cas_invalid_plan_text"
        );
        ensure!(
            !self.wait_for_completion
                && !self.enable_list_threads_callback
                && self.wait_seconds.is_finite()
                && self.wait_seconds > 0.0
                && self.wait_seconds <= 30.0,
            "cas_plan_wait_or_callback_unsupported"
        );
        ensure!(
            [
                (&self.model, 256),
                (&self.model_provider, 256),
                (&self.effort, 64)
            ]
            .iter()
            .all(|(value, limit)| value.as_ref().is_none_or(
                |value| bounded(value, *limit) && !value.chars().any(char::is_whitespace)
            )),
            "cas_invalid_plan_settings"
        );
        Ok(())
    }
}
fn canonical(value: Value, depth: usize, nodes: &mut usize) -> Result<Value> {
    *nodes += 1;
    ensure!(depth <= 32 && *nodes <= 4096, "cas_payload_bound_exceeded");
    Ok(match value {
        Value::Object(object) => {
            let sorted: BTreeMap<_, _> = object.into_iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| Ok((key, canonical(value, depth + 1, nodes)?)))
                    .collect::<Result<_>>()?,
            )
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| canonical(value, depth + 1, nodes))
                .collect::<Result<_>>()?,
        ),
        other => other,
    })
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Qualification {
    pub execution_eligible: bool,
    pub blockers: Vec<&'static str>,
}
pub fn qualification() -> Qualification {
    Qualification {
        execution_eligible: false,
        blockers: vec![
            "subscription_entitlement_unverified",
            "durable_callback_owner_unavailable",
            "workspace_and_repository_identity_unverified",
            "canonical_skill_input_unavailable",
            "structured_owner_output_schema_unavailable",
            "receipt_payload_binding_unavailable",
            "remote_evidence_and_cessation_unqualified",
        ],
    }
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TargetInspection {
    pub target: String,
    pub status: String,
    pub binding_verified: bool,
    pub execution_eligible: bool,
    pub reason: &'static str,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReceiptObservation {
    pub request_id: String,
    pub state: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub binding_verified: bool,
    pub execution_eligible: bool,
    pub reason: &'static str,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThreadObservation {
    pub target: String,
    pub cwd: String,
    pub thread_id: String,
    pub liveness: &'static str,
    pub execution_eligible: bool,
    pub reason: &'static str,
}

/// Bounded native status for one exact `{target, cwd, thread_id}`. The latest
/// item's text is deliberately never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotObservation {
    pub revision: String,
    pub changed: bool,
    pub thread_status: &'static str,
    pub waiting_on: Vec<&'static str>,
    pub native_updated_at: Option<u64>,
    pub latest_turn: Option<TurnObservation>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnObservation {
    pub id: String,
    pub status: &'static str,
    pub error_code: Option<String>,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}
/// Native item entries stay inside the adapter boundary; callers normalize
/// only bounded metadata from them and never forward the raw objects.
#[derive(Debug, Clone)]
pub struct ItemsPage {
    pub entries: Vec<Value>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone)]
pub struct TurnsPage {
    pub turns: Vec<TurnObservation>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineMarker {
    pub kind: &'static str,
    pub position: i64,
    pub turn_id: Option<String>,
    pub at: Option<u64>,
}
#[derive(Debug, Clone)]
pub struct TimelinePage {
    pub markers: Vec<TimelineMarker>,
    pub next_cursor: Option<String>,
}

pub struct CasClient {
    target: CasTarget,
    client: reqwest::Client,
    requests: std::sync::atomic::AtomicUsize,
}
impl CasClient {
    pub fn new(target: CasTarget) -> Result<Self> {
        target.validate()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| anyhow::anyhow!("cas_client_unavailable"))?;
        Ok(Self {
            target,
            client,
            requests: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    /// The exact binding always comes from operator configuration, never a caller.
    fn scope(&self, thread_id: &str) -> Result<serde_json::Map<String, Value>> {
        ensure!(bounded(thread_id, 512), "cas_invalid_thread_id");
        let Value::Object(scope) =
            json!({"target":self.target.target,"cwd":self.target.cwd,"thread_id":thread_id})
        else {
            unreachable!("static object")
        };
        Ok(scope)
    }
    pub async fn thread_snapshot(
        &self,
        thread_id: &str,
        revision: Option<&str>,
    ) -> Result<SnapshotObservation> {
        let mut payload = self.scope(thread_id)?;
        if let Some(revision) = revision {
            ensure!(hex_digest(revision), "cas_invalid_revision");
            payload.insert("revision".into(), json!(revision));
        }
        let result = self
            .read("get-thread-snapshot", Value::Object(payload))
            .await?;
        snapshot(&self.target, thread_id, &result)
    }
    /// Newest-first item page; `cursor` continues toward older items.
    pub async fn thread_items(&self, thread_id: &str, cursor: Option<&str>) -> Result<ItemsPage> {
        let result = self
            .read("list-thread-items", self.page(thread_id, cursor, true)?)
            .await?;
        items(&self.target, &result)
    }
    pub async fn thread_turns(&self, thread_id: &str, cursor: Option<&str>) -> Result<TurnsPage> {
        let mut payload = self.page(thread_id, cursor, true)?;
        payload["items_view"] = json!("notLoaded");
        let result = self.read("list-thread-turns", payload).await?;
        turns(&self.target, &result)
    }
    pub async fn thread_timeline(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
    ) -> Result<TimelinePage> {
        let result = self
            .read("list-thread-timeline", self.page(thread_id, cursor, false)?)
            .await?;
        timeline(&self.target, &result)
    }
    fn page(&self, thread_id: &str, cursor: Option<&str>, sorted: bool) -> Result<Value> {
        let mut payload = self.scope(thread_id)?;
        payload.insert("limit".into(), json!(PAGE_LIMIT));
        if sorted {
            payload.insert("sort_direction".into(), json!("desc"));
        }
        if let Some(cursor) = cursor {
            ensure!(valid_cursor(cursor), "cas_invalid_cursor");
            payload.insert("cursor".into(), json!(cursor));
        }
        Ok(Value::Object(payload))
    }
    pub async fn inspect(&self) -> Result<TargetInspection> {
        let result = self
            .read("inspect-target", json!({"target":self.target.target}))
            .await?;
        inspection(&self.target, &result)
    }
    pub async fn read_receipt(&self, request_id: &str) -> Result<ReceiptObservation> {
        ensure!(request_key(request_id), "cas_invalid_request_id");
        let result = self
            .read("read-dispatch-receipt", json!({"request_id":request_id}))
            .await?;
        receipt(request_id, &result)
    }
    pub async fn read_thread(&self, thread_id: &str) -> Result<ThreadObservation> {
        ensure!(bounded(thread_id, 512), "cas_invalid_thread_id");
        let result = self.read("read-thread", json!({"target":self.target.target,"cwd":self.target.cwd,"thread_id":thread_id,"include_turns":false})).await?;
        thread(&self.target, thread_id, &result)
    }
    async fn read(&self, action: &str, payload: Value) -> Result<Value> {
        // The action string is selected only by the read-only methods above.
        ensure!(READ_ACTIONS.contains(&action), "cas_action_not_allowed");
        ensure!(
            self.requests
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                < MAX_REQUESTS_PER_CALL,
            "cas_request_budget_exhausted"
        );
        let mut url = endpoint(&self.target.endpoint)?;
        url.set_path(&format!(
            "/api/actions/{}/{action}/run",
            self.target.package
        ));
        let mut response = self
            .client
            .post(url)
            .json(&json!({"payload":payload}))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("cas_transport_unavailable"))?;
        ensure!(response.status().is_success(), "cas_http_failure");
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= MAX_RESPONSE_BYTES as u64),
            "cas_response_too_large"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("cas_response_unavailable"))?
        {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
                "cas_response_too_large"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("cas_invalid_response"))?;
        outer(value)
    }
}
fn request_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}
fn object(value: &Value) -> Result<&serde_json::Map<String, Value>> {
    value.as_object().context("cas_invalid_response")
}
fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    ensure!(
        object(value)?
            .keys()
            .all(|key| allowed.contains(&key.as_str())),
        "cas_unknown_response_field"
    );
    Ok(())
}
fn required_text<'a>(value: &'a Value, name: &str, maximum: usize) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| bounded(text, maximum))
        .context("cas_invalid_response")
}
fn optional_text(value: &Value, name: &str, maximum: usize) -> Result<Option<String>> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if bounded(text, maximum) => Ok(Some(text.clone())),
        _ => bail!("cas_invalid_response"),
    }
}
fn outer(value: Value) -> Result<Value> {
    fields(&value, &["result", "error"])?;
    ensure!(
        value.get("result").is_some() && value.get("error").is_some(),
        "cas_invalid_response_envelope"
    );
    if !value["error"].is_null() {
        bail!("cas_action_failed");
    }
    ensure!(value["result"].is_object(), "cas_invalid_response_envelope");
    Ok(value["result"].clone())
}
fn inspection(target: &CasTarget, result: &Value) -> Result<TargetInspection> {
    ensure!(
        required_text(result, "target", 128)? == target.target,
        "cas_target_mismatch"
    );
    let status = required_text(result, "status", 32)?;
    match status {
        "resolved" => {
            fields(
                result,
                &[
                    "target",
                    "status",
                    "destination",
                    "codex_bin",
                    "socket",
                    "connectivity",
                ],
            )?;
            required_text(result, "destination", 512)?;
            required_text(result, "codex_bin", 512)?;
            ensure!(result.get("socket").is_some(), "cas_invalid_response");
            optional_text(result, "socket", 512)?;
            ensure!(
                result["connectivity"] == "not_probed",
                "cas_invalid_inspection_state"
            );
        }
        "unresolved" => {
            fields(result, &["target", "status", "error_code"])?;
            required_text(result, "error_code", 512)?;
        }
        _ => bail!("cas_invalid_inspection_state"),
    }
    Ok(TargetInspection {
        target: target.target.clone(),
        status: status.into(),
        binding_verified: false,
        execution_eligible: false,
        reason: if status == "resolved" {
            "workspace_identity_unverified"
        } else {
            "target_unresolved"
        },
    })
}
fn receipt(request_id: &str, result: &Value) -> Result<ReceiptObservation> {
    fields(
        result,
        &[
            "request_id",
            "state",
            "thread_id",
            "turn_id",
            "error_code",
            "wait_mode",
            "replayed",
            "retry_same_request_id",
            "native_state_authoritative",
        ],
    )?;
    ensure!(
        required_text(result, "request_id", 128)? == request_id,
        "cas_request_mismatch"
    );
    let state = required_text(result, "state", 32)?;
    ensure!(
        matches!(
            state,
            "not_found"
                | "unknown"
                | "in_progress"
                | "created_not_materialized"
                | "accepted"
                | "completed"
                | "failed"
                | "interrupted"
        ),
        "cas_unrecognized_receipt_state"
    );
    let thread_id = optional_text(result, "thread_id", 512)?;
    let turn_id = optional_text(result, "turn_id", 512)?;
    if state == "not_found" {
        ensure!(object(result)?.len() == 2, "cas_conflicting_receipt");
    } else {
        ensure!(
            result.get("replayed").is_some_and(Value::is_boolean)
                && result["retry_same_request_id"] == "returns_receipt_without_dispatch"
                && result["native_state_authoritative"] == true,
            "cas_invalid_receipt_contract"
        );
        if let Some(mode) = result.get("wait_mode") {
            ensure!(
                mode == "bounded" || mode == "dispatch",
                "cas_invalid_receipt_contract"
            );
        }
        optional_text(result, "error_code", 256)?;
    }
    ensure!(
        turn_id.is_none() || thread_id.is_some(),
        "cas_conflicting_receipt"
    );
    if matches!(state, "accepted" | "completed" | "failed" | "interrupted") {
        ensure!(
            thread_id.is_some() && turn_id.is_some(),
            "cas_receipt_identity_missing"
        );
    }
    if state == "created_not_materialized" {
        ensure!(
            thread_id.is_some() && turn_id.is_none(),
            "cas_conflicting_receipt"
        );
    }
    Ok(ReceiptObservation {
        request_id: request_id.into(),
        state: state.into(),
        thread_id,
        turn_id,
        binding_verified: false,
        execution_eligible: false,
        reason: "receipt_not_execution_proof",
    })
}
fn thread(target: &CasTarget, thread_id: &str, result: &Value) -> Result<ThreadObservation> {
    fields(
        result,
        &[
            "operation",
            "connection",
            "result",
            "effective_configuration",
            "receipts",
            "events",
        ],
    )?;
    ensure!(
        result["operation"] == "thread/read",
        "cas_operation_mismatch"
    );
    let connection = &result["connection"];
    fields(
        connection,
        &["target", "codex_bin", "socket", "codex_home", "server"],
    )?;
    ensure!(
        required_text(connection, "target", 128)? == target.target,
        "cas_target_mismatch"
    );
    required_text(connection, "codex_bin", 512)?;
    for field in ["socket", "codex_home", "server"] {
        optional_text(connection, field, 512)?;
    }
    for field in ["receipts", "events"] {
        if let Some(items) = result.get(field) {
            ensure!(
                items
                    .as_array()
                    .is_some_and(|items| items.len() <= 1000 && items.iter().all(Value::is_object)),
                "cas_invalid_response"
            );
        }
    }
    if let Some(configuration) = result
        .get("effective_configuration")
        .filter(|v| !v.is_null())
    {
        fields(configuration, &["model", "model_provider", "effort"])?;
        for field in ["model", "model_provider", "effort"] {
            optional_text(configuration, field, 256)?;
        }
    }
    let data = &result["result"];
    fields(data, &["thread"])?;
    let native = &data["thread"];
    ensure!(
        required_text(native, "id", 512)? == thread_id,
        "cas_thread_mismatch"
    );
    ensure!(
        required_text(native, "cwd", 512)? == target.cwd,
        "cas_cwd_mismatch"
    );
    let liveness = match native.get("status") {
        Some(status) if status.is_object() => match status["type"].as_str() {
            Some("active") => "active",
            Some("idle") => "idle",
            _ => "unknown",
        },
        None | Some(Value::Null) => "unknown",
        _ => bail!("cas_invalid_thread_status"),
    };
    Ok(ThreadObservation {
        target: target.target.clone(),
        cwd: target.cwd.clone(),
        thread_id: thread_id.into(),
        liveness,
        execution_eligible: false,
        reason: "thread_snapshot_not_cessation_proof",
    })
}

/// Largest timestamp any Factory surface accepts (the end of year 9999).
pub const MAX_TIMESTAMP: u64 = 253_402_300_799;
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// Opaque native page cursors are forwarded only as printable ASCII.
pub fn valid_cursor(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CURSOR_BYTES
        && value.bytes().all(|b| b.is_ascii_graphic())
}
/// Native integer timestamp, scaled to Unix seconds. Anything else is absent.
pub fn native_time(value: Option<&Value>, per_second: u64) -> Option<u64> {
    value
        .and_then(Value::as_u64)
        .map(|value| value / per_second)
        .filter(|seconds| *seconds <= MAX_TIMESTAMP)
}
fn page_cursor(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        // An oversized or unprintable cursor is withheld rather than forwarded later.
        Some(Value::String(cursor)) => Ok(valid_cursor(cursor).then(|| cursor.clone())),
        _ => bail!("cas_invalid_cursor"),
    }
}
fn turn_status(value: Option<&str>) -> &'static str {
    match value {
        Some("completed") => "completed",
        Some("failed") => "failed",
        Some("inProgress") => "in_progress",
        Some("interrupted") => "interrupted",
        _ => "unknown",
    }
}
/// A stable error identifier only. Native error messages are never read.
fn error_code(value: Option<&Value>) -> Option<String> {
    let identifier = |code: &str| {
        (!code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_alphanumeric()))
            .then(|| code.to_owned())
    };
    match value? {
        Value::String(code) => identifier(code).or(Some("unclassified".into())),
        Value::Object(error) => {
            let info = error.get("codexErrorInfo");
            let code = match info {
                Some(Value::String(code)) => Some(code.as_str()),
                Some(Value::Object(info)) if info.len() == 1 => {
                    info.keys().next().map(String::as_str)
                }
                _ => None,
            };
            code.and_then(identifier).or_else(|| {
                (info.is_some() || error.contains_key("message")).then(|| "unclassified".into())
            })
        }
        _ => None,
    }
}
/// RpcEnvelope checks shared by the thread history reads: exact operation and
/// configured target. Receipts, events and effective configuration are bounded
/// and otherwise ignored.
fn envelope<'a>(target: &CasTarget, value: &'a Value, operation: &str) -> Result<&'a Value> {
    fields(
        value,
        &[
            "operation",
            "connection",
            "result",
            "effective_configuration",
            "receipts",
            "events",
        ],
    )?;
    ensure!(value["operation"] == operation, "cas_operation_mismatch");
    let connection = &value["connection"];
    fields(
        connection,
        &["target", "codex_bin", "socket", "codex_home", "server"],
    )?;
    ensure!(
        required_text(connection, "target", 128)? == target.target,
        "cas_target_mismatch"
    );
    required_text(connection, "codex_bin", 512)?;
    for field in ["socket", "codex_home", "server"] {
        optional_text(connection, field, 512)?;
    }
    for field in ["receipts", "events"] {
        if let Some(items) = value.get(field) {
            ensure!(
                items
                    .as_array()
                    .is_some_and(|items| items.len() <= 1000 && items.iter().all(Value::is_object)),
                "cas_invalid_response"
            );
        }
    }
    if let Some(configuration) = value
        .get("effective_configuration")
        .filter(|v| !v.is_null())
    {
        fields(
            configuration,
            &[
                "approval_policy",
                "sandbox_policy",
                "model",
                "model_provider",
                "effort",
            ],
        )?;
    }
    ensure!(value["result"].is_object(), "cas_invalid_response");
    Ok(&value["result"])
}
fn snapshot(target: &CasTarget, thread_id: &str, value: &Value) -> Result<SnapshotObservation> {
    let result = envelope(target, value, "get_thread_snapshot")?;
    fields(
        result,
        &[
            "target",
            "cwd",
            "thread_id",
            "source",
            "thread_status",
            "active_flags",
            "unknown_active_flags",
            "native_updated_at",
            "native_updated_at_unknown",
            "latest_turn",
            "latest_item",
            "continuation",
            "observed_at",
            "revision",
            "changed",
            "native_state_authoritative",
        ],
    )?;
    ensure!(
        required_text(result, "target", 128)? == target.target,
        "cas_target_mismatch"
    );
    ensure!(
        required_text(result, "cwd", 512)? == target.cwd,
        "cas_cwd_mismatch"
    );
    ensure!(
        required_text(result, "thread_id", 512)? == thread_id,
        "cas_thread_mismatch"
    );
    ensure!(
        result["native_state_authoritative"] == true,
        "cas_invalid_snapshot"
    );
    let revision = required_text(result, "revision", 64)?;
    ensure!(hex_digest(revision), "cas_invalid_snapshot");
    let changed = result["changed"]
        .as_bool()
        .context("cas_invalid_snapshot")?;
    let thread_status = match result["thread_status"].as_str() {
        Some("active") => "active",
        Some("idle") => "idle",
        Some("notLoaded") => "not_loaded",
        Some("systemError") => "system_error",
        Some("unknown") => "unknown",
        _ => bail!("cas_invalid_snapshot"),
    };
    let flags = result["active_flags"]
        .as_array()
        .filter(|flags| flags.len() <= 8)
        .context("cas_invalid_snapshot")?;
    let mut waiting_on = flags
        .iter()
        .map(|flag| match flag.as_str() {
            Some("waitingOnApproval") => Ok("approval"),
            Some("waitingOnUserInput") => Ok("user_input"),
            _ => bail!("cas_invalid_snapshot"),
        })
        .collect::<Result<Vec<_>>>()?;
    waiting_on.sort_unstable();
    waiting_on.dedup();
    let native_updated_at = match result.get("native_updated_at") {
        None | Some(Value::Null) => None,
        Some(value) => Some(native_time(Some(value), 1).context("cas_invalid_snapshot")?),
    };
    let latest_turn = match result.get("latest_turn") {
        None | Some(Value::Null) => None,
        Some(turn) => {
            fields(
                turn,
                &["id", "status", "error_code", "native_status_unknown"],
            )?;
            Some(TurnObservation {
                id: required_text(turn, "id", 512)?.into(),
                status: turn_status(turn["status"].as_str()),
                error_code: error_code(turn.get("error_code")),
                started_at: None,
                completed_at: None,
            })
        }
    };
    if let Some(item) = result.get("latest_item") {
        ensure!(item.is_null() || item.is_object(), "cas_invalid_snapshot");
    }
    Ok(SnapshotObservation {
        revision: revision.into(),
        changed,
        thread_status,
        waiting_on,
        native_updated_at,
        latest_turn,
    })
}
fn page_data<'a>(result: &'a Value, allowed: &[&str]) -> Result<&'a Vec<Value>> {
    fields(result, allowed)?;
    let data = result["data"].as_array().context("cas_invalid_page")?;
    ensure!(data.len() <= PAGE_LIMIT, "cas_page_too_large");
    ensure!(data.iter().all(Value::is_object), "cas_invalid_page");
    Ok(data)
}
fn items(target: &CasTarget, value: &Value) -> Result<ItemsPage> {
    let result = envelope(target, value, "thread/items/list")?;
    let data = page_data(result, &["data", "nextCursor", "backwardsCursor"])?;
    for entry in data {
        ensure!(
            entry.get("item").is_some_and(Value::is_object)
                && entry
                    .get("turnId")
                    .and_then(Value::as_str)
                    .is_some_and(|turn| bounded(turn, 512)),
            "cas_invalid_page"
        );
    }
    Ok(ItemsPage {
        entries: data.clone(),
        next_cursor: page_cursor(result.get("nextCursor"))?,
    })
}
fn turns(target: &CasTarget, value: &Value) -> Result<TurnsPage> {
    let result = envelope(target, value, "thread/turns/list")?;
    let data = page_data(result, &["data", "nextCursor", "backwardsCursor"])?;
    let turns = data
        .iter()
        .map(|turn| {
            Ok(TurnObservation {
                id: required_text(turn, "id", 512)?.into(),
                status: turn_status(turn["status"].as_str()),
                error_code: error_code(turn.get("error")),
                started_at: native_time(turn.get("startedAt"), 1),
                completed_at: native_time(turn.get("completedAt"), 1),
            })
        })
        .collect::<Result<_>>()?;
    Ok(TurnsPage {
        turns,
        next_cursor: page_cursor(result.get("nextCursor"))?,
    })
}
fn timeline(target: &CasTarget, value: &Value) -> Result<TimelinePage> {
    let result = envelope(target, value, "list_thread_timeline")?;
    let data = page_data(
        result,
        &["data", "nextCursor", "activeRealtimeSessionAtPageStart"],
    )?;
    let markers = data
        .iter()
        .map(|entry| {
            let (kind, at) = match entry["type"].as_str() {
                Some("item") => ("item", None),
                Some("realtime") => ("realtime", None),
                Some("turnStarted") => ("turn_started", native_time(entry.get("startedAt"), 1)),
                Some("turnCompleted") => (
                    "turn_completed",
                    native_time(entry.get("completedAt"), 1)
                        .or_else(|| native_time(entry.get("startedAt"), 1)),
                ),
                _ => bail!("cas_invalid_page"),
            };
            Ok(TimelineMarker {
                kind,
                position: entry["position"].as_i64().context("cas_invalid_page")?,
                turn_id: optional_text(entry, "turnId", 512)?,
                at,
            })
        })
        .collect::<Result<_>>()?;
    Ok(TimelinePage {
        markers,
        next_cursor: page_cursor(result.get("nextCursor"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> CasTarget {
        CasTarget {
            endpoint: "http://127.0.0.1:8088".into(),
            package: "codex-observe".into(),
            target: "local".into(),
            cwd: "/approved/repo".into(),
        }
    }
    #[test]
    fn rejects_nonliteral_nonloopback_and_credential_endpoints() {
        for value in [
            "http://localhost:8088",
            "https://127.0.0.1",
            "http://2130706433",
            "http://127.1",
            "http://0177.0.0.1",
            "http://127.0.0.1@evil.invalid",
            "http://x:y@127.0.0.1",
            "http://127.0.0.1?token=secret",
            "http://127.0.0.1#fragment",
            "http://127.0.0.1/path",
            "http://127.0.0.1/../",
            "http://192.0.2.1",
            "http://172.30.86.1:8088",
            "http://[::ffff:127.0.0.1]",
        ] {
            let mut binding = target();
            binding.endpoint = value.into();
            assert!(binding.validate().is_err(), "accepted {value}");
        }
        for value in [
            "http://127.0.0.1:8088",
            "http://127.0.0.2/",
            "http://[::1]:8088/",
        ] {
            let mut binding = target();
            binding.endpoint = value.into();
            binding.validate().unwrap();
        }
        let mut binding = target();
        binding.cwd = "/approved/../foreign".into();
        assert!(binding.validate().is_err());
        binding = target();
        binding.package = "codex-control".into();
        assert!(binding.validate().is_err());
        assert!(serde_json::from_value::<CasTarget>(json!({"endpoint":"http://127.0.0.1","package":"codex-observe","target":"local","cwd":"/repo","authorization":"caller"})).is_err());
    }
    #[test]
    fn preparation_hashes_canonical_payload_without_execution_or_payload_retention() {
        let input = json!({"target":"local","cwd":"/approved/repo","text":"planning fixture only"});
        let a = PreparedDispatch::new(target(), input.clone()).unwrap();
        let b = PreparedDispatch::new(target(), input.clone()).unwrap();
        assert_ne!(a.request_id, b.request_id);
        assert_ne!(
            a.payload_sha256, b.payload_sha256,
            "request UUID must be part of the payload fingerprint"
        );
        assert_eq!(a.operation, CREATE_OPERATION);
        a.validate().unwrap();
        let mut normalized = serde_json::to_value(
            serde_json::from_value::<CreatePlanPayload>(input.clone()).unwrap(),
        )
        .unwrap();
        normalized["request_id"] = json!(a.request_id);
        let canonical = canonical(
            json!({"operation":CREATE_OPERATION,"payload":normalized}),
            0,
            &mut 0,
        )
        .unwrap();
        assert_eq!(
            a.payload_sha256,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&canonical).unwrap())
            )
        );
        assert!(
            !serde_json::to_string(&a)
                .unwrap()
                .contains("planning fixture only")
        );
        let mut changed = a.clone();
        changed.operation = "start_turn".into();
        assert!(changed.validate().is_err());
        changed = a.clone();
        changed.payload_sha256 = "invalid".into();
        assert!(changed.validate().is_err());
        assert!(!qualification().execution_eligible);
        for field in [
            "target",
            "cwd",
            "request_id",
            "wait_for_completion",
            "enable_list_threads_callback",
            "operation",
        ] {
            let mut bad = input.clone();
            bad[field] = match field {
                "wait_for_completion" | "enable_list_threads_callback" => json!(true),
                _ => json!("foreign"),
            };
            assert!(
                PreparedDispatch::new(target(), bad).is_err(),
                "accepted {field}"
            );
        }
        assert!(PreparedDispatch::new(target(),json!({"target":"local","cwd":"/approved/repo","text":"x".repeat(MAX_PAYLOAD_BYTES)})).is_err());
    }
    #[test]
    fn rejects_malformed_outer_and_foreign_scope_without_echoing_errors() {
        for value in [
            json!({"result":{}}),
            json!({"result":{},"error":null,"extra":true}),
            json!({"result":{},"error":"secret-response"}),
            json!({"result":null,"error":null}),
        ] {
            let error = outer(value).unwrap_err().to_string();
            assert!(!error.contains("secret-response"));
        }
        let mut inspection_result = json!({"target":"foreign","status":"resolved","destination":"hidden-host","codex_bin":"/private/codex","socket":null,"connectivity":"not_probed"});
        assert!(inspection(&target(), &inspection_result).is_err());
        inspection_result["target"] = json!("local");
        let view = inspection(&target(), &inspection_result).unwrap();
        let view = serde_json::to_string(&view).unwrap();
        assert!(!view.contains("private") && !view.contains("hidden-host"));
        let mut read = json!({"operation":"thread/read","connection":{"target":"local","codex_bin":"/codex"},"result":{"thread":{"id":"owner","cwd":"/approved/repo","status":{"type":"idle"},"preview":"secret-transcript"}}});
        let view = thread(&target(), "owner", &read).unwrap();
        assert_eq!(view.liveness, "idle");
        assert!(!view.execution_eligible);
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("secret-transcript")
        );
        read["result"]["thread"]["cwd"] = json!("/foreign");
        assert!(thread(&target(), "owner", &read).is_err());
        read["result"]["thread"]["cwd"] = json!("/approved/repo");
        read["connection"]["target"] = json!("foreign");
        assert!(thread(&target(), "owner", &read).is_err());
    }
    fn history(operation: &str, result: Value) -> Value {
        json!({"operation":operation,"connection":{"target":"local","codex_bin":"/private/codex"},"result":result,"effective_configuration":null,"receipts":[],"events":[]})
    }
    #[test]
    fn read_allowlist_contains_only_reads() {
        for action in READ_ACTIONS {
            assert!(
                ["inspect-", "read-", "get-", "list-"]
                    .iter()
                    .any(|prefix| action.starts_with(prefix)),
                "{action}"
            );
        }
        for forbidden in [
            "create-thread-and-start-turn",
            "start-turn",
            "steer-turn",
            "interrupt-turn",
            "list-loaded-threads",
        ] {
            assert!(!READ_ACTIONS.contains(&forbidden));
        }
    }
    #[test]
    fn snapshot_requires_the_exact_binding_and_never_reads_item_text() {
        let digest = "a".repeat(64);
        let result = json!({"target":"local","cwd":"/approved/repo","thread_id":"owner","source":"native-app-server","thread_status":"active","active_flags":["waitingOnApproval"],"unknown_active_flags":false,"native_updated_at":1_760_000_000,"native_updated_at_unknown":false,"latest_turn":{"id":"turn","status":"failed","error_code":"rateLimitExceeded"},"latest_item":{"id":"i","kind":"agentMessage","phase":null,"status":null,"text":"private-transcript","text_truncated":false},"continuation":{},"observed_at":"2026-10-10T00:00:00.000Z","revision":digest,"changed":true,"native_state_authoritative":true});
        let observed = snapshot(
            &target(),
            "owner",
            &history("get_thread_snapshot", result.clone()),
        )
        .unwrap();
        assert_eq!(observed.thread_status, "active");
        assert_eq!(observed.waiting_on, vec!["approval"]);
        assert_eq!(observed.native_updated_at, Some(1_760_000_000));
        let turn = observed.latest_turn.unwrap();
        assert_eq!(
            (turn.status, turn.error_code.as_deref()),
            ("failed", Some("rateLimitExceeded"))
        );
        for (field, foreign) in [
            ("target", "foreign"),
            ("cwd", "/foreign"),
            ("thread_id", "other"),
            ("revision", "not-a-digest"),
        ] {
            let mut changed = result.clone();
            changed[field] = json!(foreign);
            assert!(
                snapshot(&target(), "owner", &history("get_thread_snapshot", changed)).is_err(),
                "{field}"
            );
        }
        let mut connection = history("get_thread_snapshot", result.clone());
        connection["connection"]["target"] = json!("foreign");
        assert!(snapshot(&target(), "owner", &connection).is_err());
        assert!(snapshot(&target(), "owner", &history("thread/read", result)).is_err());
    }
    #[test]
    fn history_pages_are_bounded_and_cursors_are_printable() {
        let entry = json!({"turnId":"turn","item":{"id":"a","type":"agentMessage","text":"x"}});
        let page = items(
            &target(),
            &history(
                "thread/items/list",
                json!({"data":[entry],"nextCursor":"older","backwardsCursor":null}),
            ),
        )
        .unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("older"));
        let oversized = vec![entry.clone(); PAGE_LIMIT + 1];
        assert!(
            items(
                &target(),
                &history("thread/items/list", json!({"data":oversized}))
            )
            .is_err()
        );
        let withheld = items(
            &target(),
            &history(
                "thread/items/list",
                json!({"data":[],"nextCursor":"x".repeat(MAX_CURSOR_BYTES + 1)}),
            ),
        )
        .unwrap();
        assert!(withheld.next_cursor.is_none());
        assert!(!valid_cursor("has space") && !valid_cursor(""));
        let turn_page = turns(&target(), &history("thread/turns/list", json!({"data":[{"id":"t","status":"failed","items":[],"error":{"message":"secret detail","codexErrorInfo":{"usageLimitExceeded":{}}},"completedAt":1_760_000_100}]}))).unwrap();
        assert_eq!(
            turn_page.turns[0].error_code.as_deref(),
            Some("usageLimitExceeded")
        );
        assert_eq!(turn_page.turns[0].completed_at, Some(1_760_000_100));
        let markers = timeline(&target(), &history("list_thread_timeline", json!({"data":[{"type":"turnStarted","position":0,"turnId":"t","startedAt":1_760_000_000},{"type":"item","position":1,"turnId":"t","item":{"type":"agentMessage","text":"secret"}}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}))).unwrap();
        assert_eq!(markers.markers[0].kind, "turn_started");
        assert_eq!(markers.markers[0].at, Some(1_760_000_000));
        assert!(
            timeline(
                &target(),
                &history(
                    "list_thread_timeline",
                    json!({"data":[{"type":"unknown","position":0}]})
                )
            )
            .is_err()
        );
    }
    #[test]
    fn receipt_acknowledgement_never_becomes_binding_or_completion_proof() {
        let value = json!({"request_id":"key","state":"completed","thread_id":"owner","turn_id":"turn","replayed":true,"retry_same_request_id":"returns_receipt_without_dispatch","native_state_authoritative":true});
        let observed = receipt("key", &value).unwrap();
        assert!(!observed.binding_verified && !observed.execution_eligible);
        assert_eq!(observed.reason, "receipt_not_execution_proof");
        assert!(receipt("foreign", &value).is_err());
        assert!(receipt("key", &json!({"request_id":"key","state":"accepted"})).is_err());
        let missing = receipt("key", &json!({"request_id":"key","state":"not_found"})).unwrap();
        assert_eq!(missing.state, "not_found");
        assert!(!missing.binding_verified);
    }
}
