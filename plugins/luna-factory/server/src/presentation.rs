//! Shared operator projection and mutation policy. It grants no execution authority.
use crate::{control::Control, evidence::criterion_current, store::Run};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
fn descriptor(kind: &str, label: &str, reason: &str, tool: Option<&str>, allowed: bool) -> Value {
    json!({"kind":kind,"label":label,"reason":reason,"tool":tool,"allowed":allowed})
}
fn public_title(text: &str) -> String {
    let safe = crate::lifecycle::safe_summary(text, 1000);
    let mut units = 0;
    safe.chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= 1000
        })
        .collect()
}
pub fn public_control(run: &Run, control: &Control) -> Value {
    let criteria: Vec<_>=control.criteria.iter().enumerate().map(|(i,c)|json!({"id":c.id,"description":run.request.acceptance[i],
        "status":if criterion_current(control,c){"proven"}else if c.reason=="failed_or_contradictory_check"{"failed"}else{"unproved"},
        "reason":if criterion_current(control,c){Value::Null}else{json!(c.reason)},"check_refs":c.check_refs})).collect();
    let tasks: Vec<_>=control.tasks.values().map(|t|json!({"id":t.id,"title":public_title(if t.id=="objective"{run.request.objective.as_str()}else{t.title.as_str()}),
        "criterion_ids":t.criteria,"dependencies":t.dependencies,"state":t.state,
        "admission":if t.necessity=="unverified_native_child"{"unverified"}else if t.state==crate::control::TaskState::Blocked{"rejected"}else{"admitted"},
        "reason":if t.necessity=="unverified_native_child"{json!("native_child_policy_unverified")}else{json!(t.reason)},
        "owner_thread":if t.id=="objective"{run.thread_id.as_ref()}else{t.native_thread.as_ref()},
        "attempt_ids":control.attempts.iter().rev().filter(|a|a.task_id==t.id).take(64).map(|a|&a.id).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>()})).collect();
    let attempts: Vec<_>=control.attempts.iter().rev().take(256).collect::<Vec<_>>().into_iter().rev().map(|a|json!({"id":a.id,"task_id":a.task_id,"intent_generation":a.intent_generation,
        "dispatch_generation":a.dispatch_generation,"subject":a.source_subject,"thread_id":run.thread_id,"turn_id":a.turn_id,"status":a.phase})).collect();
    let mut effects: Vec<_> = control.attempts.iter().filter(|a| a.phase=="intent_unknown").map(|a|json!({"id":a.id,"kind":"native_dispatch","status":"unknown","reason":"dispatch_acknowledgement_unknown"})).collect();
    effects.extend(control.effects.iter().filter(|effect|effect.status=="unknown").take(32).map(|effect|json!({"id":effect.id,"kind":effect.kind,"status":effect.status,"reason":if matches!(effect.kind,crate::control::EffectKind::Push|crate::control::EffectKind::Pr){"delivery_certification_unsupported"}else{"effect_outcome_unknown"}})));
    json!({"schema_version":1,"revision":control.revision,"intent_generation":control.intent_generation,"dispatch_generation":control.dispatch_generation,
        "criteria":criteria,"tasks":tasks,"attempts":attempts,"effects":effects,"child_policy":control.child_policy})
}
pub fn project(run: &Run, control: &Control, timestamp: u64) -> Value {
    let active = control.settlement == crate::control::Settlement::Live;
    let stopped = control.settlement == crate::control::Settlement::Stopped;
    let unknown = control.unknown_effect()
        || matches!(
            run.observed_claim,
            crate::store::ObservedClaim::Foreign | crate::store::ObservedClaim::Unknown
        );
    let selection_allowed = control.selection_blocker.is_none()
        && control
            .tasks
            .get(&control.selected_task)
            .is_some_and(|task| {
                let mut task = task.clone();
                if control.selected_task == "objective" {
                    task.subject = control.current_subject.clone();
                    task.assumptions = control.assumptions.clone();
                }
                task.state != crate::control::TaskState::Blocked
                    && crate::control::admit_task(control, &task).is_ok()
            });
    let time = run.deadline_at.saturating_sub(timestamp);
    let repairs = run.request.repair_attempts.saturating_sub(run.repairs_used);
    let needs_diagnosis = control.no_progress_attempts >= 2 && control.diagnosis.is_none();
    let decision = run.pending_decision.is_some() && stopped;
    let verified = control.run_control == crate::control::RunControl::Converged
        && control.converged()
        && stopped
        && !run.claim_held
        && run.observed_claim == crate::store::ObservedClaim::Released;
    let resumable = matches!(
        control.run_control,
        crate::control::RunControl::Interrupted
            | crate::control::RunControl::Blocked
            | crate::control::RunControl::NeedsInput
            | crate::control::RunControl::Quiescent
            | crate::control::RunControl::Cancelled
    ) && stopped
        && !active
        && !unknown
        && time > 0
        && selection_allowed;
    let resume_reason = if !selection_allowed {
        "task_selection_blocked"
    } else if matches!(
        run.observed_claim,
        crate::store::ObservedClaim::Foreign | crate::store::ObservedClaim::Unknown
    ) {
        "foreign_or_unknown_claim"
    } else if control.delivery_unknown() {
        "delivery_certification_unsupported"
    } else if active {
        "owned_execution_active"
    } else if unknown {
        "effect_outcome_unknown"
    } else if !stopped {
        "owned_liveness_unknown"
    } else if time == 0 {
        "time_budget_exhausted"
    } else if repairs == 0 && !decision {
        "repair_budget_exhausted"
    } else if needs_diagnosis && !decision {
        "diagnosis_required"
    } else {
        "same_goal_remaining_budget"
    };
    let wait = descriptor(
        "wait",
        "Owner is working",
        "owned_execution_active",
        None,
        active,
    );
    let refresh = descriptor(
        "refresh",
        "Refresh",
        "read_only_status",
        Some("refresh_factory"),
        true,
    );
    let answer = descriptor(
        "answer",
        "Answer the owner",
        "pending_owner_decision",
        Some("resume_factory_run"),
        decision && resumable,
    );
    let steer = descriptor(
        "steer",
        "Correct the owner",
        "current_owned_turn",
        Some("steer_factory_run"),
        control.run_control == crate::control::RunControl::Running
            && active
            && time > 0
            && !unknown,
    );
    let cancel = descriptor(
        "cancel",
        "Stop owned execution",
        "stop_requires_native_cessation_proof",
        Some("cancel_factory_run"),
        run.claim_held && run.thread_id.is_some(),
    );
    let resume = descriptor(
        "resume",
        "Continue the same objective",
        resume_reason,
        Some("resume_factory_run"),
        resumable && !decision && repairs > 0 && !needs_diagnosis,
    );
    let reconcile = descriptor(
        "reconcile",
        "Reconcile native ownership",
        "read_only_native_reconciliation",
        Some("reconcile_factory_run"),
        run.thread_id.is_some(),
    );
    let inspect = descriptor(
        "inspect",
        "Inspect the owner",
        if needs_diagnosis {
            "diagnosis_required"
        } else {
            "inspect_native_evidence"
        },
        Some("get_factory_run"),
        true,
    );
    let primary = if control.run_control == crate::control::RunControl::NeedsInput
        && run.pending_decision.is_none()
    {
        descriptor(
            "inspect",
            "Open native Codex",
            "native_approval_requires_native_ui",
            Some("get_factory_run"),
            true,
        )
    } else if active {
        wait.clone()
    } else if unknown || (!stopped && run.claim_held) {
        if reconcile["allowed"] == true {
            reconcile.clone()
        } else {
            inspect.clone()
        }
    } else if decision {
        answer.clone()
    } else if resume["allowed"] == true {
        resume.clone()
    } else if verified {
        refresh.clone()
    } else {
        inspect.clone()
    };
    let proven = control.certified_count();
    let failed = control
        .criteria
        .iter()
        .filter(|c| !criterion_current(control, c) && c.reason == "failed_or_contradictory_check")
        .count();
    let (kind, label) = if verified {
        (
            "finished_verified",
            "Current checks and owner acceptance recorded",
        )
    } else if active {
        ("working", "Native turn is active")
    } else if decision {
        ("needs_input", "Owner needs your answer")
    } else if stopped {
        (
            "stopped_unresolved",
            "Execution stopped; acceptance remains unproved",
        )
    } else {
        ("unverified", "Execution or effect outcome remains unknown")
    };
    let workers:Vec<_>=run.owned_threads.iter().rev().take(64).map(|id|json!({"thread_id":id,"liveness":match control.child_liveness.get(id).copied().unwrap_or_default(){crate::control::Settlement::Live=>"active",crate::control::Settlement::Stopped=>"idle",crate::control::Settlement::Unknown=>"unknown"}})).collect();
    json!({"revision":control.revision,"primary_action":primary,"actions":[wait,refresh,answer,steer,cancel,resume,reconcile,inspect],
        "criteria":{"proven":proven,"failed":failed,"unproved":control.criteria.len()-proven-failed,"mandatory":control.criteria.len()},
        "result":{"kind":kind,"label":label},"owner":{"thread_id":run.thread_id,"turn_id":run.turn_id,"liveness":match control.owner_liveness {crate::control::Settlement::Live=>"active",crate::control::Settlement::Stopped=>"idle",crate::control::Settlement::Unknown=>"unknown"}},
        "workers":workers,"budget":{"time_remaining_seconds":time,"repair_attempts_remaining":repairs,"repairs_used":run.repairs_used},
        "claim":{"held":run.claim_held,"status":match run.observed_claim {crate::store::ObservedClaim::Owned=>"owned",crate::store::ObservedClaim::Released=>"released",crate::store::ObservedClaim::Foreign=>"foreign",crate::store::ObservedClaim::Unknown=>"unknown"}},
        "deliverable":{"kind":if run.request.finish=="pr"{"pr_ready"}else{&run.request.finish},"status":if verified&&run.request.finish=="local_candidate"{"verified"}else{"unproved"},"subject":control.current_subject,"reference":null}})
}
pub fn authorize_action(
    run: &Run,
    kind: &str,
    expected_revision: Option<u64>,
    timestamp: u64,
) -> Result<()> {
    let control = run
        .control
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("control_missing"))?;
    ensure!(
        expected_revision.is_none_or(|revision| revision == control.revision),
        "stale_control_revision"
    );
    let view = project(run, control, timestamp);
    let action = view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"].as_str() == Some(kind))
        .ok_or_else(|| anyhow::anyhow!("unknown_action"))?;
    ensure!(
        action["allowed"] == true,
        "action_denied:{}",
        action["reason"].as_str().unwrap_or("unverified")
    );
    Ok(())
}
