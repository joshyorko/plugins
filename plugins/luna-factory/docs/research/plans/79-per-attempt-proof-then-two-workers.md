# #79 staged plan: per-attempt proof, reservations, combined candidate, then two workers

## #79 staged plan (read-only; code at origin/main `e194b01`)

The baseline has moved past `57dbdfb`: #87 landed and shifted line numbers in `lifecycle.rs`, `store.rs` and `presentation.rs`. `control.rs`, `evidence.rs`, `continuation.rs` and `native.rs` are unchanged, so the issue's `control.rs:735-741` citation still holds. All paths below are under `plugins/luna-factory/server/src`. One Rust integration owner owns every edit to the reducer, store and lifecycle. Fixture authors only add tests. Nothing new is introduced: state lives in `Control`/`Run` snapshots and the existing `control_events` journal (`store.rs:478`).

### Code facts that set the order
- **Run-global settlement.** `Control.settlement`, `owner_liveness` and `child_liveness` (`control.rs:205-209`) are the only cessation facts. Five gates read them:
  - Dispatch (`control.rs:753-756`)
  - candidate proof `binding_identity_current` (`evidence.rs:77-78`)
  - continuation (`continuation.rs:82-95`)
  - the Converged invariant (`control.rs:469-476`)
  - claim release (`store.rs:1043-1049`)
