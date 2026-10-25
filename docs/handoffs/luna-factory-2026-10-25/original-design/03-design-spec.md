# Luna Factory: mission control design specification

> **Superseded for engineering** by `plugins/luna-factory/docs/mission-control.md` in PR #68. That file reconciles this design against the code at `main` (`dd304ef`) and carries the issue graph #69–#82. This file is kept as the design rationale.

Status: direction-selection draft, 2026-10-10. No production code changed.
Inputs: `01-ux-audit.md`, `02-platform-capabilities.md`, `mockup-fixture.md`, mockups in `mockups/`
and screenshots in `shots/`. The backend facts below come from a read-only inventory of
`plugins/luna-factory` on `luna-factory/oci-v021`, including uncommitted working-tree edits
present at read time.

---

## 1. Product model

### 1.1 The noun is "campaign"

You think in parent issues and issue graphs. The UI's primary noun becomes the **campaign**:

| User term | Meaning | Today's backend equivalent |
| --- | --- | --- |
| Campaign | A parent issue plus its child issue graph, worked toward a finish line | Closest is a planning graph (`create_factory_graph`) plus one executing run. There is no campaign entity. |
| Issue | A child of the campaign (GitHub issue) | Graph node / task with `source{provider,item_id,revision}` |
| Wave | Issues whose dependencies are all satisfied at the same depth | Derived client-side from `dependencies` |
| Luna (coordinator) | The owner agent that plans and dispatches | Run owner thread (`presentation.owner`) |
| Worker | A parallel agent working one issue | Native child thread (`presentation.workers[]`); bound to a task via `attempt.thread_id` / `task.owner_thread` |
| Helper (subagent) | A child spawned by a worker | Observed native descendant; the tree isn't exposed |
| Decision | A real question only you can answer | `pending_decision`, or native approval (`NEEDS_INPUT` without a decision) |
| Luna is handling | Retries, repairs, waits, polling, reconciliation | `wait` / `reconcile` / `refresh` primary actions, `unverified`, repair budget |

Internal words stay behind **Evidence details**: run, revision, claim, turn, subject, generation,
dispatch, hashes.

### 1.2 Attention taxonomy (Journey D)

Every state maps to exactly one of four tiers. The tier decides placement and color. Color is
never the only signal.

1. **Needs you**: a human decision is required and the server says so. Sources:
   `pending_decision` (answerable in ChatGPT), native approval (`native_approval_requires_native_ui`,
   answer in Codex), `diagnosis_required`, a protected-resource or authority-ceiling request
   *(backend: typed blocker category)*. Coral, with a question-mark glyph. Pinned first everywhere.
2. **Ready for you**: optional but valuable, such as a PR ready for review or merge when Luna's
   finish authority stops at "Open PRs". Neutral ink with a check glyph. Never coral.
3. **Luna is handling**: repairs, retries, runner queues, reconciliation, unknown liveness being
   rechecked. Grey, with a clock or circular-arrow glyph. Shows what Luna will do next and when.
   **Never escalates by itself.** It escalates to "Needs you" only when the server sets a decision
   (for example, repair budget exhausted, then `diagnosis_required`).
4. **Unverified**: something the server can't prove, such as an exit 0 without a bound check, a
   PR detected from a command string, or unknown effects. Rendered inline as a dotted-underline
   qualifier ("PR opened · unverified") with an explanation on hover or focus. **Never a
   standalone alert.**

Inactive canaries, polling cycles and fixture data never appear as attention.

---

## 2. The three directions

All three use the same campaign (`mockup-fixture.md`) and the same attention taxonomy.

### A. Flight Recorder (time × agents)
Swimlanes per agent over wall-clock time, like a tracing waterfall. Coordinator on top, workers
below, helpers as tethered sub-lanes. Event glyphs on segments. A dependency strip is secondary.
- **Answers best:** "Is it making progress?", "what did each agent do?", "where did time go?"
- **Weakest at:** dependencies and "what's ready next". It's poor inline, since time axes need
  width, and it's dense on mobile.
