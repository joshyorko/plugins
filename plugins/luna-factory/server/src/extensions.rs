//! Supported OpenAI MCP extension contracts built on the pinned rmcp model.

use anyhow::{Context, Result, bail, ensure};
use rmcp::model::{
    ElicitRequest, ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema,
    InputRequest, InputRequests, InputRequiredResult, RequestMetaObject,
};
use serde::Deserialize;
use serde_json::{Value, json};

const REPOSITORY_FORM_STATE_PREFIX: &str = "luna-factory:repository-request:v1:";
const MAX_FORM_CANDIDATES: usize = 20;
pub const OPENAI_LEGACY_FORM_METHOD: &str = "openai/elicitation/create";

pub enum FormDecision {
    Accepted(crate::repositories::RegistrationRequest),
    Declined,
    Cancelled,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryFormValue {
    candidate_id: String,
    alias: String,
    max_finish: String,
    request_for_local_approval: bool,
}

pub fn supports_openai_form(protocol_version: &str, capabilities: &Value) -> bool {
    protocol_version >= "2026-07-28"
        && capabilities["elicitation"]["form"].is_object()
        && capabilities["extensions"]["openai/elicitation"]["form"].is_object()
}

pub fn repository_form_schema(candidates: &Value) -> Result<Value> {
    let candidates = candidates
        .as_array()
        .context("repository_candidates_unavailable")?;
    ensure!(
        !candidates.is_empty() && candidates.len() <= MAX_FORM_CANDIDATES,
        "repository_form_unavailable"
    );
    let mut options = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let id = candidate["id"]
            .as_str()
            .context("repository_candidate_invalid")?;
        let name = candidate["name"]
            .as_str()
            .context("repository_candidate_invalid")?;
        ensure!(
            id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "repository_candidate_invalid"
        );
        options.push(json!({"const":id,"title":name.chars().take(120).collect::<String>()}));
    }
    Ok(json!({
        "type":"object",
        "properties":{
            "candidate_id":{"type":"string","title":"Discovered repository","oneOf":options},
            "alias":{"type":"string","title":"Local repository alias","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"},
            "max_finish":{"type":"string","title":"Maximum requested finish","oneOf":[
                {"const":"local_candidate","title":"Local candidate"},
                {"const":"push","title":"Push branch"},
                {"const":"pr","title":"Create pull request"}
            ]},
            "request_for_local_approval":{"type":"boolean","title":"Request local operator approval","default":false}
        },
        "required":["candidate_id","alias","max_finish","request_for_local_approval"]
    }))
}

pub fn legacy_repository_form_params(candidates: &Value) -> Result<Value> {
    Ok(json!({
        "mode":"form",
        "message":"Choose a discovered repository and requested limit. This only creates a pending request; a local operator must approve it before Factory can use it.",
        "requestedSchema":repository_form_schema(candidates)?
    }))
}

pub fn make_repository_form(
    candidates: &Value,
    request_state: &str,
) -> Result<InputRequiredResult> {
    ensure!(
        is_repository_form_state(request_state),
        "invalid_repository_form_state"
    );
    let requested_schema = repository_form_schema(candidates)?;
    let mut meta = RequestMetaObject::new();
    meta.0.insert(
        "openai/elicitation".into(),
        json!({"requestedSchema":requested_schema}),
    );
    let core_schema: ElicitationSchema =
        serde_json::from_value(json!({"type":"object","properties":{}}))?;
    let params = ElicitRequestParams::FormElicitationParams {
        meta: Some(meta),
        message: "Choose a discovered repository and requested limit. This only creates a pending request; a local operator must approve it before Factory can use it.".into(),
        requested_schema: core_schema,
    };
    let request = ElicitRequest::new(params);
    let requests =
        InputRequests::from([("repository".to_string(), InputRequest::Elicitation(request))]);
    Ok(InputRequiredResult::new(
        Some(requests),
        Some(request_state.to_string()),
    ))
}

pub fn parse_repository_form_result(result: &Value, candidates: &Value) -> Result<FormDecision> {
    let response: ElicitResult =
        serde_json::from_value(result.clone()).context("invalid_repository_form_response")?;
    match response.action {
        ElicitationAction::Decline => return Ok(FormDecision::Declined),
        ElicitationAction::Cancel => return Ok(FormDecision::Cancelled),
        ElicitationAction::Accept => {}
        _ => bail!("invalid_repository_form_response"),
    }
    let content = response
        .content
        .context("repository_form_content_missing")?;
    let answer: RepositoryFormValue =
        serde_json::from_value(content).context("repository_form_fields_invalid")?;
    ensure!(
        answer.request_for_local_approval,
        "repository_request_not_confirmed"
    );
    ensure!(
        crate::config::valid_alias(&answer.alias),
        "invalid_repository_alias"
    );
    let candidates = candidates
        .as_array()
        .context("repository_candidates_unavailable")?;
    let candidate = candidates
        .iter()
        .find(|candidate| candidate["id"].as_str() == Some(answer.candidate_id.as_str()))
        .context("stale_repository_candidate")?;
    ensure!(
        crate::config::finish_rank(&answer.max_finish)?
            <= crate::config::finish_rank(
                candidate["max_finish"]
                    .as_str()
                    .context("repository_candidate_invalid")?
            )?,
        "repository_authority_exceeded"
    );
    Ok(FormDecision::Accepted(
        crate::repositories::RegistrationRequest {
            candidate_id: answer.candidate_id,
            alias: answer.alias,
            max_finish: answer.max_finish,
        },
    ))
}

pub fn repository_form_state() -> &'static str {
    REPOSITORY_FORM_STATE_PREFIX
}

pub fn new_repository_form_state() -> String {
    format!("{}{}", REPOSITORY_FORM_STATE_PREFIX, uuid::Uuid::new_v4())
}

pub fn is_repository_form_state(value: &str) -> bool {
    value
        .strip_prefix(REPOSITORY_FORM_STATE_PREFIX)
        .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok())
}

#[cfg(test)]
mod tests {
    use super::{make_repository_form, new_repository_form_state, repository_form_schema};
    use serde_json::json;

    #[test]
    fn repository_form_uses_bounded_display_names_and_never_sends_paths() {
        let candidates = json!([{"id":"a".repeat(64),"name":"sample","root_alias":"private-path","root":"/private/path","max_finish":"local_candidate"}]);
        let schema = repository_form_schema(&candidates).unwrap();
        assert_eq!(
            schema["properties"]["candidate_id"]["oneOf"][0]["title"],
            "sample"
        );
        assert_eq!(
            schema["properties"]["request_for_local_approval"]["default"],
            false
        );
        assert!(!schema.to_string().contains("/private/path"));
        let result = serde_json::to_value(
            make_repository_form(&candidates, &new_repository_form_state()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            result["inputRequests"]["repository"]["params"]["requestedSchema"]["properties"],
            json!({})
        );
        assert!(!result.to_string().contains("/private/path"));
    }
}