- **Code that assumes the latest attempt is the only attempt:**
  - `evidence.rs:327` (native checks)
  - `evidence.rs:507-513` (no-progress counting)
  - `continuation.rs:47` (observed failures)
  - `control.rs:778` (the parent attempt is always the last one)
  - `lifecycle.rs:1755-1760` (continuation's attempt check)
  - `lifecycle.rs:1347-1362` and `2119-2121`: these change `attempt.phase` directly, outside the reducer, through `attempts.last_mut()`.
- **Single-attempt run scalars.** `thread_id`, `dispatch_id`, `turn_id`, `dispatch_phase` and `generation` (`store.rs:52-67`). The attempt projection reports the owner thread for every attempt (`presentation.rs:29`).
- **Every dispatch invalidates all proof** (`control.rs:793` → `523-550`).
- **One workspace for everything.** Proof and acceptance bind to the single `current_subject`/`canonical_root`: `evidence.rs:113,184,464` and `lifecycle.rs:1580,1618,2006,2306-2333`.
- **`capacity` is a native child cap, not worker slots.** It feeds the native thread config (`native.rs:929-931`) and stops the run when exceeded (`lifecycle.rs:1448-1451`). It is bounded at `store.rs:398` and `config.rs:119`.
- **Effects and unknowns are run-global.** `Effect{id,kind,status}` (`control.rs:129-135`), `unknown_effect` (`496-501`), and admission refuses everything while any effect is unknown (`663-666`).

### Stage 1: per-attempt ownership, settlement and proof, still serial
**PR boundary:** one PR covering `control.rs`, `evidence.rs`, `continuation.rs`, `lifecycle.rs` and `store.rs` decode. The guard at `control.rs:735-741` stays untouched.
- **New `Attempt` fields** (`control.rs:107-119`): `settlement`, `thread_id` and `result_subject`, all serde-defaulted.
- **New reducer event `Event::Stopped{id}`.** It replaces the direct phase writes at `lifecycle.rs:1362` (use the existing `Event::Acknowledged`) and `2120`.
- **Find attempts by id or turn, never by `last()`,** at every site listed above.
- **Proof binds to the attempt.** `binding_identity_current` checks the bound attempt's own `settlement == Stopped` and `result_subject`.
- **Global settlement becomes a barrier.** `Control.settlement` is kept and recomputed as "every attempt plus owner plus children stopped". Dispatch, continuation, Converged and `release_verified` keep using it.
- **Legacy migration** (`store.rs:119-226`): earlier attempts may inherit `Stopped`, because each later Dispatch already required global `Stopped` (`control.rs:753`). The latest attempt inherits the global value. Nothing else is upgraded.

**Invariants:** a single active attempt; immutable budgets, deadline and owner (`store.rs:884-905`); decoding never upgrades proof; replay never dispatches.

**Tests:**
- All existing tests pass unchanged, including `evidence.rs::new_dispatch_requires_explicit_reattestation_and_denies_old_active_or_unknown_attempts` and `continuation.rs::lost_continuation_ack_reconcile_and_restart_never_redispatch`.
- `stopped_attempt_proof_current_while_owner_live_never_claims_global_stopped`
- `legacy_snapshot_infers_attempt_settlement_only_from_serial_guard`
- `stopped_is_a_replayed_event_not_a_direct_write`
- `native_check_binds_by_turn_not_last_attempt`

**Operator-gated:** the database migration on any deployed preview.

### Stage 2: workspace and capacity reservations, still serial
**PR boundary:**
- **Per-run ceilings.** Add `worker_slots` and `model_calls` to `StartRequest` (`store.rs:21-31`), defaulting to 1 for legacy runs. They join the immutable run contract (`store.rs:874-884`).
- **Reservations inside `Control`.** Each reservation covers one attempt (or the owner/verifier call) and records its workspace, its write paths and a state: `reserved | dispatching | running | cancelling | unknown`.
- **New `Task.write_scope` field.** An empty scope means the whole repository, so such tasks serialize.
- **`Effect.attempt_id`** binds each effect to its attempt.
- **Per-attempt git worktree** under the one campaign claim (`store.rs:445`, `969-985`). File digests and subject checks resolve against the attempt's own workspace instead of `canonical_root`.

**Invariants:**
- A slot is freed only by a definite no-send, or by verified cessation plus settled effects.
- Cancelling and unknown reservations stay occupied. A cancel request (`lifecycle.rs:2140`) and deadline expiry do not free a slot.
- An unknown `NativeThreadStart` (`lifecycle.rs:1020-1054`) keeps its reservation and is never retried with a fresh key.
- The claim is released only when every reservation is released.

**Ceilings are separate:**
- `worker_slots` counts occupied worker reservations.
- `model_calls` counts owner turns, workers, verifier and escalation calls.
- An idle owner uses no model call, and no third call is ever free.
- The existing `capacity` stays the native child cap. Observed worker children count against `model_calls`; exceeding it stops the run, following `lifecycle.rs:1448`.

**Tests:**
- `cancelling_reservation_keeps_slot_and_workspace`
- `lost_thread_start_ack_keeps_unknown_reservation_no_fresh_key`
- `overlapping_or_unscoped_write_paths_rejected_with_zero_sends`
- `restart_reconstructs_reservations` (`mark_interrupted`, `store.rs:997-1020`)
- `workspace_not_reused_until_attempt_stopped`
- `model_call_ceiling_counts_owner_turn`
- `claim_release_requires_all_reservations_released`

**Operator-gated:** default ceilings and where worktrees live on disk.

### Stage 3: combined-candidate construction and verification, exercised serially
**PR boundary:**
- **Worker results are fixed when they return.** `Returned` (`control.rs:814-828`) records the worker's result commit as `result_subject`. Task-level receipts stay historical and immutable.
- **New `Control.integration`.** It records the integration subject; each member's task, attempt, base and result subject; and the assembly operation.
- **Rust performs a local merge** into a separate integration worktree, recorded as a new non-delivery `EffectKind`.
- **Acceptance moves to the integration subject.** `criterion_current` and `converged` (`evidence.rs:182-200`, `control.rs:485-495`) require checks bound to the integration subject, and an integration that covers the current attempt of every necessary task.
- **Serial equivalence.** With one member, the integration subject equals today's `current_subject`.

**Invariants:**
- Worker receipts are never rewritten.
- A rebase, repair, subject change, assumption change or criterion change makes the integration stale (`control.rs:829-847`).
- Non-overlapping paths permit dispatch, never acceptance.
- `release_verified` with CONVERGED still requires `converged()`.

**Tests:**
- **`two_passing_candidates_whose_combination_fails_stay_unconverged`:**
  - Setup: T1 and T2 each have current passed task proof, but the integration check fails.
  - `converged()` is false and the Converged state is rejected (`control.rs:469-476`).
  - The CONVERGED claim release is refused, and the repair diagnosis must cite the integration-bound failure.
- `member_receipts_immutable_after_rebase`
- `integration_stale_when_member_repaired`
- `disjoint_paths_do_not_imply_acceptance`
- `single_member_integration_equals_legacy_subject`

**Operator-gated:** which checks are load-bearing in the acceptance contract (#71/#77).

### Stage 4: two-worker scheduling, only after stages 1-3 and the #71 profile pass
**PR boundary:**
- **The guard is relaxed last.** Replace `control.rs:735-741` and `753-756` with reservation counting: occupied slots below `worker_slots`, active calls below `model_calls`, no overlapping scope, and no global unknown. Any unknown thread start still blocks everything.
- **`Dispatch` names its task** instead of relying on `selected_task` (`control.rs:211,901-916`).
- **One start per worker.** Each worker gets its own `start_thread_guarded` call (`native.rs:914-943`) with the worker's worktree as `cwd`, an `EffectIntent` and its own guard.
- **Owner identity stays fixed** (`store.rs:886-891`).
- **Events route by thread to the attempt** instead of `is_owner` (`lifecycle.rs:1276`).
- **Per-attempt stop.** Stopping one attempt interrupts only that worker. Today `cancel_owned` interrupts the owner first (`lifecycle.rs:2176-2196`).
- **Projection:** `attempt.thread_id` (`presentation.rs:29`), and `owner_thread` docks a worker token only on tasks bound to the owner or an attempt (`:26`).
- **Continuation stays one owner directive per admission** (`continuation.rs:69-155`). No pump is added.

**Invariants:** one campaign claim; budgets, repair limits and revision fencing unchanged; convergence only after the integration passes and every attempt has ceased.

**Tests:**
- `two_concurrent_attempts_on_independent_ready_tasks`
- `overlapping_path_dispatch_rejected_zero_sends`
- `claim_release_waits_for_cessation_of_both`
- `third_dispatch_denied_when_one_slot_unknown`
- `worker_stop_does_not_interrupt_owner_or_peer`
- `one_cancelling_one_completing`
- `stale_dependency_proof_blocks_dependent_dispatch`
- `crash_restart_no_duplicate_launch_or_workspace_reuse`
- The combined-failure regression from stage 3, rerun with `worker_slots=2`.

**Operator-gated:**
- Choosing and qualifying the execution profile. Threads in a shared daemon need proven per-thread cessation; otherwise use separately owned stdio processes.
- Enabling `worker_slots > 1` for each campaign. The default ships as 1.
- The live canary.
- #82 (multiple campaigns) stays out of scope.
