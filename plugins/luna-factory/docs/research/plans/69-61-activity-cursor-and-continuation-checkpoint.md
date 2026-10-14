# #69 residual activity cursor and #61 continuation checkpoint

**Baseline correction.** `origin/main` is `e194b01`, not `57dbdfb`. PR #87 has already shipped `receipts[].thread_id`, `run.created_at`, a wall-clock `activity_at` and source `display`. So the #69 residual is only ordering and graph activity. Both reports predate #87. All paths below are under `plugins/luna-factory/server/src` unless noted.

## A. #69 residual: canonical activity cursor

**Facts already in the code**
- Each run already has a gapless, monotonic order: `control.revision`. `persist` checks `old+1` and stale revisions (`store.rs:869-993`) and writes `control_events` with `UNIQUE(run_id,revision)` (`store.rs:478`, `968`). It is capped at 10000 events per run (`store.rs:961-966`).
- Graph propose and apply already go through `apply_event`, which bumps the revision (`lifecycle.rs:614-621`, `660-668`; `control.rs:690-718`). Each proposal records `base_revision` and `applied_revision`.
- `persist` never touches `updated_at`. That is why graph changes are invisible to `activity_at` (`lifecycle.rs:59`, `66-71`). `control_events` has no time column.
- Receipts have their own `sequence INTEGER PRIMARY KEY AUTOINCREMENT` (`store.rs:446`). It is never projected. Receipts are written outside the persist transaction (`store.rs:1070-1092`), 100 are kept per run (`1091`) and 20 are projected (`1095`; `schemas.rs:130`).
- The UI orders by wall clock, at seconds resolution (`store.rs:13-18`): `ui/src/lanes.ts:25`, `ui/src/narrative.ts:104-105`. The "since you last looked" baseline is `{at, updated}` timestamps (`narrative.ts:72`).

**Stage A1: Rust contract PR (the one integration owner)**
- Project `receipts[].sequence`. Sort and limit by it, as today.
- Add a nullable `receipts.control_revision` column, set to `run.control.revision` at insert time. Use the same idempotent, no-`user_version` migration as `migrate_receipt_attribution` (`store.rs:457-468`). Legacy rows stay NULL and are never backfilled.
- Project `run.activity_cursor = {revision, receipt_sequence|null, oldest_retained_sequence|null}`.
- Derive graph activity entries `{change_id, kind: proposed|applied, revision}` from `control.graph_changes`. Proposed is at `base_revision+1`; applied is at `applied_revision`. No new table is needed.
- Optional: add a nullable, display-only `control_events.recorded_at`.
- `activity_at` and `created_at` stay as they are and are display-only.
- Invariants:
  - Order within a run comes only from `(revision, sequence)`. Timestamps are never compared.
  - Per-run sequence gaps are normal, because the sequence is global.
  - No revision, fencing, claim or evidence semantics change.
  - A cursor below `oldest_retained_sequence` reports `truncated` instead of claiming continuity.
  - Retention stays 100/20/10000.
- Tests:
  - `receipt_order_follows_sequence_when_created_at_ties_or_regresses`: same second, and a clock that moves backwards.
  - `graph_propose_and_apply_advance_activity_cursor_without_receipts`.
  - `activity_cursor_monotonic_across_restart_and_receipt_pruning`.
  - `cursor_older_than_retained_window_reports_truncated`.
  - `legacy_receipts_project_null_control_revision`.
  - Update `tests/output_schemas.rs`, `tests/mcp_contract.rs` and `tests/control.rs:760`.

**Stage A2: UI adoption (Claude)**
- Add the zod fields in `ui/src/domain.ts:55,66`.
- Sort by `sequence` in `lanes.ts:25` and `narrative.ts:105`.
- The baseline stores the cursor, so a revision bump marks a plan change as new.
- Tests: `ui/test/graph_e2e.test.ts`, plus parser cases for missing and truncated cursors.

**Operator-gated:** only deploying the exact build to the preview and authenticated host acceptance.

## B. #61 residual checkpoint

**What PR #63 already provides (fixture-proven)**
- Automatic continuation is triggered only by `FinishOrigin::LiveOwnerEvent` (`lifecycle.rs:1942-1973`).
- The terminal state is saved before the next intent (`2019-2034`).
- `continue_live_owner_locked` (`1709-1771`) requires:
  - a graph stamp (`1724`) and no observed reroute (`1728`);
  - the claim (`1733`) and observed stop (`1737`);
  - an unchanged source (`1741`).
- `continuation::decide` (`continuation.rs:69-155`) requires QUIESCENT, the deadline, and stopped owner, children and effects (`70-95`). `next` requires a fresh, necessary READY task (`36-45`). `repair` needs a current observed failure (`46-68`) within `repair_limit`.
- Dispatch rechecks everything at the wire through `dispatch_guard` (`lifecycle.rs:2306-2335`), and `persist` rejects budget resets (`store.rs:869-993`).
- No saved directive is replayed on restart:
  - Recovery runs `finish_turn_locked` with `RecoveryReadOnly` (`lifecycle.rs:2396-2437`).
  - The directive is held in memory and is never persisted.
  - Startup blocks with "Resume explicitly" (`1191`).

