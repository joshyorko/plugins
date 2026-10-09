//! Pure task/admission/attempt control. Native Codex remains the execution owner.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA_VERSION: u32 = 2;
fn objective_task() -> String {
    "objective".into()
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunControl {
    Starting,
    Running,
    NeedsInput,
    Verifying,
    Blocked,
    Interrupted,
    Cancelling,
    Cancelled,
    Quiescent,
    Converged,
    Failed,
    #[default]
    Unknown,
}
impl RunControl {
    pub fn legacy(self) -> &'static str {
        match self {
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::NeedsInput => "NEEDS_INPUT",
            Self::Verifying => "VERIFYING",
            Self::Blocked | Self::Unknown => "BLOCKED",
            Self::Interrupted => "INTERRUPTED",
            Self::Cancelling => "CANCELLING",
            Self::Cancelled => "CANCELLED",
            Self::Quiescent => "QUIESCENT",
            Self::Converged => "CONVERGED",
            Self::Failed => "FAILED",
        }
    }
    pub fn from_legacy(value: &str) -> Result<Self> {
        Ok(match value {
            "STARTING" => Self::Starting,
            "RUNNING" => Self::Running,
            "NEEDS_INPUT" => Self::NeedsInput,
            "VERIFYING" => Self::Verifying,
            "BLOCKED" => Self::Blocked,
            "INTERRUPTED" => Self::Interrupted,
            "CANCELLING" => Self::Cancelling,
            "CANCELLED" => Self::Cancelled,
            "QUIESCENT" => Self::Quiescent,
            "CONVERGED" => Self::Converged,
            "FAILED" => Self::Failed,
            _ => anyhow::bail!("invalid_run_control"),
        })
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Settlement {
    #[default]
    Unknown,
    Live,
    Stopped,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Candidate,
    Ready,
    Running,
    Verify,
    Done,
    Blocked,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub accepted: bool,
    pub check_refs: Vec<String>,
    pub subject: Option<String>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub criteria: Vec<String>,
    pub dependencies: Vec<String>,
    pub assumptions: BTreeMap<String, String>,
    pub subject: String,
    pub necessity: String,
    pub effects: Vec<String>,
    pub claim: String,
    pub state: TaskState,
    pub native_thread: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub task_id: String,
    pub parent: Option<String>,
    pub intent_generation: u64,
    pub dispatch_generation: u64,
    pub source_subject: String,
    pub assumptions: BTreeMap<String, String>,
    pub phase: String,
    pub turn_id: Option<String>,
    pub certified_before: usize,
    pub diagnosis: Option<Diagnosis>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    NativeThreadStart,
    NativeThreadResume,
    NativeSteer,
    Push,
    Pr,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub id: String,
    pub kind: EffectKind,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Diagnosis {
    pub summary: String,
    pub basis: String,
    pub check_refs: Vec<String>,
    #[serde(default)]
    pub same_goal_replan: bool,
}
impl Diagnosis {
    pub fn validate(&self, control: &Control) -> Result<()> {
        ensure!(
            !self.summary.trim().is_empty()
                && self.summary.len() <= 1000
                && matches!(self.basis.as_str(), "operator_semantic" | "observed_checks")
                && self.check_refs.len() <= 32,
            "invalid_diagnosis"
        );
        ensure!(
            self.check_refs.iter().all(|id| bounded_id(id)
                && control.checks.iter().any(|check| &check.id == id
                    && check.outcome == "passed"
                    && crate::evidence::binding_current(control, &check.binding))),
            "diagnosis_check_unproved"
        );
        ensure!(
            self.basis != "observed_checks" || !self.check_refs.is_empty(),
            "diagnosis_check_missing"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub schema_version: u32,
    pub revision: u64,
    pub intent_generation: u64,
    pub dispatch_generation: u64,
    pub current_subject: String,
    pub assumptions: BTreeMap<String, String>,
    pub tasks: BTreeMap<String, Task>,
    pub attempts: Vec<Attempt>,
    pub criteria: Vec<Criterion>,
    pub checks: Vec<crate::evidence::ObservedCheck>,
    pub repair_limit: u32,
    pub repairs_used: u32,
    pub deadline_at: u64,
    pub no_progress_attempts: u32,
    pub diagnosis: Option<Diagnosis>,
    pub replan_used: bool,
    pub child_policy: String,
    pub migrated: bool,
    #[serde(default)]
    pub run_control: RunControl,
    #[serde(default)]
    pub settlement: Settlement,
    #[serde(default)]
    pub owner_liveness: Settlement,
    #[serde(default)]
    pub child_liveness: BTreeMap<String, Settlement>,
    #[serde(default = "objective_task")]
    pub selected_task: String,
    #[serde(default)]
    pub selection_blocker: Option<String>,
    #[serde(default)]
    pub effects: Vec<Effect>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    pub id: String,
    pub expected_revision: u64,
    pub event: Event,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Dispatch {
        id: String,
        generation: u64,
        repair: bool,
    },
    Acknowledged {
        id: String,
        turn_id: String,
    },
    Returned {
        id: String,
    },
    Subject {
        subject: String,
    },
    Assumption {
        id: String,
        value: String,
    },
    Diagnose {
        diagnosis: Diagnosis,
    },
    EffectIntent {
        id: String,
        effect: EffectKind,
    },
    EffectSettled {
        id: String,
    },
    Replan {
        justification: String,
    },
    Candidate {
        task: Task,
    },
    CandidateRejected {
        task: Task,
        reason: String,
    },
    SelectTask {
        id: String,
    },
}

pub fn bounded_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
impl Control {
    pub fn new(
        subject: &str,
        acceptance: &[String],
        repair_limit: u32,
        deadline_at: u64,
    ) -> Result<Self> {
        ensure!(
            bounded_id(subject) && !acceptance.is_empty() && acceptance.len() <= 32,
            "invalid_control_contract"
        );
        let criteria: Vec<_> = acceptance
            .iter()
            .enumerate()
            .map(|(i, _)| Criterion {
                id: format!("A{}", i + 1),
                accepted: false,
                check_refs: vec![],
                subject: None,
                reason: "evidence_missing".into(),
            })
            .collect();
        let mut control = Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            intent_generation: 1,
            dispatch_generation: 0,
            current_subject: subject.into(),
            assumptions: BTreeMap::new(),
            tasks: BTreeMap::new(),
            attempts: vec![],
            criteria,
            checks: vec![],
            repair_limit,
            repairs_used: 0,
            deadline_at,
            no_progress_attempts: 0,
            diagnosis: None,
            replan_used: false,
            child_policy: "cooperative_unverified".into(),
            migrated: false,
            run_control: RunControl::Starting,
            settlement: Settlement::Unknown,
            owner_liveness: Settlement::Unknown,
            child_liveness: BTreeMap::new(),
            selected_task: objective_task(),
            selection_blocker: None,
            effects: vec![],
        };
        let task = Task {
            id: "objective".into(),
            criteria: control.criteria.iter().map(|c| c.id.clone()).collect(),
            dependencies: vec![],
            assumptions: BTreeMap::new(),
            subject: subject.into(),
            necessity: "explicit_user_objective".into(),
            effects: vec!["native_owner_turn".into()],
            claim: "owned".into(),
            state: TaskState::Candidate,
            native_thread: None,
            title: "Bounded objective".into(),
            reason: None,
        };
        control.tasks.insert(task.id.clone(), task);
        admit_task(&control, &control.tasks["objective"])?;
        control.tasks.get_mut("objective").unwrap().state = TaskState::Ready;
        Ok(control)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported_control_schema"
        );
        ensure!(
            self.intent_generation == 1
                && bounded_id(&self.current_subject)
                && !self.criteria.is_empty()
                && self.criteria.len() <= 32
                && self.tasks.len() <= 128
                && self.attempts.len() <= 1000
                && self.checks.len() <= 1000
                && self.assumptions.len() <= 64
                && self.tasks.contains_key("objective")
                && self.child_policy == "cooperative_unverified",
            "invalid_control_state"
        );
        for (i, criterion) in self.criteria.iter().enumerate() {
            ensure!(
                criterion.id == format!("A{}", i + 1)
                    && criterion.check_refs.len() <= 32
                    && criterion.reason.len() <= 256,
                "invalid_control_criterion"
            );
        }
        for (id, task) in &self.tasks {
            ensure!(
                id == &task.id
                    && bounded_id(id)
                    && task.criteria.len() <= 32
                    && task.dependencies.len() <= 128
                    && task.dependencies.iter().all(|id| bounded_id(id))
                    && task.title.len() <= 4000
                    && task
                        .reason
                        .as_ref()
                        .is_none_or(|reason| reason.len() <= 256)
                    && task.assumptions.len() <= 64
                    && task
                        .assumptions
                        .iter()
                        .all(|(key, value)| bounded_id(key) && bounded_id(value))
                    && bounded_id(&task.subject)
                    && task.necessity.len() <= 1000
                    && task.effects.len() <= 32
                    && task.effects.iter().all(|effect| bounded_id(effect))
                    && matches!(task.claim.as_str(), "owned" | "foreign" | "unknown"),
                "invalid_control_task"
            );
        }
        let mut ids = BTreeSet::new();
        for attempt in &self.attempts {
            ensure!(
                bounded_id(&attempt.id)
                    && ids.insert(&attempt.id)
                    && self.tasks.contains_key(&attempt.task_id)
                    && attempt.intent_generation == self.intent_generation
                    && attempt.dispatch_generation <= self.dispatch_generation
                    && bounded_id(&attempt.source_subject)
                    && attempt.assumptions.len() <= 64
                    && matches!(
                        attempt.phase.as_str(),
                        "intent_unknown" | "active" | "returned" | "stopped"
                    )
                    && attempt
                        .parent
                        .as_ref()
                        .is_none_or(|parent| ids.contains(parent)),
                "invalid_control_attempt"
            );
        }
        for check in &self.checks {
            check.validate()?;
        }
        if let Some(diagnosis) = &self.diagnosis {
            diagnosis.validate(self)?;
        }
        ensure!(
            self.effects.len() <= 1000
                && self.effects.iter().all(|effect| bounded_id(&effect.id)
                    && matches!(effect.status.as_str(), "unknown" | "settled")),
            "invalid_effect_state"
        );
        ensure!(
            self.selection_blocker
                .as_ref()
                .is_none_or(|reason| reason.len() <= 256)
                && bounded_id(&self.selected_task)
                && (self.tasks.contains_key(&self.selected_task)
                    || self.selection_blocker.is_some()),
            "invalid_task_selection"
        );
        ensure!(
            self.revision <= 9_007_199_254_740_991 && self.child_liveness.len() <= 128,
            "control_bound_exceeded"
        );
        if self.run_control == RunControl::Converged {
            ensure!(
                self.converged()
                    && self.settlement == Settlement::Stopped
                    && !self.unknown_effect(),
                "unproved_convergence_state"
            );
        }
        Ok(())
    }
    pub fn certified_count(&self) -> usize {
        self.criteria
            .iter()
            .filter(|criterion| crate::evidence::criterion_current(self, criterion))
            .count()
    }
    pub fn converged(&self) -> bool {
        self.certified_count() == self.criteria.len()
            && self.selection_blocker.is_none()
            && self
                .tasks
                .values()
                .filter(|task| {
                    task.necessity == "owner_declared_necessary" && task.state != TaskState::Blocked
                })
                .all(|task| crate::evidence::task_proof_current(self, task))
    }
    pub fn unknown_effect(&self) -> bool {
        self.attempts
            .iter()
            .any(|attempt| attempt.phase == "intent_unknown")
            || self.effects.iter().any(|effect| effect.status == "unknown")
    }
    pub fn observe_effect(&mut self, id: &str, kind: EffectKind) -> Result<()> {
        ensure!(
            bounded_id(id) && self.effects.len() < 1000,
            "effect_bound_reached"
        );
        if let Some(effect) = self.effects.iter().find(|effect| effect.id == id) {
            ensure!(effect.kind == kind, "effect_identity_conflict");
            return Ok(());
        }
        self.effects.push(Effect {
            id: id.into(),
            kind,
            status: "unknown".into(),
        });
        Ok(())
    }
    pub fn delivery_unknown(&self) -> bool {
        self.effects.iter().any(|effect| {
            matches!(effect.kind, EffectKind::Push | EffectKind::Pr) && effect.status == "unknown"
        })
    }
    pub fn invalidate(&mut self, reason: &str) {
        if self.run_control == RunControl::Converged {
            self.run_control = RunControl::Quiescent;
        }
        if self
            .diagnosis
            .as_ref()
            .is_some_and(|diagnosis| diagnosis.basis == "observed_checks")
        {
            self.diagnosis = None;
        }
        for check in &mut self.checks {
            if check.kind == "file_sha256" && check.outcome == "passed" {
                check.outcome = "unverified".into();
                check.reason = "explicit_reattestation_required".into();
            }
        }
        for criterion in &mut self.criteria {
            criterion.accepted = false;
            criterion.reason = reason.into();
        }
        for task in self.tasks.values_mut() {
            if task.state == TaskState::Done {
                task.state = TaskState::Verify;
            }
        }
    }
    pub fn observe_child(&mut self, thread: &str, returned: bool) -> Result<()> {
        ensure!(bounded_id(thread), "invalid_child_identity");
        self.child_liveness
            .entry(thread.into())
            .or_insert(Settlement::Unknown);
        let id = format!("child:{thread}");
        ensure!(
            self.tasks.len() < 128 || self.tasks.contains_key(&id),
            "task_bound_reached"
        );
        let task = self.tasks.entry(id.clone()).or_insert_with(|| Task {
            id,
            criteria: vec![],
            dependencies: vec![],
            assumptions: BTreeMap::new(),
            subject: self.current_subject.clone(),
            necessity: "unverified_native_child".into(),
            effects: vec!["unknown".into()],
            claim: "unknown".into(),
            state: TaskState::Candidate,
            native_thread: Some(thread.into()),
            title: "Observed native child".into(),
            reason: Some("native_child_policy_unverified".into()),
        });
        if returned {
            task.state = TaskState::Verify;
        }
        Ok(())
    }
}
/// Structural admission precedes managed dispatch. Necessity remains an owner judgment.
pub fn admit_task(control: &Control, task: &Task) -> Result<()> {
    ensure!(
        !task.criteria.is_empty()
            && task
                .criteria
                .iter()
                .all(|id| control.criteria.iter().any(|c| &c.id == id)),
        "missing_acceptance_binding"
    );
    ensure!(
        matches!(
            task.necessity.as_str(),
            "explicit_user_objective" | "owner_declared_necessary"
        ),
        "necessity_unverified"
    );
    ensure!(
        task.subject == control.current_subject
            && task
                .assumptions
                .iter()
                .all(|(k, v)| control.assumptions.get(k) == Some(v)),
        "stale_task_assumption_or_subject"
    );
    ensure!(task.claim == "owned", "foreign_or_unknown_claim");
    ensure!(
        !task.effects.is_empty()
            && task.effects.iter().all(|effect| matches!(
                effect.as_str(),
                "native_owner_turn" | "read_only_file_check"
            )),
        "unknown_effect"
    );
    fn visit<'a>(
        id: &'a str,
        control: &'a Control,
        candidate: &'a Task,
        path: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        ensure!(path.insert(id), "dependency_cycle");
        if !done.contains(id) {
            let task = if id == candidate.id {
                candidate
            } else {
                control
                    .tasks
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("missing_dependency"))?
            };
            for dep in &task.dependencies {
                visit(dep, control, candidate, path, done)?;
            }
            done.insert(id);
        }
        path.remove(id);
        Ok(())
    }
    visit(
        &task.id,
        control,
        task,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )?;
    ensure!(
        task.dependencies
            .iter()
            .all(|id| control
                .tasks
                .get(id)
                .is_some_and(|dependency| dependency.state == TaskState::Done
                    && dependency.subject == control.current_subject
                    && dependency
                        .criteria
                        .iter()
                        .all(|id| control.criteria.iter().any(
                            |c| &c.id == id && crate::evidence::criterion_current(control, c)
                        )))),
        "dependency_wait"
    );
    ensure!(
        control.repairs_used <= control.repair_limit && !control.unknown_effect(),
        "budget_or_unknown_effect"
    );
    Ok(())
}
/// Replay returns state only. It never starts or resumes native work.
pub fn reduce(current: &Control, envelope: &EventEnvelope) -> Result<Control> {
    current.validate()?;
    ensure!(
        bounded_id(&envelope.id) && envelope.expected_revision == current.revision,
        "stale_control_revision"
    );
    let mut next = current.clone();
    match &envelope.event {
        Event::Dispatch {
            id,
            generation,
            repair,
        } => {
            ensure!(
                bounded_id(id)
                    && next.attempts.len() < 1000
                    && !next.attempts.iter().any(|a| &a.id == id),
                "invalid_or_duplicate_attempt"
            );
            ensure!(
                *generation == next.dispatch_generation + 1,
                "dispatch_generation_conflict"
            );
            ensure!(
                !next
                    .attempts
                    .iter()
                    .any(|a| matches!(a.phase.as_str(), "active" | "intent_unknown")),
                "execution_or_effect_unknown"
            );
            if *repair {
                ensure!(
                    next.repairs_used < next.repair_limit,
                    "repair_budget_exhausted"
                );
                ensure!(
                    next.no_progress_attempts < 2 || next.diagnosis.is_some(),
                    "diagnosis_required"
                );
                next.repairs_used += 1;
            }
            ensure!(
                next.attempts.is_empty() || next.settlement == Settlement::Stopped,
                "native_cessation_unproved"
            );
            ensure!(next.selection_blocker.is_none(), "task_selection_blocked");
            let selected = next.selected_task.clone();
            let task = next
                .tasks
                .get_mut(&selected)
                .ok_or_else(|| anyhow::anyhow!("selected_task_unavailable"))?;
            if selected == "objective" {
                task.subject = next.current_subject.clone();
                task.assumptions = next.assumptions.clone();
            }
            ensure!(
                next.tasks[&selected]
                    .effects
                    .iter()
                    .any(|effect| effect == "native_owner_turn"),
                "native_owner_turn_not_permitted"
            );
            admit_task(&next, &next.tasks[&selected])?;
            next.attempts.push(Attempt {
                id: id.clone(),
                task_id: selected.clone(),
                parent: next.attempts.last().map(|a| a.id.clone()),
                intent_generation: next.intent_generation,
                dispatch_generation: *generation,
                source_subject: next.current_subject.clone(),
                assumptions: next.assumptions.clone(),
                phase: "intent_unknown".into(),
                turn_id: None,
                certified_before: next.certified_count(),
                diagnosis: next.diagnosis.take(),
            });
            next.dispatch_generation = *generation;
            next.run_control = RunControl::Starting;
            next.settlement = Settlement::Unknown;
            next.owner_liveness = Settlement::Unknown;
            next.tasks.get_mut(&selected).unwrap().state = TaskState::Ready;
            next.invalidate("new_dispatch_requires_verification");
        }
        Event::Acknowledged { id, turn_id } => {
            ensure!(bounded_id(turn_id), "invalid_turn_identity");
            let attempt = next
                .attempts
                .iter_mut()
                .find(|a| &a.id == id)
                .ok_or_else(|| anyhow::anyhow!("attempt_not_found"))?;
            ensure!(
                attempt
                    .turn_id
                    .as_ref()
                    .is_none_or(|known| known == turn_id),
                "conflicting_dispatch_turn_identity"
            );
            attempt.turn_id = Some(turn_id.clone());
            if attempt.phase == "intent_unknown" {
                attempt.phase = "active".into();
            }
        }
        Event::Returned { id } => {
            let attempt = next
                .attempts
                .iter_mut()
                .find(|a| &a.id == id)
                .ok_or_else(|| anyhow::anyhow!("attempt_not_found"))?;
            attempt.phase = "returned".into();
            next.run_control = RunControl::Verifying;
            let task_id = attempt.task_id.clone();
            let task = next.tasks.get_mut(&task_id).unwrap();
            task.state = TaskState::Verify;
            // Rebind only the owned task whose attempt actually returned. Its
            // dependencies, assumptions, necessity and original budget persist.
            task.subject = next.current_subject.clone();
        }
        Event::Subject { subject } => {
            ensure!(bounded_id(subject), "invalid_subject");
            if next.current_subject != *subject {
                next.current_subject = subject.clone();
                next.invalidate("source_subject_changed");
            }
        }
        Event::Assumption { id, value } => {
            ensure!(
                bounded_id(id)
                    && bounded_id(value)
                    && (next.assumptions.len() < 64 || next.assumptions.contains_key(id)),
                "invalid_assumption"
            );
            if next.assumptions.get(id) != Some(value) {
                next.assumptions.insert(id.clone(), value.clone());
                next.invalidate("assumption_changed");
            }
        }
        Event::Diagnose { diagnosis } => {
            diagnosis.validate(&next)?;
            if diagnosis.same_goal_replan {
                ensure!(!next.replan_used, "replan_denied");
                next.replan_used = true;
                next.invalidate("same_goal_replan");
            }
            next.diagnosis = Some(diagnosis.clone());
        }
        Event::Replan { justification } => {
            ensure!(
                !next.replan_used
                    && !justification.trim().is_empty()
                    && justification.len() <= 1000,
                "replan_denied"
            );
            next.replan_used = true;
            next.diagnosis = Some(Diagnosis {
                summary: justification.clone(),
                basis: "operator_semantic".into(),
                check_refs: vec![],
                same_goal_replan: true,
            });
            next.invalidate("same_goal_replan");
        }
        Event::EffectIntent { id, effect } => {
            next.observe_effect(id, *effect)?;
        }
        Event::EffectSettled { id } => {
            let effect = next
                .effects
                .iter_mut()
                .find(|effect| &effect.id == id)
                .ok_or_else(|| anyhow::anyhow!("effect_not_found"))?;
            ensure!(
                !matches!(effect.kind, EffectKind::Push | EffectKind::Pr),
                "delivery_certification_unsupported"
            );
            effect.status = "settled".into();
        }
        Event::CandidateRejected { task, reason } => {
            ensure!(
                bounded_id(&task.id)
                    && reason.len() <= 256
                    && next.tasks.len() < 128
                    && !next.tasks.contains_key(&task.id),
                "candidate_record_denied"
            );
            let mut task = task.clone();
            task.state = TaskState::Blocked;
            task.reason = Some(reason.clone());
            next.tasks.insert(task.id.clone(), task);
        }
        Event::SelectTask { id } => {
            let task = next
                .tasks
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("selected_task_unavailable"))?;
            ensure!(
                matches!(
                    task.state,
                    TaskState::Ready | TaskState::Verify | TaskState::Done
                ),
                "selected_task_not_admitted"
            );
            admit_task(&next, task)?;
            next.selected_task = id.clone();
            next.selection_blocker = None;
        }
        Event::Candidate { task } => {
            ensure!(
                next.tasks.len() < 128 && !next.tasks.contains_key(&task.id),
                "task_bound_or_duplicate"
            );
            ensure!(
                !task.criteria.iter().all(|id| next
                    .criteria
                    .iter()
                    .any(|criterion| &criterion.id == id
                        && crate::evidence::criterion_current(&next, criterion))),
                "criterion_already_proven"
            );
            admit_task(&next, task)?;
            let mut task = task.clone();
            task.state = TaskState::Ready;
            next.tasks.insert(task.id.clone(), task);
        }
    }
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("revision_exhausted"))?;
    next.validate()?;
    Ok(next)
}
