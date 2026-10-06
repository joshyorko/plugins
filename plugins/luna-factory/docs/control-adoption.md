# Standalone control and evidence adoption

Baseline: Plugins `6cc2062070f0ee6b7e8c51422580bd0743f972bb`. This pass strengthens
the existing Rust service, SQLite history and MCP Apps workbench. Native Codex
remains the execution owner. There is no Review, OMP or Changeplane dependency,
second planner, scheduler, execution harness or daemon.

The previous ChatGPT numerical quality ratings were not measurements: they had
no controlled dataset, metric definition, denominator or live-native proof.
They are not acceptance evidence or an adoption input.

## Immutable donor identities

- **R**: Review self-hosted `6b18ff1c35df4eb144a61add9c69271484195c02`.
- **RG**: Review #275 `144c0a7f4fee4c4306a3e8d02b4c610ecb70877a`.
  This is unmerged; krun graph proof was blocked at the original inspection.
  Its source/fixtures establish no Plugins native conformance, and this PR does
  not depend on its merge.
- **CL**: Plugins changeplane/language `2bec221f0158ac8ef21a31324b2547b034d4dc4e`.
- **CP**: Plugins experiment/changeplane-fast-reference
  `6e0d19f1298a1f8b7963bd3cea8989e8274bc9e6`. Prototype, not production authority.

Paths below are relative to the named donor repository. Each SHA is verified
against its actual Git object; private snapshots and a per-file hash receipt
retain the inspected source without changing donor worktrees.

The refreshed Review handoff reports a settled packaged krun graph fixture at
test/docs head `b652f77570f070f64dd43b27c25cb4679e63e3c0`, still using production
source RG and OMP 18.4.12. Its host-loopback deterministic provider is explicit in
`tests/fixtures/luna-factory-graph-krun-host-provider.sh:69` and `:73` at that head.
This supersedes the earlier transport blocker only for that bounded fixture;
network isolation, current-base conformance and Plugins native execution are not
established. No additional Review runtime behavior is adopted from this result.

## Code-first adoption matrix