**What the #71 live gate must still prove**
- **Entitlement:** a real subscription-entitled owner and a Factory-owned worker, with no API fallback.
- **Admission gate:** today `Factory::start` (`lifecycle.rs:956`) is ungated. Eligibility is hard-coded false (`926`; `schemas.rs:153`).
- **Routing:** the observed route. `observed_routing` is "unverified" (`925`), and `validate_luna_route` (`native.rs:769`) only checks the catalog.
- **Callbacks and approvals:** native callback and approval flows.
- **Cessation:** owner, descendants and terminal commands, on the real runtime. Native child policy is `cooperative_unverified` (`928`). This uses `observe_stopped` (`2044-2124`), `cancel_owned` (`2176-2196`), `stop_owned_terminals` (`2197-2236`), and `native.rs:1054`, `1107` and `1247`.
- **Restart on the real runtime:** kill the daemon while a turn is live and while a directive is pending. Neither may be replayed.

**Sequential milestone stages, in order**

| # | Stage | Code | PR boundary | Invariants | Tests (existing = E, new = N) | Operator-gated |
|---|---|---|---|---|---|---|
| 1 | Issue objective | `store.rs:365`, `621-781` | none (manual text; #76 later) | immutable request (`store.rs:875-885`); `display` is never proof | E `planning_roundtrip_replay_conflicts_and_no_execution` (`tests/graph.rs:138`) | choice of the issue |
| 2 | Persisted plan | `lifecycle.rs:499`, `568`, `626`; `control.rs:690-718` | **B2:** minimal #77 promotion: a new run from an exact revision. Today tasks come only from owner candidates (`lifecycle.rs:789`, `control.rs:582`) | planning flag never flips; promotion is idempotent | N `promotion_creates_one_run_from_exact_revision_idempotently`, N `stale_promotion_revision_rejected` | approving the plan revision |
| 3 | Admitted qualified execution | `lifecycle.rs:956`, `1001-1129`, `2306` | **B1:** a qualification record in the existing SQLite store, checked in `dispatch_guard`, start, continue and steer | zero sends when unqualified; a qualification-only grant cannot start a campaign | N `unqualified_profile_sends_zero_turns_via_mcp_start_continue_repair_steer`, N `revoked_qualification_refuses_prewire_send` | recording the qualification evidence (#71 doc + JSON) |
| 4 | Two necessary tasks | `lifecycle.rs:1709-1771`, `1562-1708`; `continuation.rs:69-121` | exists | one owner, deadline and generation lineage | E `two_tasks_continue_without_external_resume_and_keep_original_lineage` (`tests/continuation.rs:111`), `independent_ready_task_continues_while_unrelated_dependency_stays_blocked` (`319`), `blocked_absent_malformed_and_unverified_failure_directives_do_not_continue` (`440`) | live run |
| 5 | Bounded repair | `continuation.rs:46-68`, `122-153`; `evidence.rs:590` | exists | `repairs_used` is never reset; only observed failures count | E `failed_file_predicate_repairs_same_task_once_with_original_budget_and_deadline` (`tests/continuation.rs:183`), `failed_selected_task_repairs_its_observed_output_without_replacement` (`tests/factory_integration.rs:1475`), `replayed_child_failure_does_not_consume_two_repairs` (`tests/decision_recovery.rs:244`) | live failure case |
| 6 | Verified local candidate | `evidence.rs:179`, `202`, `375`, `541`; `control.rs:485`; `lifecycle.rs:26-40`, `2035-2041` | **B3:** captured, load-bearing predicates (today the independent check is only `file_sha256`; delivery is unsupported) | CONVERGED requires stopped, no unknown effect and a released claim | E `sequential_necessary_tasks_finish_only_after_explicit_current_reattestation` (`tests/factory_integration.rs:1592`); N `worker_written_pass_file_does_not_prove_command` | which checks count as load-bearing |
| 7 | Restart recovery | `lifecycle.rs:1156-1207`, `2396-2437` | exists | read-only first; claim held | E `recovery_of_completed_report_never_runs_its_continuation_directive` (`tests/continuation.rs:402`), `lost_continuation_ack_reconcile_and_restart_never_redispatch` (`531`), `restart_recovers_a_completed_dispatch_without_starting_a_thread_or_turn` (`tests/factory_integration.rs:622`), `second_service_cannot_reconcile_another_live_owner` (`189`) | live daemon kill |
| 8 | Cancellation | `lifecycle.rs:2128-2236`, `1145` | exists | claim kept until owner, descendants and terminals are observed stopped | E `cancellation_terminates_only_fresh_owned_terminal_and_requires_exit_evidence` (`691`), `restart_preserves_pending_and_unknown_terminal_stops_without_replay` (`806`), `interrupt_ack_and_missing_children_do_not_prove_cancellation` (`tests/lifecycle.rs:72`), `closed_daemon_proxy_reconnects_and_cancels_the_same_owned_execution` (`tests/review_regressions.rs:182`) | live cancel with a real child and terminal |

**Composition test (B3):** one fixture scenario, N `issue_objective_to_verified_local_candidate_with_repair_restart_and_cancel`, in `tests/continuation.rs`. Label it as simulated.

**Expected post-restart prompt:** after a restart, the one explicit Resume required at `lifecycle.rs:1191` is by design and does not count as a routine prompt.

**Operator-gated live canary:** the only existing live test is `installed_binary_initializes_and_advertises_exact_luna` (`tests/native_live.rs:10`, ignored), and it sends only initialize and model/list. #61 stays open until the live canary evidence is recorded.
