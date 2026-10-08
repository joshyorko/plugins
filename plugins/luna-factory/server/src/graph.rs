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
    }
    validate_dependencies(control)?;
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
pub fn envelope(run: &Run, proposal: Option<&Proposal>) -> Value {
    let control = run.control.as_ref().expect("decoded_control");
    let public = crate::presentation::public_control(run, control);
    let mut nodes = public["tasks"].as_array().unwrap().clone();
    for node in &mut nodes {
        let id = node["id"].as_str().unwrap().to_owned();
        node["source"] = json!(control.graph_sources.get(&id));
        node["target_preference"] = json!(control.target_preferences.get(&id));
    }
    json!({"graph":{"run_id":run.id,"revision":control.revision,
        "repository":{"alias":run.request.repository,"identity":repository_id(run),"base_head":run.base_head,"subject":run.current_subject},
        "planning_only":run.planning_only,"nodes":nodes,"criteria":public["criteria"],"attempts":public["attempts"],
        "claim":{"held":run.claim_held,"status":match run.observed_claim {crate::store::ObservedClaim::Owned=>"owned",crate::store::ObservedClaim::Released=>"released",crate::store::ObservedClaim::Foreign=>"foreign",crate::store::ObservedClaim::Unknown=>"unknown"}},
        "changes":control.graph_changes.values().collect::<Vec<_>>()},"proposal":proposal})
}
