# ChatGPT workbench dogfood findings

This correctness campaign belongs to draft PR #66 and issue #59. Deferred design is tracked in issue #67. Product Design
owns the larger conversation-first issue graph and swarm mission-control direction.
These fixes preserve the existing workbench and execution authority.

## Evidence boundaries

- The maintainer reported authenticated ChatGPT observations on the independent
  Cutover connection: false attention for an inactive planning canary, an empty
  prerequisite change advancing revision 6 to 8, clutter and compact layout
  problems, and successful run/task context handoffs. These are operator-reported
  before-fix observations; original authenticated screenshots are not bundled here.
- A read-only snapshot of the live planning canary confirmed no pending decision,
  owner, workers, or held claim, and missing criterion evidence. Its existing
  history was neither repaired nor used as a test fixture.
- `ui/test/dogfood.test.ts` uses independent synthetic run/task identities.
  `server/tests/graph.rs` uses fresh temporary repositories and SQLite databases.
  `ui/test/graph_e2e.test.ts` uses a compiled server, real MCP HTTP, the production
  controller, and the SDK's actual AppBridge in an independent disposable ledger.
- `ui/test/dogfood-host.html` mounts the production app through an actual
  AppBridge/PostMessageTransport. Its ten browser captures are synthetic host
  evidence, not authenticated ChatGPT Desktop acceptance. The runner checks
  horizontal overflow, keyboard focus, large host fonts, composer separation,
  teardown, and zero mutation/execution calls or automatic messages.
- The corrected candidate has not been deployed to the live preview. Updated
  authenticated ChatGPT behavior remains **UNVERIFIED** until operator approval
  for a consistent preview update and subsequent host observations.

## Findings matrix

| Finding | Classification | Before and source correction | Regression and source status | Owner / deferred question |
| --- | --- | --- | --- | --- |
| BUG-01 False Needs me | Correctness | Unverified/stopped results were automatically attention items. `ui/src/domain.ts::needsOperatorDecision` now requires an authoritative allowed answer or native approval action. Inactive plans, blockers and uncertain evidence remain inspectable in history. | PASS: `ui/test/dogfood.test.ts`, `ui/test/domain.test.ts`; compiled fixture checks planning classification. | Engineering fixed. Product Design can choose a separate visual treatment for blockers and inactive plans. |
| BUG-02 Refresh as human judgment | Correctness and safe copy | A planning refresh appeared under Needs Josh. `ui/src/view.ts::renderRun` presents plan navigation separately; refresh remains a read and follows server eligibility. | PASS: dogfood DOM and compiled MCP/AppBridge regressions. | Engineering fixed; no new decision semantics. |
| BUG-03 Inactive execution audit | Safe UX | Planning runs emphasized a 0-minute timer, unknown owner and unproved deliverable. Planning now says execution has not started, time is inactive, and execution/delivery internals are behind an audit disclosure. Criterion uncertainty is retained honestly. | PASS: dogfood fixture, server planning projection and browser captures. | Engineering fixed the misleading state. Repeated panel hierarchy is deferred to Product Design. |
| BUG-04 No-op revision mutation | Correctness | Empty unchanged prerequisites wrote proposal and apply events. `server/src/graph.rs::ensure_meaningful` rejects new unchanged dependency sets/targets at lifecycle command boundaries, without changing the historical reducer. | PASS: no-op, dependency-order, stale revision, replay, and historical journal regressions in `server/tests/graph.rs`; real MCP rejection in graph E2E. | Engineering fixed. See `docs/control-wire.md` for the rejection/replay contract. |
| BUG-05 Navigation versus mutation | Correctness and safe UX | Editing controls sat beside navigation and application needed only one click. The candidate editor is secondary, explains proposal versus application, and requires confirmation of an exact proposal/revision. No-op submissions, duplicate clicks, stale/disconnected views and recovered proposals are gated. | PASS: dogfood, graph/controller, graph-view and browser proposal-review tests. | Engineering fixed safety. Product Design owns the eventual conversational editing workflow. |
| BUG-06 Current model context | Correctness | Run context omitted its revision and late/reconnected graph data could remain shareable. Context includes current run/node/graph identities and revisions; mutations reread the authoritative run; stale/disconnected snapshots cannot send follow-ups. Switch/overview/host teardown clears attachment data. Restored node context needs its revision fence. | PASS: production HostBridge tests, dogfood message tests, and compiled AppBridge message/reconnect/persistence regressions. | Engineering fixed. Actual run-level handoff was previously observed by the operator; corrected host behavior remains unverified. |
| BUG-07 Compact host layout | Safe UX plus larger design | Fixed minimum widths and long fields risked overflow. Container-aware graph stacking, wrapping controls/identities, host font tokens and stable focus address the bounded defects. | PASS: ten real-browser synthetic AppBridge captures with width/controls/composer/focus assertions and a source hash manifest. | Product Design owns graph representation, repeated status density, conversation-first entry, and enjoyable mission control. No broad redesign shipped here. |
| BUG-08 Backend qualification | Correctness and intended policy | Configured native stdio was selectable as if usable execution. Server capabilities explicitly report unqualified execution; the UI fails closed when qualification is absent. Native remains an optional secondary planning note. Disabled CAS and unsupported Cloud adapters are absent from selectable notes but visible in qualification audit. | PASS: backend/output-schema, controller and DOM capability gates. Authentication, entitlement and real execution remain UNVERIFIED. | Engineering fixed presentation. No execution, auth/provider path, or Cloud API qualification was added. |

## Reproduce locally

Run from `plugins/luna-factory/ui`:

```sh
npm ci
npm test
npm run build
npm run test:host
LUNA_GRAPH_E2E=1 LUNA_FACTORY_BINARY=/absolute/path/to/disposable-build/luna-factoryd npm test -- --run test/graph_e2e.test.ts
```

The browser runner uses pinned `playwright-core` and an existing compatible Chromium
cache. If absent, provision the test browser with its supported
`npx playwright-core install chromium` command or set `LUNA_CHROMIUM_EXECUTABLE` to
a compatible test executable. The runner binds a random loopback fixture port and
closes its browser/server. It never connects to live Dakota services.

Run the focused backend regressions with the repo's pinned containerized Rust
toolchain: `cargo test --locked --test graph --test output_schemas --test backends`.
Native execution is unavailable in graph fixtures. The broader Rust suite uses
fake native transports; its live audit stays opt-in/ignored.

## Remaining gates

1. Check all four hosted gates on the final PR SHA.
2. Product Design direction remains a separate reviewable task; no swarm execution
   or broader permissions follow from the desired design.
3. Before updating the live preview, obtain operator approval for matching
   server/UI bytes and a consistent isolated SQLite/WAL backup and rollback plan.
   Preserve the original blocked ledger and held claim, all current previews,
   Cutover/Executor routes, credentials and native sessions.
4. In the existing authenticated ChatGPT Luna app, verify corrected attention,
   explicit proposal/apply, current node context/messages, refresh/reconnect and
   actual inline/thread/fullscreen rendering. A browser simulator cannot close
   these host acceptance gates.
