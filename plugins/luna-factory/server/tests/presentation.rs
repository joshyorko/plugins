//! Typed blocker categories: copy and placement only, derived from existing server state.
use luna_factoryd::{
    control::{Control, EffectKind, RunControl, Settlement},
    presentation::{authorize_action, project},
    store::{ObservedClaim, PendingDecision, Run},
};
use serde_json::{Value, json};

/// A stopped, released, resumable run with time and repairs left. No store or native process.
fn stopped_run() -> Run {
    let mut run: Run = serde_json::from_value(json!({"id":"run","request":{"repository":"test","objective":"work","acceptance":["passes"],"non_goals":[],"finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":1,"wall_seconds":60,"idempotency_key":"key"},"canonical_root":"/private/repo","repository_identity":"/private/repo/.git","state":"BLOCKED","base_head":"abc","current_subject":"abc:123","thread_id":"owner","turn_id":"turn","owned_threads":[],"generation":1,"repairs_used":0,"created_at":0,"updated_at":0,"deadline_at":60,"delta":"Stopped","blocker":"Needs evidence","observed_model":null,"observed_effort":null,"configured_model":"gpt-6-luna","configured_effort":"low","claim_held":false})).unwrap();
    let mut control = Control::new(&run.current_subject, &run.request.acceptance, 1, 60).unwrap();
    control.run_control = RunControl::Blocked;
    control.settlement = Settlement::Stopped;
    control.owner_liveness = Settlement::Stopped;
    run.control = Some(control);
    run.observed_claim = ObservedClaim::Released;
    run
}
fn view(run: &Run, timestamp: u64) -> Value {
    project(run, run.control.as_ref().unwrap(), timestamp)
}
fn action(view: &Value, kind: &str) -> Value {
    view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == kind)
        .unwrap()
        .clone()
}
fn control(run: &mut Run) -> &mut Control {
    run.control.as_mut().unwrap()
}

/// Every kind the server can currently derive, with the state that produces it.
fn cases() -> Vec<(&'static str, Run, u64, Value)> {
    let mut cases = Vec::new();
    let resumable = stopped_run();
    cases.push((
        "resume_available",
        resumable.clone(),
        0,
        json!({"kind":"none"}),
    ));

    let mut planning = stopped_run();
    planning.planning_only = true;
    cases.push(("planning", planning, 0, json!({"kind":"planning_only"})));

    let mut approval = stopped_run();
    approval.set_state(RunControl::NeedsInput);
    cases.push((
        "native_approval",
        approval,
        0,
        json!({"kind":"native_approval"}),
    ));

    let mut active = stopped_run();
    control(&mut active).settlement = Settlement::Live;
    control(&mut active).run_control = RunControl::Running;
    active.state = "RUNNING".into();
    cases.push(("active", active, 0, json!({"kind":"none"})));

    cases.push((
        "time",
        stopped_run(),
        60,
        json!({"kind":"budget_exhausted","budget":"time"}),
    ));

    let mut repair = stopped_run();
    repair.repairs_used = 1;
    cases.push((
        "repair",
        repair,
        0,
        json!({"kind":"budget_exhausted","budget":"repair"}),
    ));

    let mut diagnosis = stopped_run();
    control(&mut diagnosis).no_progress_attempts = 2;
    cases.push((
        "diagnosis",
        diagnosis,
        0,
        json!({"kind":"diagnosis_required"}),
    ));

    let mut effect = stopped_run();
    control(&mut effect)
        .observe_effect("unsettled-steer", EffectKind::NativeSteer)
        .unwrap();
    cases.push((
        "effect",
        effect,
        0,
        json!({"kind":"effect_outcome_unknown"}),
    ));

    let mut delivery = stopped_run();
    control(&mut delivery)
        .observe_effect("unsettled-pr", EffectKind::Pr)
        .unwrap();
    cases.push((
        "delivery",
        delivery,
        0,
        json!({"kind":"effect_outcome_unknown"}),
    ));

    let mut liveness = stopped_run();
    control(&mut liveness).settlement = Settlement::Unknown;
    cases.push(("liveness", liveness, 0, json!({"kind":"liveness_unknown"})));

    // A foreign claim has no typed category today; it stays uncategorized rather than guessed.
    let mut foreign = stopped_run();
    foreign.observed_claim = ObservedClaim::Foreign;
    cases.push(("foreign_claim", foreign, 0, json!({"kind":"unknown"})));

    let mut failed = stopped_run();
    failed.set_state(RunControl::Failed);
    cases.push(("failed", failed, 0, json!({"kind":"unknown"})));
    cases
}

#[test]
fn each_blocker_kind_is_derived_from_existing_state_and_reason_codes() {
    for (name, run, timestamp, expected) in cases() {
        let projected = view(&run, timestamp);
        assert_eq!(projected["blocker_kind"], expected, "{name}");
    }
    // The kind follows the server's own reason code for the same projection.
    let time = view(&stopped_run(), 60);
    assert_eq!(action(&time, "resume")["reason"], "time_budget_exhausted");
    let mut repair = stopped_run();
    repair.repairs_used = 1;
    assert_eq!(
        action(&view(&repair, 0), "resume")["reason"],
        "repair_budget_exhausted"
    );
    assert_eq!(action(&view(&stopped_run(), 0), "resume")["allowed"], true);
}

#[test]
fn blocker_kind_never_creates_a_decision_on_its_own() {
    for (name, run, timestamp, _) in cases() {
        assert!(run.pending_decision.is_none());
        let projected = view(&run, timestamp);
        assert!(
            projected["actions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|action| action["kind"] != "answer" || action["allowed"] == false),
            "{name}"
        );
        assert_ne!(projected["primary_action"]["kind"], "answer", "{name}");
        assert_ne!(projected["result"]["kind"], "needs_input", "{name}");
        assert!(
            authorize_action(&run, "answer", None, timestamp).is_err(),
            "{name}"
        );
        // Attention is unchanged when the category is removed: nothing else reads it.
        let mut without = projected.clone();
        without.as_object_mut().unwrap().remove("blocker_kind");
        let mut again = view(&run, timestamp);
        again.as_object_mut().unwrap().remove("blocker_kind");
        assert_eq!(without, again, "{name}");
    }
    // A genuine owner decision is answerable; its blocker category stays `none`.
    let mut decision = stopped_run();
    decision.set_state(RunControl::NeedsInput);
    decision.pending_decision = Some(PendingDecision {
        id: "decision".into(),
        question: "Which option?".into(),
    });
    let projected = view(&decision, 0);
    assert_eq!(projected["primary_action"]["kind"], "answer");
    assert_eq!(projected["blocker_kind"], json!({"kind":"none"}));
    // An exhausted budget stays visible next to a decision, but does not make it answerable.
    let exhausted = view(&decision, 60);
    assert_eq!(
        exhausted["blocker_kind"],
        json!({"kind":"budget_exhausted","budget":"time"})
    );
    assert_eq!(action(&exhausted, "answer")["allowed"], false);
}