| Donor function and test | Decision / Rust target | Exact guarantee and regression |
| --- | --- | --- |
| R `image/extension/luna-factory/core/reducer.ts:114`, `reduce`; `tests/luna_factory.test.ts:800` | Adopt, `server/src/control.rs`, Store transition | Stale expected revision cannot overwrite current state. Same event ID and fingerprint is a no-op; different payload conflicts. Test stale/reordered/duplicate events through SQLite and Factory. |
| R `core/admission.ts:63`, `admit`; `tests/luna_factory.test.ts:185`, `:196` | Adapt, pure task admission on Factory dispatch path | Mandatory A# binding, explicit necessity judgment, current assumptions, dependencies, acyclic graph, effects, claim and remaining budget precede managed dispatch. Missing prerequisites/cycles/foreign or unknown claims never become READY. Necessity remains a recorded owner/user judgment. |
| R `core/model.ts:198`, `Attempt`; `core/reducer.ts:145`, `start_attempt`; `tests/luna_factory.test.ts:1208` | Adopt, task/attempt ledger | Separate intent generation, execution dispatch generation and source subject. Recording an identity/acknowledgement is not execution proof. A worker return is VERIFY until bound checks and explicit owner acceptance. |
| R `core/evidence.ts:61`, `reconcileReceipt`; `tests/luna_factory.test.ts:747`, `:761` | Adapt, typed checks and runtime evidence | Wrong task/attempt/generation/subject, contradictory results, missing check references, aborted/truncated/unknown output or required environment evidence cannot certify a criterion. Runtime facts, worker claims and semantic acceptance remain separate. |
| R `core/evidence.ts:173`, `taskProofCurrent`; `tests/luna_factory.test.ts:1088`, `:1166` | Adapt, assumption invalidation | Relevant assumption changes invalidate affected proof. Subject movement invalidates current-subject proof; unaffected retention requires explicit scoped justification/revalidation, never a blanket assumption that old results still apply. |
| R `core/reducer.ts:481`, repair/replan; `tests/luna_factory.test.ts:1222`, `:1248` | Adapt, retained attempt lineage/diagnosis | Repair cannot reset the original budget. Two no-progress attempts require a diagnosis before another repair; one bounded same-goal replan is not a new goal or an authority grant. |
| R `core/convergence.ts:46`, `evaluateRun`; `core/batch.ts:137`, `batchConverged` | Adapt, criterion/result projection | Counts and remaining gaps derive from current ledger proof. Quiescence means no useful admitted next step, not successful acceptance or physical cessation. Retain the standalone descendant/terminal proof and claim gate. |
| R `core/journal.ts:198`, `parseJournal`, `:305`, `readJournal` | Adapt, additive versioned SQLite migration/event journal | Validate newest state and bindings; fail closed on corruption. Preserve run/claim/decision/deadline/dispatch identities. Replay computes state/effects but executes no external mutation. The donor checkpoint parser is not a durability/locking proof. |
| R `omp/batch-service.ts:634`, `:719`; `tests/luna_factory_corrective.test.ts:109` | Adapt provenance requirement; reject OMP execution transplant | Failed predicates and exact repair lineage survive persistence. Simulated worker/reviewer tests prove fixture plumbing, not independent semantic judgment or native execution quality. |
| R `ui/projection.ts:294`, `retryEligible`, `:364`, `nextSafeAction`; `tests/luna_factory_projection.test.ts:211`, `:314`, `:457` | Adapt, `server/src/presentation.rs`, shared MCP/UI action gate | Stale DONE, unknown effects/claims/liveness, dependency waits and exhausted budgets have distinct reasons. A live final attempt remains wait, not a retry. No retry of uncertain push/PR/dispatch. |
| R `ui/actions.ts:4`, `dashboardActionAllowed`; `tests/luna_factory_dashboard_actions.test.ts:9`; R `ui/claims.ts:38` | Adopt principle, Factory mutation boundary | UI and direct MCP calls re-evaluate the same current action availability. A claim is not liveness, and idle/empty queues cannot release unknown writers. |
| R `ui/dashboard.ts:365`, `:412`; `ui/evidence-viewer.ts:41` | Adapt hierarchy; reject TUI widgets | Objective → what changed → needs Josh → one safe primary action. Fold task/criterion/attempt/routing/evidence details. Preserve accessibility, narrow layouts, deep links and bounded model context; no invented workers or percentage. |
| RG `image/extension/luna-factory/core/graph.ts:80`, `evaluateWorkGraph`; `tests/luna_factory.test.ts:2853`, `:2938` | Adapt matching dependency cases; defer selected-batch execution | Directed authoritative prerequisite edges block affected lanes; inferred/semantic relation hints do not grant admission. Do not port batch campaigns, workspace setup, GitHub execution or blocked krun transport. |
| CL `plugins/changeplane/skills/changeplane/references/language-contract.md:50`, `:66`, `:121`; `tests/fixtures/semantic_cases.json`, `stale assumption admitted`, `receipt omits evidence`, `terminal reopens` | Adapt typed bindings and stable reasons | Bind subject, assumptions, authority, actor and receipt. Keep vocabulary mapping explicit; its terminal/new-plan rule does not replace the standalone same-thread, same-goal decision/repair continuation. |
| CP `plugins/changeplane/core/changeplane/engine.py:33`, `evaluate_outcome`; `:58`, `evaluate_action`; `tests/test_engine.py:32`, `:37` | Reject weaker algorithms | Recursive dependencies lack a cycle guard; same-key prior receipt lookup lacks a full action fingerprint. Preserve stronger SQLite request fingerprint conflict handling and add bounded cycle rejection. |
| CP `engine.py:70`, `schedule`; `tests/test_engine.py:52` versus CL `language-contract.md:143` | Reject as conformance | Prototype scheduling of EVIDENCE_MISSING/CANDIDATE_MISMATCH is not READY-only admission. No broad ontology/evaluator/runtime transplant. |

`core/…`, `omp/…` and `ui/…` abbreviate
`image/extension/luna-factory/…` within R/RG only.

## State mapping and ownership

The Rust control ledger has typed task admission/execution states, a separate run
control/result, typed intent and dispatch generations, exact source subjects,
stable reasons, bounded assumptions/checks and attempt lineage. Existing uppercase
MCP run states remain a compatibility projection, not a second authority.

Review CANDIDATE/READY/RUNNING/VERIFY/DONE map to candidate/admitted work,
observed execution, returned work awaiting verification, and accepted current proof.
Its BLOCKED/UNKNOWN are reasons, not interchangeable failures. Changeplane WAITING
maps to unmet prerequisites; UNKNOWN maps to missing/stale/contradictory evidence;
REJECTED maps to denied admission. Changeplane terminal reopening is not silently
imported: existing supported native continuation keeps G1, increments dispatch
generation, retains attempts/deadlines/authority and obeys decision-ID fencing.