- **Backend cost: medium, if it reads from codex-action-server (CAS).** Luna's own store has no
  per-agent timeline: attempts have no timestamps and receipts aren't attributed to a thread.
  CAS (`joshyorko/codex-action-server`, 1.0.2 at `172.30.86.1:8088`) already passes through the
  native Codex history reads for each thread:
  - `list-thread-timeline` (`thread/timeline/list`)
  - `list-thread-turns`, `list-thread-items`
  - `get-thread-snapshot`, `read-thread`
  - `list-loaded-threads`, `list-background-terminals`
  - `read-dispatch-receipt`

  `list-targets` currently returns `local`, `devsy` and `cas-worker-01`, but per Josh **only
  `local` is real**. The other two are leftover entries from the static `CODEX_ACTION_TARGETS`
  file or the dynamic `CODEX_ACTION_DYNAMIC_TARGETS` registry. A listed target is configuration,
  not a live or qualified environment. Lanes may show an agent's location only after
  `inspect-target` resolves it, and never from the list alone. Luna already has a read-only CAS
  adapter (`inspect_factory_cas`). See §8.1 for what it would take to feed lanes from it safely.

### B. Campaign Map (the issue graph is the world)
The parent's graph is auto-laid out into dependency waves. Agents are tokens docked on the issues
they work, with tethers from the coordinator. Pulse rings appear only on real recent events.
Completed waves settle. A frontier marks what's ready.
- **Answers best:** "Which issues are done, blocked, ready or in progress?", "who is on what?",
  "what blocks the release?" It mirrors the artifact you already build.
- **Weakest at:** time history, which moves into the inspector. Big graphs need collapsing, which
  waves give you naturally.
- **Backend cost: medium.** Nodes, dependencies, state, `owner_thread`, source and attempts exist
  today. Parallel workers, PR/checks and the agent tree need work.

### C. Dispatch (attention-first briefing)
Opens on "Since you last looked": a headline sentence, grounded change lines, decisions typeset as
questions, a flow river (Waiting → Ready → In progress → In review → Merged) and a crew line.
- **Answers best:** periodic check-ins, Journey D, inline and mobile.
- **Weakest at:** "living swarm" delight, the parallelism picture and dependency structure.
- **Backend cost: lowest.** Run list, state, decisions, receipts with `created_at` and criteria
  exist. The "since last looked" diff can be computed client-side from snapshots and receipts.
  Change lines about PRs and checks need GitHub data.

### Recommendation: B, Campaign Map, with C's briefing as its entry layer

Your workflow is: build an issue graph, say "luna yolo #101", check in periodically, steer by
chat. Direction B is the only one whose primary visual is the thing you actually author and
reason about, the issue graph. It answers six of your eight "instantly understand" questions on
one screen:

- campaigns
- which agents are working
- issue states
- distribution across parallel agents
- coordinator→worker ownership
- where attention is needed

It degrades honestly to today's backend, because the graph exists and agent tokens can show
liveness only. Its waves are also a natural progressive-disclosure device for graphs of dozens of
issues.

Why the map is the primary view rather than the lanes, now that CAS makes lanes affordable:

- **Authority.** The map is drawn from Luna's authoritative graph state. Lanes are drawn from
  CAS observations that are explicitly unverified. The main picture should be the one Luna can
  stand behind. Observed activity is a layer on top.
- **It ships first.** The map's P0 version runs on today's tools, while lanes need the §8.1
  adapter work and loopback forwarding.
- **Your mental model.** You plan in issues and dependencies. Lanes answer "what did the agents
  do?", which is the second question you ask, not the first.

Take two pieces from the others:
- **From C:** the "Since you last looked" sentence becomes the top line of the campaign view, the
  inline card and mobile. It's the answer to "is this making progress?" in words.
- **From A:** a **Lanes** view in the same campaign screen, toggled with Map, plus per-agent
  timelines in the inspector. It's fed by CAS (§8.1), labelled "Observed in Codex", and ships at
  P1.5. Lanes scale with worker count (at most 8), which complements the map, which scales with
  issue count.

Close second: A. If over time you find yourself watching agents more than issues, make Lanes the
default view. The spec supports either default without structural change.

---

## 3. Screen hierarchy (recommended direction)

