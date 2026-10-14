# #80 staged plan: route policy, verified per-turn route, then bounded escalation

## Baseline (checked against code)

- The worktree HEAD and `origin/main` are both `e194b01` (#87 has merged), not `57dbdfb`. Some issue citations have moved: the child-model stop is now at `lifecycle.rs:1419-1426` and the capability text at `lifecycle.rs:926-928`.
- No `PolicySnapshot` type exists yet. What does exist is the admitted run contract:
  - `Run.request` and `requested_effort` are set at `store.rs:727`.
  - `persist()` keeps them immutable (`immutable_run_contract`, `store.rs:876-884`) and holds the control budget fixed (`immutable_control_budget`, `store.rs:910-916`).
  - Each save is journaled with a SHA-256 snapshot (`store.rs:943-968`).
  - #80 should extend this contract. It should not add a new store.
- Luna is hard-coded in several places: `LUNA_MODEL` (`native.rs:55`), the catalog check (`native.rs:769-789`), the thread and turn guards (`native.rs:929-940` and `1001-1012`), and the projection constant (`lifecycle.rs:51`, `schemas.rs:112`).
- The only native evidence of what actually ran is `model/rerouted`. Its parser, `observed_reroute` (`native.rs:736-755`), accepts only the `highRiskCyberActivity` reason, and `observe_routing_mismatch` (`lifecycle.rs:1508-1560`) always stops the run. Observed effort is never recorded; it is reset on each dispatch (`lifecycle.rs:1628-1629`).
- Repair budget is counted in two places, `Run.repairs_used` and `Control.repairs_used`, and synced at `store.rs:917`. The escalation budget should not repeat this. It should live only in `Control` and be changed only by the reducer.

## Stage 0: prerequisite (not a PR, operator-gated)

#71 must record a qualified Luna profile. The same qualification must also name which turn-bound native field, if any, authoritatively reports model and effort on the exact native version. If no such field exists, observed routing stays `null`, escalation stays disabled, and Stages 2–3 ship with no effect.

## Stage 1 (PR A): route policy in the run contract, no behaviour change

**Changes**
- `config.rs:38-43` (`Profile`) gains an optional `routing` block:
  - `allowed_models`: exact IDs, defaulting to `["gpt-6-luna"]`.
  - `escalation`: exact model ID, effort, `max_requests` (default 0), and an operator-declared `treatment` (`solar` or `stellar`).
- Validation goes at `config.rs:146-155`: bounded counts, valid labels, and Luna must stay in the list.
- `Run` (`store.rs:40-98`) gains `route_policy`, captured at admission next to `requested_effort` (`store.rs:727`) and added to the immutability check at `store.rs:876-884`.
- `Control` (`control.rs:183-223`) gains `escalation_limit`, `escalations_used` and a bounded `escalations: Vec<EscalationRecord>` with fields attempt ID, model, effort, a reason of at most 256 characters, and status.
  - Each new field uses `serde(default, skip_serializing_if)`, as `cas_requests` already does, so legacy snapshots and digests stay unchanged.
  - Bounds are checked in `validate` (`control.rs:363-390`).
  - The limit joins `immutable_control_budget`.
- `validate_current_authority_config` (`lifecycle.rs:2259-2304`) blocks a run if the current config is narrower than its snapshot. A wider config never widens an active run.
- Add a `policy` sub-object to the route projection (`lifecycle.rs:51-56`, `schemas.rs:111-118`).

**Invariants**
- `StartRequest` stays `deny_unknown_fields` and keeps the same idempotency fingerprint (`store.rs:628-634`). Neither callers nor the model can supply models or budgets.
- Legacy rows decode as Luna-only with zero escalations.

**Tests**
- `legacy_run_decodes_luna_only_policy_with_unchanged_digest`
- `route_policy_snapshot_immutable_across_save_and_restart`
- `config_tightening_blocks_restart_widening_does_not_extend_run` (extends `review_regressions.rs:110`)
- `start_request_cannot_carry_models_or_escalation_budget`

## Stage 2 (PR B): verified per-turn observed route for Luna

**Changes**
- `native.rs:711-727`: `NativeRouteObservation` gains `effort` and `kind` (`reroute` or `turn_route`), both defaulted.
- Add `observed_turn_route(event)` for the field qualified in Stage 0, with `source` set to that native method. `configured_route` (`native.rs:757-767`) stays configuration only.
- Add `Event::RouteObserved { attempt_id, thread_id, turn_id, model, effort, source }` (`control.rs:234-286`). It is applied through `apply_event`, which deduplicates by fingerprint (`store.rs:831-866`), so replays are idempotent.
- `observe()` (`lifecycle.rs:1244-1254`) records an observation only for the exact `run.turn_id` or an owned child turn found with `find_turn`. This is the same correlation `lifecycle.rs:1520-1534` uses.
- Any observed model other than the requested one goes through the existing stop path.
- Update the `routing_telemetry` text (`lifecycle.rs:927`) only when the source is qualified.

**Invariants**
- Catalog and configuration data never count as observed.
- The 100-observation bound stays (`lifecycle.rs:1535-1538`).
- Observed fields are reset on each dispatch.

**Tests**
- Existing, kept as-is: `configured_route_never_becomes_observed_execution_telemetry` (`native_protocol.rs:41`) and `reroute_notifications_preserve_exact_turn_evidence_and_stop_owned_work` (`factory_integration.rs:478`).
- Extended: `unrelated_stale_or_invalid_reroutes_cannot_contaminate_current_evidence` (`factory_integration.rs:551`) is extended to turn routes.
- New:
  - `turn_route_records_model_effort_source_for_exact_turn`
  - `missing_route_source_leaves_observed_null`
  - `observed_luna_effort_mismatch_stops`

## Stage 3 (PR C): one bounded, persisted escalation per attempt

**Changes**
- `continuation.rs:9-25`: add `Kind::Escalate` with a required, bounded `reason` and an exact `model`. The model must already be in the run's snapshot, so the request grants nothing new.
- Gate it in `decide` (`continuation.rs:69-150`) with the same quiescent, stopped, no-unknown-effect and deadline checks used today.
- The reducer handles `Event::EscalationRequested` next to `Dispatch` (`control.rs:720-760`). It checks that the model is allowed, that `escalations_used < escalation_limit`, and that no record exists for the current attempt. It increments the counter in the same persisted event, before any `turn/start` (persist first, as `Dispatch` already does).
- `dispatch_existing_owner_locked` (`lifecycle.rs:1585-1680`) passes the snapshot model and effort to native calls:
  - Generalize `validate_luna_route` into `validate_route(catalog, model, effort)`. Catalog presence is required but proves nothing.
  - Parameterize `start_skill_turn_with_id_guarded` (`native.rs:985-1015`) and its thread-model check.
  - `dispatch_guard` (`lifecycle.rs:2306-2335`) re-checks the policy at the wire.
  - The next Luna turn must re-verify the thread model at `native.rs:1003-1006`.
- An escalated turn is "verified permitted" only when its Stage 2 observation matches the requested model and effort.

**Invariants**
- Any `model/rerouted` still stops the run, including an unrequested reroute to an allowed model.
- The child-spawn stop (`lifecycle.rs:1419-1426`) still applies; children never escalate.
- An uncertain acknowledgement (`lifecycle.rs:1670-1677`) keeps the budget spent and never replays.
- `reconcile_startup` (`lifecycle.rs:1156-1167`) never resets the counter.

**Tests**
- `out_of_policy_reroute_still_stops`
- `unrequested_reroute_to_allowed_model_still_stops`
- `permitted_escalation_records_verified_route`
- `escalation_model_not_in_snapshot_rejected_zero_send` (the fake transport sees no `turn/start`)
- `second_escalation_same_attempt_rejected`
- `escalation_budget_spent_before_send_not_refunded_on_lost_ack`
- `escalation_budget_survives_restart_without_redispatch` (modelled on `continuation.rs:531`)
- `escalated_observed_model_mismatch_stops`
- `child_spawn_on_escalation_model_stops`
- `escalation_after_deadline_or_unknown_effect_rejected`

## Stage 4 (PR D): projection and UI (Claude owns the UI; the Rust maintainer owns the projection)

**Changes**
- `presentation.rs` emits `route.escalation = {state: none|unverified|verified, treatment}`. `treatment` is set only from a verified record plus the snapshot's operator-declared treatment. It never comes from matching on a model name.
- `ui/src/view.ts:194` and `ui/src/domain.ts:59-62` render treatment from that field only.

**Tests**
- Existing, kept: `mission.test.ts:134`.
- New:
  - `ui_never_renders_treatment_from_catalog_or_configuration`
  - `unverified_escalation_stays_neutral`
  - `verified_permitted_escalation_renders_treatment`
- `public_run_contract_covers_legacy_nulls_and_lifecycle_projections` (`output_schemas.rs:256`) is extended to the new fields.

## Ownership and operator gates

- One Rust maintainer lands A, B and C in order, after #71. The work is separate from #79: it uses the single active slot and adds no scheduler.
- These steps remain with the operator:
  - choosing the exact allowed models, efforts and budget per profile;
  - the #71 evidence for which native field reports route and effort;
  - a live subscription run of one permitted escalation and one out-of-policy reroute, with no API fallback, recorded in `docs/*-verification.md`;
  - deploying the build.
- Fake-transport tests do not prove real routing.