Identifiers have separate lifetimes: the durable run ID names the original
request; intent generation names acceptance meaning; task IDs name admitted work;
attempt IDs name retained execution lineage; dispatch generation advances for a
new native turn within that same intent; the source subject names exact bytes.
No repair or decision continuation silently creates a new goal or resets budgets.
Only explicit, authoritative prerequisite edges block admission. Inferred relation
hints, repository overlap and issue similarity are not dependencies or authority.

The initial managed owner task is the user's explicit bounded objective, tied to
all mandatory A# criteria. It is structurally admitted before Factory creates an
owner or starts/resumes a turn. Bounded owner-declared candidates and observed
native child tasks share that ledger. Native child identities/returns do not
fabricate necessity, READY admission, observed execution or DONE.

SQLite owns snapshots and a revision/event journal. Reducer results and intent
records persist before Factory-owned native effects; acknowledgements and observed
facts settle them separately. Duplicate/conflicting identities and lost
acknowledgements cannot cause an automatic replay. There is no second execution
owner. Existing repository approval/revocation, finish caps and claim identity
rules continue to gate admission and recovery.

## Evidence boundary

An owner `passed: true` and prose is a report, not observed proof. Every certified
mandatory criterion needs a current structured check reference plus explicit
semantic acceptance by the owned verifier/owner. Bind task, attempt, intent,
dispatch, subject and required assumptions. Failed/foreign/stale/conflicting
checks cannot certify it; an unknown remains unknown.

Read-only local file predicates can be checked independently by the daemon after
native cessation, using bounded regular files within the already approved source
root. Only a relative path/digest enters this boundary; no credential contents,
absolute paths, arbitrary command execution or external verifier harness is added.
Native command/thread observations retain exact identities and observed results;
missing upstream output-completeness or clean-environment evidence is explicitly
unverified. A successful command exit is execution evidence, not an oracle for
natural-language requirements. Semantic coverage/necessity remain owner judgments.

Native Codex has no verified pre-spawn task/action policy hook. The service enforces
its own admission and RPC intent/continuation boundaries. In-turn task constraints,
child selection and native external command policy remain cooperative/unverified;
no blanket hooks or parallel harness are used to pretend otherwise. Push/PR delivery
is not proven by a URL/prose, and merge/release/deploy authority is never granted.

Semantic acceptance is a bound owner decision record, not independent proof that
natural-language requirements are true. Structural admission and observed predicates
cannot manufacture that judgment. Likewise a projected action remains advisory
until revision, subject, claim, liveness, budget and authority are rechecked at the
actual Factory/MCP effect boundary. Conformance fixtures must exercise the Plugins
SQLite/Factory production path; copied donor fixtures are not live acceptance.

The initial explicit objective is the aggregate G1 rollup, not a second final
verification turn. Admitted necessary candidates need their own task/attempt-bound
current proof before they can be complete; another task's accepted criterion does
not certify them. Outstanding necessary or selected work blocks successful
convergence. Optional or rejected discoveries alone do not reopen a proven goal.
Proof is conservatively invalidated across new dispatches when unaffected retention
cannot be explicitly justified. Native child identities never receive inferred
admission, execution or completion proof.

Current checkpoint limit: check bindings still require the latest global attempt.
Sequential necessary tasks therefore cannot all retain current proof and cannot
converge. Multiple admitted records are not multi-task completion support. The
next alignment gate is explicit current re-attestation for each task's recorded
attempt, without accepting unknown execution or automatically retaining old proof.

## Migration, delivery and acceptance

Add versioned bounded state and events transactionally. Legacy prose imports as
unproved, never as fabricated observation. Preserve original claims, dispatch IDs,
native identities, decision-answer fingerprints, provider/profile/skill guards,
deadlines and repair use. Corruption fails closed. Nothing in migration or replay
starts/resumes native work or discards state.

Test failure cases through Factory and SQLite, not only a disconnected reducer.
Keep existing cancellation/terminal, decision, onboarding/revocation, branding and
package tests. A small donor-cited JSON corpus contains only genuinely matching
pure invariants, with intentional vocabulary/runtime differences documented.

Retain the exact 6cc2062 package/config and an online SQLite backup. Stage/test
before replacing the same loopback instance; active/uncertain runs or retained
claims prohibit hot-swap. Rollback must preserve the post-migration database and
must not erase new runs or switch around a live writer. No real factory run is
started by this delivery. New MCP fields/tools require a documented PR #11 pin/
compatibility handoff; that repo is not modified or claimed integrated.