```
Global entrypoint (sidebar "Luna Factory", fullscreen)
└── Home: all campaigns
    ├── Needs you                 (cross-campaign decisions, max 3 visible + "n more")
    ├── Live campaigns            (featured live campaign + rows: mini wave map, now-sentence, crew)
    └── Recently finished         (one line each)
        └── Campaign (fullscreen)
            ├── Header line       ("#101 Actions v2 · 3 of 13 merged · since 13:10: …")
            ├── Wave map          (primary canvas; toggle Map | Lanes, where Lanes comes from Direction A via CAS, P1.5)
            ├── Crew rail         (left; Luna + workers + helpers, task, last activity)
            ├── Pulse rail        (right; meaningful events, newest first)
            └── Inspector         (right sheet over the map, for an issue or agent)
                ├── Summary       (why this exists, depends on / unblocks)
                ├── Now           (current step, owner, helper, budget)
                ├── PR & checks
                ├── Changes       (diff summary)
                ├── Timeline      (from A)
                └── Evidence details   (disclosure: criteria, receipts, internal IDs)

Thread entrypoint (side panel in a conversation)
└── The campaign bound to this conversation (same Campaign view, narrow layout)

Inline card (in conversation, after a Luna tool result)
└── Campaign pulse: now-sentence · mini wave map · "1 decision needs you" · [Open campaign]
```

Advanced configuration (default capacity, finish authority, profile, budgets, repositories,
execution targets) moves to **ChatGPT structured settings** (`openai/settings`, already wired)
plus a "Campaign settings" overflow item. It's never on the primary path.

---

## 4. Interactions

### 4.1 Journey A: start a campaign
1. You: "Use Luna Factory to finish the issue graph in joshyorko/actions. Parent issue #101. YOLO it."
2. The model calls a read tool that inspects the parent and its children, returning:
   - graph shape, waves, and issues with no acceptance signal
   - protected paths touched
   - eligibility: repository approved, finish ceiling, execution qualification
3. The model replies in chat with **only the necessary decisions**, for example: "13 issues in 4
   waves. #113 touches protected `release.yml`. I'll stop at open PRs. Start?" If structured input
   is truly needed (authority boundary), it uses an OpenAI form (`openai/elicitation`). The HTML
   fallback is the existing accessible form.
4. On confirmation the model calls the start tool. The inline **campaign pulse** appears showing
   wave 1 dispatching, with one primary action: "Open campaign".
5. If execution isn't qualified on this host, the card says so plainly ("Planned only: execution
   isn't qualified on this connection") and offers no start affordance. It never pretends to run.

### 4.2 Journey B: observe
- Opening the global or thread entrypoint lands on Home or the bound campaign.
- Liveness: the view polls an app-only read tool while visible (today `refresh_factory` every
  30s). A freshness stamp ("updated 14:06") is always visible. Pulse rings run only for events
  newer than 5 minutes. Reduced-motion users get a static "new" dot.
- Pressing an issue or agent token opens the inspector. The map stays visible and dims to the
  selection's neighbourhood. Esc or the X closes it, and arrow keys move between nodes in a wave.

### 4.3 Journey C: steer through ChatGPT
- **Selection is context.** Selecting an issue or agent calls `ui/update-model-context` with a
  bounded payload and `openai/title` ("#109 · Worker 4"). ChatGPT shows it as a composer chip.
  You type "Why is this agent stuck?" and the model already knows what "this" is.
- **Suggested asks** (2–3, contextual) appear in the inspector footer. On desktop they prefill
  the composer with `ui/message {target:"active", send:false}` so you edit and send. On mobile
  they send directly, after an explicit tap.
- **The model can drive the view.** View-registered tools (Bits & Bolts pattern) such as
  `focus_issue`, `focus_agent` and `show_wave` let "show me Worker 4" move the fullscreen view.
  These change the view only, never the engine.
- **Mutations go through the model's tool calls.** The UI shows the outcome when the next poll
  lands. Server-authoritative actions keep their current fencing.
- **@-mentions.** Typing `@Luna Factory #109` in the composer resolves through
  `search_factory_mentions`. Extend it from runs and tasks to campaigns, issues and agents.

### 4.4 Journey D: human attention
- A decision is a question plus options plus consequence plus context links. Example: "#113 needs
  `id-token: write` in protected release.yml. Allow for this PR only / Skip #113 / I'll handle it."
