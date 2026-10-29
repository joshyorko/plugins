//! Bounded planning metadata on the existing control ledger. No execution effects.
use crate::{
    control::{Control, Task, TaskState, bounded_id},
    store::Run,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub provider: String,
    pub repository_id: String,
    pub item_id: String,
    pub revision: String,
    /// Importer-supplied display hints. Never proof, authority or identity: duplicate detection
    /// ignores it. Omitted when absent so earlier proposal fingerprints and snapshots are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<SourceDisplay>,
}
pub const DISPLAY_URL_PREFIX: &str = "https://github.com/";
pub const DISPLAY_URL_MAX: usize = 512;
/// Largest number a JSON client can represent exactly.
pub const DISPLAY_NUMBER_MAX: u64 = 9_007_199_254_740_991;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceDisplay {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}
impl SourceDisplay {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.number.is_some() || self.url.is_some(),
            "invalid_graph_source_display"
        );
        ensure!(
            self.number
                .is_none_or(|number| (1..=DISPLAY_NUMBER_MAX).contains(&number)),
            "invalid_graph_source_display"
        );
        ensure!(
            self.url.as_deref().is_none_or(valid_display_url),
            "invalid_graph_source_display"
        );
        Ok(())
    }
}
/// A bounded `https://github.com/` path. No query, fragment, userinfo or other host can be
/// expressed, so the link can only open GitHub. Its content is still unverified display data.
fn valid_display_url(url: &str) -> bool {
    url.len() <= DISPLAY_URL_MAX
        && url.strip_prefix(DISPLAY_URL_PREFIX).is_some_and(|path| {
            !path.is_empty()
                && path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~/%".contains(&b))
        })
}
impl SourceBinding {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                &self.provider,
                &self.repository_id,
                &self.item_id,
                &self.revision
            ]
            .iter()
            .all(|s| bounded_id(s)),
            "invalid_graph_source"
        );
        if let Some(display) = &self.display {
            display.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CandidateNode {
    pub id: String,
    pub title: String,
    pub criterion_ids: Vec<String>,
    pub dependencies: Vec<String>,
    pub source: SourceBinding,
}

fn required_nullable_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphChange {
    ImportCandidates {
        nodes: Vec<CandidateNode>,
    },
    SetDependencies {
        node_id: String,
        dependencies: Vec<String>,
    },
    SetTarget {
        node_id: String,
        target_id: String,
    },
    UpdateTask {
        node_id: String,
        #[serde(deserialize_with = "required_nullable_string")]
        title: Option<String>,
        dependencies: Vec<String>,
        #[serde(deserialize_with = "required_nullable_string")]
        target_id: Option<String>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeChange {
    pub run_id: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub change: GraphChange,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyChange {
    pub run_id: String,
    pub change_id: String,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: String,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub actor: String,
    pub base_revision: u64,
    pub subject: String,
    pub change: GraphChange,
    pub status: String,
    pub applied_revision: Option<u64>,
}
/// Binding is local opaque identity, not a forge URL or an authorization grant.
pub fn repository_stamp(root: &std::path::Path, common_dir: &str) -> Result<String> {
    let mut hash = Sha256::new();
    for path in [root, std::path::Path::new(common_dir)] {
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update([0]);
        let metadata = std::fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            hash.update(metadata.dev().to_le_bytes());
            hash.update(metadata.ino().to_le_bytes());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            hash.update(metadata.creation_time().to_le_bytes());
        }
        #[cfg(not(any(unix, windows)))]
        anyhow::bail!("repository_identity_unavailable");
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn repository_id(run: &Run) -> String {
    run.graph_repository_stamp
        .clone()
        .unwrap_or_else(|| format!("{:x}", Sha256::digest(run.repository_identity.as_bytes())))
}
pub fn fingerprint(request: &ProposeChange) -> Result<String> {
    // Typed serialization fixes field order and rejects caller authorization fields.
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request)?)
    ))
}
fn editable(control: &Control, id: &str) -> Result<()> {
    let task = control.tasks.get(id).context("graph_node_missing")?;
    ensure!(
        task.state == TaskState::Candidate
            && task.native_thread.is_none()
            && !control.attempts.iter().any(|a| a.task_id == id),
        "graph_node_not_editable"
    );
    Ok(())
}
fn validate_task_snapshot(
    node_id: &str,
    title: Option<&str>,
    dependencies: &[String],
    target_id: Option<&str>,
) -> Result<()> {
    let title_limit = if node_id == "objective" { 8000 } else { 4000 };
    if let Some(title) = title {
        ensure!(
            !title.trim().is_empty() && title.len() <= title_limit,
            "invalid_graph_task_title"
        );
    }
    ensure!(
        dependencies.len() <= 128
            && dependencies.iter().collect::<BTreeSet<_>>().len() == dependencies.len(),
        "invalid_graph_dependencies"
    );
    if let Some(target_id) = target_id {
        ensure!(bounded_id(target_id), "invalid_target_preference");
    }
    Ok(())
}
fn validate_dependencies(control: &Control) -> Result<()> {
    fn visit<'a>(
        id: &'a str,
        control: &'a Control,
        path: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        ensure!(path.insert(id), "dependency_cycle");
        let task = control.tasks.get(id).context("missing_dependency")?;
        ensure!(
            task.dependencies.len() <= 128
                && task.dependencies.iter().collect::<BTreeSet<_>>().len()
                    == task.dependencies.len(),
            "invalid_graph_dependencies"
        );
        for dep in &task.dependencies {
            visit(dep, control, path, done)?;
        }
        path.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in control.tasks.keys() {
        visit(id, control, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}
pub fn apply_change(control: &mut Control, change: &GraphChange) -> Result<()> {
    let mut candidate = control.clone();
    apply_change_inner(&mut candidate, change)?;
    *control = candidate;
    Ok(())
}

fn apply_change_inner(control: &mut Control, change: &GraphChange) -> Result<()> {
    match change {
        GraphChange::ImportCandidates { nodes } => {
            ensure!(
                !nodes.is_empty() && nodes.len() <= 32 && control.tasks.len() + nodes.len() <= 128,
                "graph_node_bound"
            );
            for node in nodes {
                node.source.validate()?;
                ensure!(
                    bounded_id(&node.id)
                        && !control.tasks.contains_key(&node.id)
                        && !node.title.trim().is_empty()
                        && node.title.len() <= 4000,
                    "invalid_or_duplicate_graph_node"
                );
                ensure!(
                    !node.criterion_ids.is_empty()
                        && node.criterion_ids.len() <= 32
                        && node
                            .criterion_ids
                            .iter()
                            .all(|id| control.criteria.iter().any(|c| &c.id == id)),
                    "missing_acceptance_binding"
                );
                ensure!(
                    !control
                        .graph_sources
                        .values()
                        .any(|source| source.provider == node.source.provider
                            && source.repository_id == node.source.repository_id
                            && source.item_id == node.source.item_id),
                    "duplicate_graph_source"
                );
                control.tasks.insert(
                    node.id.clone(),
                    Task {
                        id: node.id.clone(),
                        title: node.title.clone(),
                        criteria: node.criterion_ids.clone(),
                        dependencies: node.dependencies.clone(),
                        assumptions: BTreeMap::new(),
                        subject: control.current_subject.clone(),
                        necessity: "unverified_import".into(),
                        effects: vec![],
                        claim: "unknown".into(),
                        state: TaskState::Candidate,
                        native_thread: None,
                        reason: Some("planning_candidate_no_authority".into()),
                    },
                );
                control
                    .graph_sources
                    .insert(node.id.clone(), node.source.clone());
            }
        }
        GraphChange::SetDependencies {
            node_id,
            dependencies,
        } => {
            editable(control, node_id)?;
            control.tasks.get_mut(node_id).unwrap().dependencies = dependencies.clone();
        }
        GraphChange::SetTarget { node_id, target_id } => {
            editable(control, node_id)?;
            ensure!(bounded_id(target_id), "invalid_target_preference");
            control
                .target_preferences
                .insert(node_id.clone(), target_id.clone());
        }
        GraphChange::UpdateTask {
            node_id,
            title,
            dependencies,
            target_id,
        } => {
            editable(control, node_id)?;
            validate_task_snapshot(
                node_id,
                title.as_deref(),
                dependencies,
                target_id.as_deref(),
            )?;
            let task = control
                .tasks
                .get_mut(node_id)
                .context("graph_node_missing")?;
            if let Some(title) = title {
                task.title = title.clone();
            }
            task.dependencies = dependencies.clone();
            match target_id {
                Some(target_id) => {
                    control
                        .target_preferences
                        .insert(node_id.clone(), target_id.clone());
                }
                None => {
                    control.target_preferences.remove(node_id);
                }
            }
        }
    }
    validate_dependencies(control)?;
    Ok(())
}
/// New command boundary only. Historical journal events must remain readable.
pub fn ensure_meaningful(control: &Control, change: &GraphChange) -> Result<()> {
    match change {
        GraphChange::SetDependencies {
            node_id,
            dependencies,
        } => {
            editable(control, node_id)?;
            let requested: BTreeSet<_> = dependencies.iter().collect();
            ensure!(
                dependencies.len() <= 128 && requested.len() == dependencies.len(),
                "invalid_graph_dependencies"
            );
            let current: BTreeSet<_> = control.tasks[node_id].dependencies.iter().collect();
            ensure!(requested != current, "graph_change_noop");
        }
        GraphChange::SetTarget { node_id, target_id } => {
            editable(control, node_id)?;
            ensure!(
                control.target_preferences.get(node_id) != Some(target_id),
                "graph_change_noop"
            );
        }
        GraphChange::UpdateTask {
            node_id,
            title,
            dependencies,
            target_id,
        } => {
            editable(control, node_id)?;
            validate_task_snapshot(
                node_id,
                title.as_deref(),
                dependencies,
                target_id.as_deref(),
            )?;
            let task = &control.tasks[node_id];
            let requested_dependencies: BTreeSet<_> = dependencies.iter().collect();
            let current_dependencies: BTreeSet<_> = task.dependencies.iter().collect();
            ensure!(
                title.as_ref().is_some_and(|title| title != &task.title)
                    || requested_dependencies != current_dependencies
                    || control.target_preferences.get(node_id) != target_id.as_ref(),
                "graph_change_noop"
            );
        }
        GraphChange::ImportCandidates { .. } => {}
    }
    Ok(())
}
pub fn validate_metadata(control: &Control) -> Result<()> {
    ensure!(
        control.graph_sources.len() <= 128
            && control.target_preferences.len() <= 128
            && control.graph_changes.len() <= 128,
        "graph_metadata_bound"
    );
    for (id, source) in &control.graph_sources {
        ensure!(control.tasks.contains_key(id), "graph_node_missing");
        source.validate()?;
    }
    for (id, target) in &control.target_preferences {
        ensure!(
            control.tasks.contains_key(id) && bounded_id(target),
            "invalid_target_preference"
        );
    }
    let mut keys = BTreeSet::new();
    for (id, proposal) in &control.graph_changes {
        ensure!(
            id == &proposal.id
                && bounded_id(id)
                && bounded_id(&proposal.idempotency_key)
                && keys.insert(&proposal.idempotency_key)
                && proposal.actor == "local_operator"
                && bounded_id(&proposal.subject)
                && proposal.base_revision < control.revision
                && proposal.fingerprint.len() == 64
                && match proposal.status.as_str() {
                    "proposed" => proposal.applied_revision.is_none(),
                    "applied" => proposal
                        .applied_revision
                        .is_some_and(|r| r <= control.revision && r > proposal.base_revision),
                    _ => false,
                },
            "invalid_graph_proposal"
        );
    }
    Ok(())
}
pub fn guard(run: &Run) -> Result<()> {
    let control = run.control.as_ref().context("control_missing")?;
    ensure!(
        run.graph_repository_stamp.is_some(),
        "legacy_graph_repository_unqualified"
    );
    if !run.planning_only {
        ensure!(
            control.settlement == crate::control::Settlement::Stopped
                && control.owner_liveness == crate::control::Settlement::Stopped
                && control
                    .child_liveness
                    .values()
                    .all(|s| *s == crate::control::Settlement::Stopped)
                && !control.unknown_effect()
                && control
                    .attempts
                    .iter()
                    .all(|a| !matches!(a.phase.as_str(), "active" | "intent_unknown"))
                && !run.claim_held
                && run.observed_claim == crate::store::ObservedClaim::Released
                && run.active_threads.is_empty()
                && run.owned_commands.values().all(|done| *done),
            "graph_execution_or_ownership_unresolved"
        );
    }
    Ok(())
}

const GRAPH_TITLE_DISPLAY_MAX: usize = 1000;

fn projected_task_title<'a>(run: &'a Run, control: &'a Control, id: &str) -> &'a str {
    if id != "objective" {
        return control.tasks[id].title.as_str();
    }
    control
        .graph_changes
        .values()
        .filter_map(|proposal| match &proposal.change {
            GraphChange::UpdateTask {
                node_id,
                title: Some(title),
                ..
            } if node_id == id && proposal.status == "applied" => Some((
                proposal.applied_revision.unwrap_or_default(),
                title.as_str(),
            )),
            _ => None,
        })
        .max_by_key(|(revision, _)| *revision)
        .map(|(_, title)| title)
        .unwrap_or(run.request.objective.as_str())
}

fn truncate_utf16(text: &str, max_units: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= max_units
        })
        .collect()
}

fn graph_display_title(title: &str) -> String {
    truncate_utf16(
        &crate::lifecycle::safe_summary(title, GRAPH_TITLE_DISPLAY_MAX),
        GRAPH_TITLE_DISPLAY_MAX,
    )
}

pub fn envelope(run: &Run, proposal: Option<&Proposal>) -> Value {
    let control = run.control.as_ref().expect("decoded_control");
    let public = crate::presentation::public_control(run, control);
    let mut nodes = public["tasks"].as_array().unwrap().clone();
    for node in &mut nodes {
        let id = node["id"].as_str().unwrap().to_owned();
        let canonical_title = projected_task_title(run, control, &id);
        let display_title = graph_display_title(canonical_title);
        node["title"] = json!(display_title);
        if canonical_title != display_title {
            let editor_limit = if id == "objective" { 8000 } else { 4000 };
            let safe_editor_title = truncate_utf16(
                &crate::lifecycle::safe_summary(canonical_title, editor_limit),
                editor_limit,
            );
            node["editor_title"] = if safe_editor_title == canonical_title {
                json!(canonical_title)
            } else {
                Value::Null
            };
        }
        node["source"] = json!(control.graph_sources.get(&id));
        node["target_preference"] = json!(control.target_preferences.get(&id));
    }
    json!({"graph":{"run_id":run.id,"revision":control.revision,
        "repository":{"alias":run.request.repository,"identity":repository_id(run),"base_head":run.base_head,"subject":run.current_subject},
        "planning_only":run.planning_only,"nodes":nodes,"criteria":public["criteria"],"attempts":public["attempts"],
        "claim":{"held":run.claim_held,"status":match run.observed_claim {crate::store::ObservedClaim::Owned=>"owned",crate::store::ObservedClaim::Released=>"released",crate::store::ObservedClaim::Foreign=>"foreign",crate::store::ObservedClaim::Unknown=>"unknown"}},
        "changes":control.graph_changes.values().collect::<Vec<_>>()},"proposal":proposal})
}
