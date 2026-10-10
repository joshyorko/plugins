# Mission control: product design and contract reconciliation

Product Design parent: #67. First slice: #68 (UI only). Reconciled against `main` at `dd304ef`,
the squash of #66. The campaign entity (#77) is reconciled against `bec70a5`, the squash of #68.

## Product direction

- **Campaign Map is the primary view.** The plan's tasks are laid out in dependency waves.
- **Lanes (Flight Recorder) is a secondary view.** It shares the same selection.
- **Narrative.** Dispatch's "since you last looked" storytelling supplies the status sentences.
- **ChatGPT is the control surface.** The workbench supplies context, visual feedback, real
  decisions and navigation. It has no second chatbot and no routine configuration form.

## Truth rules

1. Every displayed value comes from a named projection field, or is a labelled derivation of one
   (for example waves, "prerequisites met", or "longest remaining chain" measured by count).
2. Missing data is omitted or labelled unavailable. It is never filled from fixtures or inferred.
3. **Needs you** is exactly `needsOperatorDecision`: an allowed answer, or a native approval.
   Planning refreshes, unverified outcomes, unknown liveness and inactive canaries never appear
   there.
4. Motion only follows a real state change between two server snapshots. Reduced motion disables
   it. Nothing loops to suggest activity.
5. Observed data is labelled "Observed in Codex" and GitHub data is labelled "reported by GitHub".
   Neither is Factory proof, and only Luna's criteria are proof.
