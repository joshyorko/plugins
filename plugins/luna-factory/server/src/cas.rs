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

pub struct CasClient {
    target: CasTarget,
    client: reqwest::Client,
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
        Ok(Self { target, client })
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
        // The action string is selected only by the three read-only methods.
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
