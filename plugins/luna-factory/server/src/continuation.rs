//! One explicit, owner-requested next step at a verified live return boundary.
//! This policy never polls, starts native work, or upgrades model prose to facts.
use crate::control::{Control, Diagnosis, Settlement, TaskState, bounded_id};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Next,
    Repair,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairDiagnosis {
    pub summary: String,
    pub check_refs: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Directive {
    pub kind: Kind,
    pub task_id: String,
    #[serde(deserialize_with = "required_nullable_diagnosis")]
    pub diagnosis: Option<RepairDiagnosis>,
}
fn required_nullable_diagnosis<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<RepairDiagnosis>, D::Error> {
    Option::<RepairDiagnosis>::deserialize(deserializer)
}
pub struct Plan {
    pub repair: bool,
    pub diagnosis: Option<Diagnosis>,
}
pub fn fresh_selected(control: &Control) -> bool {
    control
        .tasks
        .get(&control.selected_task)
        .is_some_and(|task| {
            task.state == TaskState::Ready
                && task.necessity == "owner_declared_necessary"
                && !control.attempts.iter().any(|a| a.task_id == task.id)
        })
}
pub fn observed_failure_current(control: &Control, id: &str) -> bool {
    let Some(attempt) = control.attempts.last() else {
        return false;
    };
    control
        .checks
        .iter()
        .find(|check| check.id == id)
        .is_some_and(|check| {
            check.outcome == "failed"
                && check.binding.task_id == control.selected_task
                && check.binding.task_id == attempt.task_id
                && check.binding.attempt_id == attempt.id
                && matches!(attempt.phase.as_str(), "returned" | "stopped")
                && crate::evidence::binding_current(control, &check.binding)
                && ((check.kind == "file_sha256"
                    && check.observed_sha256.is_some()
                    && check.expected_sha256.is_some()
                    && check.observed_sha256 != check.expected_sha256)
                    || (check.kind == "native_command"
                        && check.exit_code.is_some_and(|exit| exit != 0)))
        })
}
pub fn decide(control: &Control, directive: &Directive, timestamp: u64) -> Result<Plan> {
    ensure!(
        control.run_control == crate::control::RunControl::Quiescent,
        "continuation_not_quiescent"
    );
    ensure!(
        bounded_id(&directive.task_id) && directive.task_id == control.selected_task,
        "continuation_task_mismatch"
    );
    ensure!(
        timestamp < control.deadline_at && control.attempts.len() < 1000,
        "continuation_budget_exhausted"
    );
    ensure!(
        control.settlement == Settlement::Stopped
            && control.owner_liveness == Settlement::Stopped
            && control
                .child_liveness
                .values()
                .all(|state| *state == Settlement::Stopped)
            && !control.unknown_effect()
            && control
                .attempts
                .iter()
                .all(|attempt| !matches!(attempt.phase.as_str(), "active" | "intent_unknown")),
        "continuation_execution_unresolved"
    );
    ensure!(
        control.selection_blocker.is_none(),
        "task_selection_blocked"
    );
    let task = control
        .tasks
        .get(&directive.task_id)
        .context("selected_task_unavailable")?;
    crate::control::admit_task(control, task)?;
    ensure!(
        task.effects
            .iter()
            .any(|effect| effect == "native_owner_turn"),
        "native_owner_turn_not_permitted"
    );
    match directive.kind {
        Kind::Next => {
            ensure!(
                directive.diagnosis.is_none() && fresh_selected(control),
                "continuation_next_not_ready"
            );
            Ok(Plan {
                repair: false,
                diagnosis: None,
            })
        }
        Kind::Repair => {
            ensure!(
                task.state == TaskState::Verify && control.repairs_used < control.repair_limit,
                "continuation_repair_denied"
            );
            let diagnosis = directive
                .diagnosis
                .as_ref()
                .context("continuation_diagnosis_required")?;
            ensure!(
                !diagnosis.summary.trim().is_empty()
                    && diagnosis.summary.len() <= 1000
                    && !diagnosis.check_refs.is_empty()
                    && diagnosis.check_refs.len() <= 32
                    && diagnosis.check_refs.iter().collect::<BTreeSet<_>>().len()
                        == diagnosis.check_refs.len()
                    && diagnosis
                        .check_refs
                        .iter()
                        .all(|id| bounded_id(id) && observed_failure_current(control, id)),
                "continuation_failure_unproved"
            );
            Ok(Plan {
                repair: true,
                diagnosis: Some(Diagnosis {
                    summary: diagnosis.summary.clone(),
                    basis: "observed_failure".into(),
                    check_refs: diagnosis.check_refs.clone(),
                    same_goal_replan: false,
                }),
            })
        }
    }
}