- Choosing an option sends the choice to ChatGPT (`ui/message`, labelled "Answering sends your
  choice to ChatGPT"). The model calls `resume_factory_run` with `expected_decision_id`. The app
  never answers a decision on its own, so no answer bypasses the conversation's audit trail.
- Native approvals show "Answer in Codex" with the reason. The app can't answer them.

---

## 5. Agent visualization rules

- **Identity:** the coordinator token is filled ink with the moon glyph "L". Workers are outlined
  W1…Wn, numbered by dispatch order and stable for the campaign. Helpers are small tokens
  tethered to their parent worker.
- **State glyphs** (shape and text, not color alone):
  - working: solid ring
  - repairing: ring with a circular arrow
  - quiet (no event in 10 minutes or more): hollow, dimmed
  - idle or free: dashed outline in the crew rail
  - unknown liveness: "?" with dotted outline, and the inspector explains it
- **Motion budget:** one 1.6s ease pulse per real event, then still. No looping spinners on the
  map, no particle effects, no fake typing.
- **Ownership:** a thin tether from Luna to each active worker token. Hovering a worker highlights
  its issue and its PR chip.
- **Today's data:** without per-worker task binding, tokens dock on the run's selected task.
  Workers without a binding sit in the crew rail labelled "working (task not reported)". No
  invented assignments.

---

## 6. Surfaces and responsive behaviour

| Surface | Layout |
| --- | --- |
| Fullscreen ≥ 1200px | Crew rail 240 · wave map · pulse rail 320. The inspector replaces the pulse rail as a 440px sheet. Bottom 96px reserved for the ChatGPT composer. |
| Fullscreen 768–1199 / thread panel | Map plus a collapsible crew strip on top. Pulse becomes a "Recent" toggle. The inspector is a full-height sheet. |
| Inline | Auto-height, no scroll. Now-sentence, mini wave map (dots by state, max 40 then grouped), attention line, one primary button and an optional secondary ("Ask why #108 is blocked" → prefill). |
| Mobile (< 600) | Single column. Needs you, then the since-you-looked sentence, then waves as a vertical stack (settled waves collapsed to "3 merged"), then crew. The inspector is a full screen. Composer at the bottom. |

Theme: the host theme variables via `styles.variables`. Light and dark are both first-class.
State colors are tuned for AA on both.

---

## 7. Progressive disclosure

1. **Visible by default:** campaign, issues, agents, PRs, checks, decisions, freshness.
2. **One click (inspector):** timeline, diff summary, failing check line, budget, dependency
   reasons.
3. **Evidence details disclosure:** criteria proof states, receipts, attempt lineage, internal
   IDs, revision, claim status, route and model. A flat list with no nesting.
4. **Settings (ChatGPT structured settings or overflow):** capacity, finish authority, profile,
   budgets, repositories, execution targets and qualification.
5. **Not in the UI:** graph editing. Dependency or target changes go through chat ("make #111
   wait on #109"). The model proposes them through `propose_factory_change`, and the inspector
   shows the proposal for confirmation.

---

## 8. Supported today vs. backend engineering

Legend: ✅ supported by current tools · 🟡 partially (derivable client-side, or limited) ·
🔴 needs backend engineering.

| Capability | Status | Today's source / gap |
| --- | --- | --- |
| List runs, states, needs/active/recent | ✅ | `list_factory_runs`, `classifyRun` |
| Real decision vs native approval vs unverified | ✅ | `pending_decision`, `result.kind`, action reasons |
| Answer a decision via chat | ✅ | `resume_factory_run{expected_decision_id}` |
| Issue graph nodes, dependencies, state, source issue | ✅ | `get_factory_graph` |
| Waves layout | 🟡 | Computed client-side from `dependencies` |
| Coordinator liveness, worker liveness | ✅ | `presentation.owner/workers` |
| Worker ↔ issue binding | 🟡 | `task.owner_thread`, `attempt.thread_id`. Only one task is dispatched at a time per run today. |
| Recent activity feed | 🟡 | Receipts (20, `created_at`, not attributed to an agent) |
| "Since you last looked" | 🟡 | Client snapshot diff plus receipts. Needs `created_at` on runs and a per-user "last seen" (widget state). |
| Selection → model context chip | ✅ | Existing bridge (8 bounded fields); extend to issue and agent |
| Suggested asks prefill (`send:false`) | 🟡 | Host supports it on desktop. Recheck the SDK typing; Bits & Bolts uses it. |
| View-registered tools (model drives view) | 🔴 UI-only | New UI work, no backend |
| Campaign entity (parent issue → graph → execution) | 🔴 | No campaign type. A planning graph can't become executable. |
| "yolo #101" intake from GitHub | 🔴 | No GitHub fetch. Node sources must be supplied by the caller. |
| Derived acceptance from issues | 🔴 | `acceptance` is required. Needs derivation that's visible and editable in chat. |
| Parallel workers on independent issues | 🔴 | Dispatch rejects a second active attempt. Parallelism exists only through native children. |
| Agent tree (coordinator → worker → helper) | 🔴 → 🟡 via CAS | `owned_threads` is flat. CAS collab/spawn items can supply parent→child links (§8.1). |
| Per-agent role, current step, timestamps | 🔴 → 🟡 via CAS | Not in Luna's store. CAS `thread/timeline/list` and `get-thread-snapshot` give them as untrusted observations (§8.1). |
| PR links, check runs, failing step | 🔴 | `deliverable.reference` is always null. No GitHub/CI data. Push/PR are never certified. |
| Diff summary (files, +/-) | 🔴 | Only a subject hash and `base_head` |
| Event journal / timeline | 🔴 → 🟡 via CAS | Luna's `control_events` isn't exposed. Per-agent native timelines come from CAS (§8.1). |
| Typed blocker category (protected file, authority) | 🔴 | Free text plus reason strings |
| Multiple concurrent campaigns in one repository | 🔴 | One executing run per repository claim |
| Execution itself on this connection | 🔴 (qualification) | `execution.eligible:false` (`authentication_entitlement_and_adapter_qualification_unverified`). Don't assume a manifest entry is qualified. |
| Push updates | n/a | The platform has no server push. Keep polling while visible. |

### 8.1 CAS as the observation source for agent lanes and timelines

Josh pointed out that codex-action-server can supply Flight Recorder-level detail. Checked
read-only on 2026-10-10. I fetched `/openapi.json` and called `list-targets`, and read no thread
content.

- **What CAS gives:** native, timestamped, paginated thread history for an exact
  `{target, cwd, thread_id}`. That includes timeline, turns, items (commands, file changes,
  messages, collab or subagent calls), snapshot status, loaded threads and background terminals.
  This is the per-agent event source that `presentation.workers` lacks. Luna already knows the
  owner and worker thread IDs, so it can address them.
- **What Luna's adapter allows today:** only `inspect_target`, `read_dispatch_receipt` and
  `read_thread`, against **literal loopback origins only**. `172.30.86.1` is not loopback, so it
  needs operator-approved loopback forwarding or a separately reviewed remote adapter. That's a
  documented blocker in `docs/cas-verification.md`. Every response carries
  `binding_verified:false`, `execution_eligible:false`.
- **Trust rule for the UI:** CAS observations are *untrusted observations*, not Factory proof.
  Lanes and timelines built from them render as **"Observed in Codex"**. They are visually
  distinct from Luna's proven criteria and never change attention tiers by themselves. A worker
  can look busy in the trace while its criterion stays "unproved". That's correct and the UI must
  show both.
- **Backend work (P1.5, read-only, no execution):**
  1. Extend the CAS allowlist to `list_thread_timeline`, `list_thread_turns`, `list_thread_items`,
     `get_thread_snapshot` and `list_loaded_threads`, with the same 10s timeout, 1 MiB limit and
     ≤3 requests per call budget, or a paged variant.
  2. Add an app-only tool `read_factory_agent_timeline {run_id, thread_id, cursor?}`. It
     normalizes native items into a small, redacted event vocabulary (`assigned`, `command`,
     `file_change`, `commit`, `pr_opened`, `check_result`, `subagent_spawned`, `message`,
     `waiting`, `error`) with timestamps and thread attribution. It returns no raw output, only
     `safe_summary` text.
  3. Resolve the agent tree from collab/spawn items: parent thread, then receiver thread.
  4. Cache per thread with a cursor so 30s polling doesn't multiply CAS calls.
- **Effect on the directions:** A's lanes and B's inspector Timeline become feasible before the
  full campaign engine. They can show "observed" activity even while execution through Luna
  stays unqualified.

---

## 9. Prioritized acceptance criteria

### P0: honest mission control on today's backend (UI only)
1. Home shows Needs you / Live / Recent across all runs. Each live item shows a now-sentence and a
   freshness stamp. No stat-tile grid.
2. Only `pending_decision`, native approval and `diagnosis_required` appear under Needs you.
   `unverified`, `reconcile`, `wait` and unknown liveness appear as "Luna is handling" or as an
   inline qualifier. Verified by fixture tests for each `result.kind` and reason string.
3. The campaign view renders the graph as waves computed from dependencies. Node state uses glyph
   plus text. Selecting a node opens the inspector without losing the map.
4. Internal identifiers (revision, claim, turn, subject, hashes, thread IDs) appear only inside
   Evidence details. A test asserts none of them in the primary DOM.
5. The inline card has at most 2 actions, no internal scroll, and auto height. It shows the
   attention line when a decision exists.
6. Selection sets model context with an `openai/title` chip label, and clearing the selection
   clears it. Existing stale-revision suppression is kept.
7. Every existing safety test still passes: fencing, idempotency, no optimistic stop,
   server-computed `allowed`.
8. Mobile 390px: the decision and the since-sentence are visible on the first screen.
9. Light and dark both pass AA. Reduced motion disables pulses. Every control is reachable by
   keyboard.

### P1: conversation as the control (UI plus small server changes)
10. Suggested asks prefill the composer on desktop (`send:false`) and send on explicit tap on mobile.
11. View tools `focus_issue`, `focus_agent` and `show_wave` let the model move the fullscreen view.
12. Mentions return campaign, issue and agent resource links.
13. A "Since you last looked" sentence comes from client snapshots and receipts. The last-seen time
    is stored in widget state.
14. Expose `created_at` and per-receipt agent attribution (`thread_id`) in projections.
14a. (P1.5) A read-only CAS timeline adapter and `read_factory_agent_timeline` (§8.1). Lanes and timelines are labelled "Observed in Codex", and observations never change proof or attention.

### P2: campaign engine (backend engineering)
15. A `campaign` entity: repository, parent issue, graph, coordinator, finish authority and
    defaults. A planning graph can be promoted into an executing campaign under revision fencing.
16. A GitHub intake read tool returns the child graph (sub-issues, "blocked by" links, task lists),
    derived acceptance candidates (marked "derived from issue"), and protected-path hits.
17. Parallel dispatch: up to `capacity` concurrent attempts on independent ready tasks, each bound
    to a worker thread and an isolated working copy. The repository claim stays per campaign.
18. Agent tree projection: coordinator → worker → helper, with role, bound task, last event time
    and current step summary (redacted).
19. Typed blockers: `protected_resource{path, permission}`, `authority_ceiling`,
    `external_dependency{kind, retry_at}`, `budget_exhausted`.

### P3: delivery visibility (backend plus GitHub)
20. PR reference, check runs (name, status, failing step summary) and review state per task.
    These are shown as "reported by GitHub", distinct from Luna's proven criteria.
21. Diff summary per PR: file list and +/- counts.
22. Expose the event journal as a bounded, paginated, redacted timeline per campaign and per agent.

---

## 10. Build handoff for the Luna team

**Guardrails (don't change):**
- server-authoritative `actions[].allowed`
- `expected_revision` / `expected_decision_id` / `expected_turn_id` fencing
- no force-unlock
- push/PR never certified as proof
- `safe_summary` redaction
- bounded model context
- self-contained single-file UI bundle
- no remote assets

**Phase 1 (P0, UI only, about 1–2 weeks):**
- New view modules: `home`, `campaign-map` (wave layout, tokens), `inspector`, `inline-pulse`,
  `attention.ts` (tier classifier with exhaustive tests over reason strings).
- Keep `bridge.ts`, `controller.ts` fencing and `domain.ts` validation. Replace `view.ts`
  rendering. Retire the stat-tile grid and the "Continue in ChatGPT" panel.
- Move start, settings and repository forms behind an overflow menu, and use ChatGPT structured
  settings where available.
- Rebuild the fixture scenarios around a 13-issue graph and capture new visual snapshots
  (still labelled synthetic).

**Phase 2 (P1):**
- `ui/message send:false` prefill
- view-registered tools
- mention types
- `created_at` and receipt `thread_id` in projections

**Phase 3 (P2/P3), engine work in Rust, each behind its own qualification evidence doc in the
style of `docs/*-verification.md`:**
- campaign entity and promotion
- GitHub intake
- parallel dispatch
- agent tree
- typed blockers
- PR/check/diff ingestion
- event journal tool

**Open questions for Josh:**
1. Should a campaign ever merge, or always stop at "Open PRs"? This decides whether "Ready for
   you" includes a merge action.
2. GitHub data source: should Luna read GitHub directly (a token held server-side), or should
   ChatGPT's GitHub connector supply the graph to Luna?
3. Should multiple concurrent campaigns per repository be supported (worktree isolation), or one
   at a time?
