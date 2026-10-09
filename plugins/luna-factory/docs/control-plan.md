# Control alignment implementation plan

> For agentic workers: use `superpowers:executing-plans` task by task.
> The existing Plugins owner is the sole integrator. One isolated Rust writer owns
> server/core/store/lifecycle; one disjoint writer owns ui/. Bounded read-only
> donor and adversarial reviewers do not write product code.

**Goal:** Put revision-fenced task admission, observed evidence and safe operator
actions on the standalone runtime's production path without changing execution
ownership or losing the green onboarding/branding behavior.

**Architecture:** Pure Rust control/evidence functions produce bounded next state
and authorized effects. One SQLite snapshot/event journal persists transitions
before the existing Factory interprets native effects. A read-only presentation
model supplies the same action availability to MCP guards and the ChatGPT UI.

**Tech stack:** Existing Rust/rmcp/SQLite, native Codex, TypeScript/MCP Apps.
**Spec:** [Control adoption](control-adoption.md).
**Wire boundary:** [Control/presentation contract](control-wire.md).

## Global constraints

Historical implementation constraints below describe the original swarm. The [October 9 maintainer decision](https://github.com/joshyorko/plugins/issues/59#issuecomment-6089593813) supersedes their merge/release/activation prohibitions for the verified Luna Factory stack. Preserve the unknown owner and held claim; safe activation still requires the recovery and acceptance gates in [integration readiness](integration-readiness.md).

- Baseline `6cc2062070f0ee6b7e8c51422580bd0743f972bb`; preserve onboarding/branding/tests.
- Review and Changeplane are read-only donors, never runtime dependencies.
- One authoritative SQLite state and native Codex execution owner.
- No new planner/scheduler/daemon/harness, provider/global/credential changes,
  Review workers/resumes, Devsy workspace, Executor changes, merges/tags/releases.
- No real factory run on Josh's behalf; preserve recoverable 6cc2062 live state.
- Serialize heavy checks, use the existing two-CPU/four-GiB container/cache and
  check disk headroom. No broad cleanup/prune.
- All Dakota container/image/release/full Rust/expensive whole-suite work acquires
  the shared exclusive nonblocking `heavy-slot.lock`. If busy, do source/light
  work instead of polling. GitHub is the visible tested-checkpoint surface; private
  file handoffs remain machine coordination. The enabled experimental internal
  board has no exposed usable read/post operations in this thread/subagents.

## Review focus

1. Legacy NEEDS_INPUT with consumed answers and uncertain dispatch must migrate
   without a duplicate native call or budget reset.
2. Source changes or contradictory native observations must demote old success,
   while a live/unknown writer retains its claim.
3. Proposed/observed child work is not pre-spawn-enforced by native Codex;
   the ledger and UI must expose this limit rather than invent admission proof.
4. UI and direct tool calls must share fresh revision/action checks; an unknown
   native/external effect offers reconciliation, never an unsafe retry.
5. Unsupported output-completeness/clean-environment or semantic proof must stay
   unproved rather than being replaced by prose, positive fixtures or ratings.

## Slice 1: Typed control, admission and additive persistence

Files: add `server/src/control.rs`, `server/tests/control.rs` and a donor-cited
`tests/fixtures/control-conformance.json`; modify `store.rs`, `lib.rs`,
`lifecycle.rs` and the package input inventory as needed.

Interfaces: typed `Control`, `Task`, `Attempt`, `EventEnvelope`, `Reason`;
`reduce(current, envelope) -> Transition`, `admit_task(control, candidate)`,
`authorize_action(control, action, context) -> Permit` and replay with no effects
executed. Store applies expected-revision transitions transactionally and preserves
the existing request fingerprint/claim tables.

- [ ] Write and run failing cases for stale revision, duplicate/conflicting event,
  missing dependency/cycle, stale assumption, foreign/unknown claim, budget reset,
  separate goal/dispatch generations, worker return without acceptance and legacy
  migration preserving decisions/claims/identities.
- [ ] Implement bounded pure types/reducer/admission; migrate legacy proof to
  unproved and fail closed on corrupt version/state.
- [ ] Wire Factory-owned start/resume/intent and return paths through the control
  state before native effects. Preserve existing native guards and no-replay logic.
- [ ] Run focused Rust tests, then existing Rust regression suites. Review the
  actual production call path and commit an independently coherent slice.

## Slice 2: Current observed checks, acceptance and uncertainty

Files: add `server/src/evidence.rs`, `server/tests/evidence.rs`; modify
`lifecycle.rs`, `native.rs`, `store.rs` and native fixture producers without
dropping existing regression cases.

Interfaces: typed `CheckSpec`, `ObservedCheck`, `EvidenceBinding`,
`reconcile_evidence(control, receipts)`; read-only bounded file verifier at the
adapter boundary; native facts remain scoped to matched native identities.

- [ ] Write/run failures for prose-only acceptance, wrong task/attempt/intent/
  subject, missing/invalidated assumptions, contradictory/nonzero/truncated/
  unknown checks and worker self-completion.
- [ ] Observe/check actual current source and identity; require explicit owner
  semantic acceptance and matching structured current check references.
- [ ] Preserve lineage across repair, require diagnosed no-progress, retain
  uncertain effect intents/acknowledgements and refuse automatic retry.
- [ ] Test crash points before/after intent, acknowledgement and persistence;
  restart reconstructs identities without dispatch. Re-run existing descendant/
  terminal cancellation and decision-answer cases. Commit the coherent slice.

## Slice 3: Authoritative presentation and ChatGPT actions

Files: add `server/src/presentation.rs`; modify `http.rs`, `mcp.rs`,
`ui/src/{domain,controller,view,main,style}.ts`/CSS and their focused tests.

Interfaces: `project(control, observed_context) -> Presentation` with action/
reason, criterion proof, tasks/attempts, identity/liveness, budget, claim and
deliverable. Existing MCP uppercase states remain compatible; additive revision
fields and any read-only reconciliation tool have a documented pin handoff.

- [ ] Write/run cases for stale DONE, live last attempt, unknown effect/claim,
  exhausted budget, stopped-unresolved vs verified completion, stale UI action,
  and direct MCP denial matching projected actions.
- [ ] Render objective, delta, needs Josh and one safe primary action. Fold task,
  criterion, routing and evidence detail; preserve accessibility/mobile/deep links/
  bounded model context. Never derive workers or a percentage from guesses.
- [ ] Run UI/typecheck/build, all focused Rust/package/repo checks and formatting/
  clippy; validate actual staged package and preserved branding. Fresh read-only
  acceptance review, then commit/push exact coherent head and observe CI.

## Slice 4: Exact-head live handoff

- [ ] Read current live runs/claims and source identity; retain online backup,
  immutable 6cc2062 package/config and rollback constraints.
- [ ] If safe, replace only the same Luna Factory acceptance service using the
  tested package and original database. Keep Cutover identity/client and other
  services untouched; do not start a real run.
- [ ] Verify live initialize/tools/UI/core projection, approved sandbox alias,
  standard icon, zero-inference reads and connected/healthy exact upstream.
- [ ] Update PR evidence/capability limits and PR #11 compatibility/pin handoff.
  Report exact tested/CI/live heads, unproved native/host gates and safe rollback;
  send the requested short Slack DM.