6. Model treatments (Sol, Astra) render only from verified, policy-permitted routing (#80). Today
   any non-`gpt-6-luna` model can appear only as stop evidence, inside Evidence and audit.

## Surfaces

| Surface | Layout | Notes |
| --- | --- | --- |
| Global entrypoint (`open_factory`) | Home, then Campaign | Fullscreen. The bottom stays clear for the host composer. |
| Thread entrypoint (`open_factory_panel`) | Campaign for this conversation | Narrow, container-responsive. Waves become a vertical list. |
| Inline card (`get_factory_run` result) | Status, sentence, mini map, attention line | Auto height and no inner scroll. At most two actions: one ChatGPT ask and "Open Luna Factory". |
| Mobile | The same DOM as the narrow layout | Needs you and the story come first. Inputs are 16px or larger. |

## Screen hierarchy

```
Home: Needs you · Parent campaigns (campaign → plan → runs) · In progress · Planned · not started · History
  └ Campaign: header (status, proof phase, freshness) · story · next action / decision
      ├ Map | Lanes  (same selection)
      ├ Crew strip    (orbit: Luna plus one satellite per reported worker)
      ├ Inspector     (task: wave, waits on, unblocks, owner, proof, asks, attempts, plan editor;
      │                agent: role, liveness, bound task)
      ├ Proposal review (only when a proposal exists; explicit, revision-bound confirmation)
      └ Evidence and audit (one disclosure: criteria, delivery, model route, receipts, targets, identity)
```

## Lunar visual system

- **Neutrals** come from host style variables, in light and dark. **Accent (moonlight)**: `#4B55C8`
  light, `#A8B0FF` dark. The manifest `brandColor` matches it.
- **State hues**, always paired with a distinct glyph shape and text:

  | State | Color | Glyph |
  | --- | --- | --- |
  | done | green | filled check |
  | running | blue | half disc |
  | verify | violet | ring with dot |
  | ready | moonlight | chevron |
  | blocked | slate | bar, hatched node |
  | planned / unknown | faint | dashed ring |
  | needs | coral | diamond, reserved for real decisions |
- **Glyphs** (`ui/src/lunar.ts`):
  - `lunaMark`: crescent and orbit.
  - `phaseGlyph`: a moon lit by proven over mandatory criteria. It is always paired with the count
    text and never with a percentage, per `control-wire.md` "No percentage".
  - Coordinator crescent token and numbered worker satellites. Liveness changes the rim only:
    solid for active, plain for idle, dashed for unknown.
- **Typography**: host font tokens throughout. Tabular numerals for counts and times.
- **Depth**: a dot grid on the map canvas only, which is a functional spatial surface. No other
  decorative backgrounds or gradients.

## Motion storyboard

Every transition is triggered only by a diff between two server snapshots. All of them are
disabled under `prefers-reduced-motion`.

| Moment | Source signal | What the user sees | Today |
| --- | --- | --- | --- |
| Plan imported | New nodes in `get_factory_graph` | Nodes settle into their waves (420ms ease) | Available |
| Dependency satisfied | A prerequisite's state becomes `done` | Its edge turns solid green, and the dependent gains "prerequisites met" | Available (state diff) |
| Worker launched | A new `presentation.workers[]` entry | A satellite joins the orbit, and its token docks on the bound task | Liveness only. Binding needs #79 |
| Investigating a failed test | `check_result` failure with a thread | Lane segment changes to the held hue, and the inspector shows the failing step | Needs #75 and #78 |
| PR review-ready | GitHub PR plus required checks | A PR chip on the node shows check dots, plus a "Ready for review" line | Needs #78 |
| Ready for authorized integration | Authority granted and checks green | A separate "Ready to integrate" line, never auto-merged | Needs #81 |
| Verified Sol/Astra escalation | Policy-permitted observed route | One restrained solar glow (Sol) or star glint (Astra) on the agent token, plus a route line in the inspector. Unknown routes stay neutral. | Needs #80 |
| Blocked becomes actionable | Decision appears | The decision bar becomes the coral "Needs you" panel, and the inline card adds its attention line | Available |
| Campaign converged | `finished_verified` with all criteria proven | The phase reaches full moon, all waves settle green, with one soft halo pulse | Available (single run) |

## Stale, unavailable and unverified states

| Condition | Presentation |
| --- | --- |
| Disconnected | `data-connection="disconnected"`. Alert copy. Messages and expansion are disabled. Context is cleared. |
| Graph stale (run revision ahead of graph) | Last valid map kept. A stale notice. Plan edits and context are suppressed until the next read. Polling rereads it. |
| Old timestamps | Dates are shown when not from today. Lanes ends at the last event instead of "now" for records older than 6 hours. |
| No agents | "No agents yet. Execution hasn't started on this plan." Or "The server reports no agent identities." |
| Per-agent timeline | A hatched lane reading "Per-agent timeline not reported by this server" until #75. |
| Unknown reason code | "The server reported this action without further detail." Never the raw code. |
| Execution unqualified | Start stays disabled. The footer states that execution stays unavailable until independently qualified. |

## Contract matrix

Legend: **✅ implemented in #68** (or the issue named) · **🟡 implementable with current data** ·
**🔴 needs a new contract** (issue).

| UI component | Displays | Authoritative source | Today | Refresh | Stale / missing behaviour | Needed contract | Tests |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Home groups | Needs you, In progress, Planned, History | `refresh_factory` / `open_factory` → `runs[]`, `needsOperatorDecision`, `presentation.result` | ✅ | 30s app-only poll while visible | Last valid list kept, with the error shown | none (runs linked to a campaign move under it, below) | `mission.test.ts` home grouping, `dogfood.test.ts` |
| Campaign grouping | Campaign row (parent label, title, planned task count, status), then its plan, then any linked runs | `list_factory_campaigns` → `campaigns[]`: `parent`, `planning.tasks`, `planning_run_id`, `run_ids`, `status` | ✅ #77 | Read after each refresh and on connect | Last valid grouping kept and marked stale. A server without the tool keeps the ungrouped tiers. A real decision on a linked run still appears in Needs you | Multiple campaigns per repo #82 | `mission.test.ts` campaign grouping, `home-light.png`, `home-campaign-mobile-dark.png`, `live-home-campaign-light.png` |
| Open a campaign | Its planning run's Campaign Map, with the parent label in the header | campaign `planning_run_id` → `get_factory_run` / `get_factory_graph` | ✅ #77 | Same as the map | Same as the map | none | `mission.test.ts`, campaign captures |
| Row freshness | "Updated …" | `run.updated_at` | ✅ | Poll | Dated when not today. "not reported" if absent | `activity_at` / `created_at` #69 | `mission.test.ts` |
| Proof phase | Proven / mandatory | `presentation.criteria` (validated against control) | ✅ | Poll | "Proof unverified" without projection | none | `view.test.ts`, `mission.test.ts` |
| Since you opened | New receipts and update | `receipts[].created_at`, `updated_at`, session baseline | ✅ (session scope) | Poll | Nothing shown when unchanged | Persisted baseline #74, graph-change activity #69 | `mission.test.ts` since-tracker |
| Campaign Map waves | Tasks, edges, waves | `get_factory_graph.graph.nodes[].dependencies/state`, else `run.control.tasks` | ✅ | Read on open, plus reread when stale | Stale notice, plan edits locked | Issue numbers and URLs #69 | `mission.test.ts` layout, captures |
| Prerequisites met / chain | Derived overlays | Derived from node states and dependencies | ✅ (labelled derived) | Same | Hidden when not computable | none | `mission.test.ts` |
| Source label | Provider (`GitHub`) | `node.source.provider`, `item_id` | ✅ provider only | Same | Opaque ID only in the inspector | `display.number/url` #69 | captures |
| Crew / orbit | Coordinator plus workers, liveness | `presentation.owner`, `presentation.workers[]` | ✅ | Poll | "No agents" copy | Roles, agent tree #75 | `mission.test.ts` roster |
| Token docking | Agent on task | `task.owner_thread` matches a reported thread | ✅ | Same | Not docked when unmatched | Per-attempt worker binding #79 | `mission.test.ts` |
| Lanes run row | Receipt ticks over time | `get_factory_run.receipts[]` (≤20) | ✅ | On select, plus poll | "No receipts retained". Old records end at last event | Thread attribution #69 | `mission.test.ts` lanes |
| Lanes agent rows | Per-agent events | none today | 🔴 shown as unavailable | | Hatched "not reported" | CAS timeline adapter #75 | `mission.test.ts` |
| Inspector task | Wave, waits on, unblocks, owner, proof, attempts | graph node, criteria, attempts | ✅ | Same | "No criterion bindings", "No agent assigned" | Typed blockers #70 | `graph-view.test.ts`, captures |
| Shared selection → ChatGPT | Run, task and agent context | Existing fenced `syncContext` plus `agent_label/role/liveness/task_id` | ✅ | On selection | Cleared on overview, disconnect, teardown or stale | `openai/title` chip #74 | `mission.test.ts`, host capture context assertion |
| ChatGPT asks | summary / blocker / choose | `buildFollowUpPrompt` (≤1800 chars, current revision) | ✅ explicit click, `send:true` | n/a | Disabled when the host lacks messages or the view is stale | Desktop `send:false` prefill #74 | `follow-up.test.ts`, `view.test.ts` |
| View tools | Model moves the view | none | 🔴 | | | #74 | |
| Decision panel | Question plus primary action | `pending_decision`, `primary_action`, `actions[]` | ✅ | Poll | Legacy runs show refresh only | Typed blockers #70 | `view.test.ts` |
| Plan editor and proposal review | Explicit propose, then confirmed apply | `propose/apply_factory_change` with fencing | ✅ (unchanged from #66) | After mutation, the run is reread | Locked when stale, disconnected or pending | Clear target #73 | `dogfood.test.ts`, `graph_e2e.test.ts` |
| PR chip / checks / diff | PR state, check dots, +/- | none | 🔴 | | Omitted | #78 | |
| Ready for review vs integration | Two distinct states | none | 🔴 | | Omitted | #78, #81 | |
| Parent intake | Campaign for one parent item, with a planning graph | `create_factory_campaign` (model and app): caller-supplied `parent` assertion, labelled "reported by GitHub", never proof | ✅ #77 for a supplied parent | After creation, the next home read | Duplicate parent rejected (`campaign_parent_exists`). Same key with a new payload rejected | Authenticated GitHub fetch from `owner/repo#N` #76 | `server/tests/campaign.rs`, `live-plan-capture.mjs` (#67 from the recorded snapshot) |
| "luna yolo" from `owner/repo#N` | Fetch, plan and run in one ask | none | 🔴 | | Home hint: "Ask ChatGPT to plan work" | #76, #71 | |
| Parallel workers | Multiple workers on independent issues | Children observed only | 🔴 | | Liveness only | #71, #79, #61 | |
| Sol / Astra treatment | Verified escalation | `route` is reroute-only today | 🔴 | | Neutral. Reroute shown as stop evidence | #80 | `mission.test.ts` reroute |
| Execution | Start, steer, stop | `capabilities.execution.eligible` (false) | ✅ gated | | Start disabled | #71 | `dogfood.test.ts` |
| Campaign promotion | Plan into execution | `promote_factory_campaign` fails with `execution_not_qualified` while `capabilities.execution.eligible` is false. `promotion.allowed` is always `false` | ✅ #77 fails closed | | No UI control. No event, revision or record is written | Qualified execution and a promotion transition #71 | `server/tests/campaign.rs` promotion |
| Multiple campaigns per repo | | One claim per repository | 🔴 | | | #82 | |
| Brand mark asset | Logo and composer icon | `assets/logo.*` (older purple tile) | 🟡 | | | #72 | |

## Implementation order

```
#68 UI slice (current contracts)
 ├─ #73 clear planning target (UI fix)
 ├─ #69 receipt attribution, created_at, source display ─┬─ #74 slice 2: handoffs, view tools, mentions
 │                                                       └─ #77 campaign entity ─┐
 ├─ #70 typed blockers                                                           │
 ├─ #75 CAS agent timeline (needs a loopback-forward decision)                   │
 ├─ #76 GitHub App intake ── #78 PR/CI telemetry ────────────────────────────────┤
 ├─ #71 qualify native execution ── #80 routing policy and telemetry             │
 │                              └── #79 parallel dispatch (+#61, #77) ── #81 authorized integration
 │                                                                     └─ #82 multiple campaigns per repo
 └─ #72 brand asset unification
```

### #77 ordering note

#77 does not wait for #69. It needs only the parent's display number and URL, so it
carries them on the campaign `parent.display` and leaves graph-node `source`
unchanged. #69 still owns display data for graph nodes. Promotion is in the
contract, but it fails closed until #71 qualifies execution and defines the
transition. #79 (parallel dispatch) and #82 (multiple campaigns per repository)
build on this entity. Today it allows one campaign per repository identity and
parent item, and it has no linked execution runs.

## Acceptance evidence for #68

- `npm run typecheck` and `npm test`: 169 passed, 1 skipped (the opt-in compiled e2e, which runs
  in CI).
- `npm run test:host`: 18 real-browser MCP Apps AppBridge captures with synthetic fixtures. Every
  capture is checked for:
  - read-only tool calls
  - zero automatic messages
  - no horizontal overflow or clipped controls
  - composer separation
  - agent context without raw thread fields

  The manifest binds them to the UI source hash.
- Fixture labels:
  - **Planning campaign**: shaped like an `import_candidates` result, which is producible today.
  - **`fixtureSwarmRun`**: explicitly synthetic, because execution is unqualified here.
- **Not claimed**: authenticated ChatGPT Desktop rendering. That needs a separately approved
  preview cutover.
