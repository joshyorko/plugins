# Luna Factory workbench UX audit

Scope: the MCP App in `plugins/luna-factory/ui` on `luna-factory/oci-v021` (PR #66, UI 0.2.1).
Evidence: the six fixture captures in `ui/test/visual-snapshots/` and the view source
(`src/view.ts`, `src/graph-view.ts`). No screenshots were attached to the request, so these
committed captures are the audited "supplied screenshots". They are synthetic fixtures, not
ChatGPT Desktop renders.

## Verdict

The workbench is a careful, honest frontend for the **run** record: one objective, one owner,
acceptance criteria, receipts. It is not built around the work you actually do, a **campaign**
across a parent issue's graph with parallel agents. Its safety design is good and should be
kept. Its information architecture, vocabulary and start flow are the wrong product.

## Findings, ranked

### 1. Wrong unit of work (critical)

- The sidebar lists runs. Each run is "one objective, one durable owner" (`view.ts` start
  editor copy). Parent issues, issue graphs, waves of parallel work and PRs are not first-class.
- The task graph sits at the bottom of the run page, captioned "Planning only · Execution has
  not been authorized · Revision 7 · Claim released". The thing you think in is the last thing
  shown, framed as a planning artifact.

### 2. Starting a campaign is a form (critical)

"New run" asks for repository, objective, **mandatory acceptance (required, one per line)**,
non-goals, finish authority, runtime profile, worker capacity, time limit in seconds and repair
attempts. Then you choose between "Create planning graph" and "Start run". That is the opposite
of `luna yolo #101`. Nearly all of it can be derived from the issue graph, the repository policy
and saved defaults.

### 3. Every region is a box (high)

The fullscreen run page stacks about ten boxed regions before the graph: heading, What changed,
Next safe action, Continue in ChatGPT, three proof counters, five metric tiles, then the graph
heading. The counters (Proven 1 / Failed 0 / Unproved 1) and tiles (Owner, Workers, Time left,
Repository claim, Deliverable) are exactly the CRUD-dashboard pattern you asked to avoid. At
1440×1900 the first selected task's editor still isn't fully visible.

### 4. Internal vocabulary in primary UI (high)

Seen in the captures: "Steer the owner", "Exact run", "Repository claim · owned · Retained",
"Deliverable · unproved · PR ready · f51105bf…:working-tree", "CANDIDATE · CANDIDATE",
"task-owner", "owner-123 · turn turn-123", "Revision 7", "Claim released". These are the
backend's correctness words. They belong behind an "Evidence details" disclosure.

### 5. Uncertainty is promoted to urgency (high)

- "NEEDS JOSH: Native owner liveness is unknown. Keep the claim held." This is unverified
  liveness presented as a human decision, which is the failure you named in Journey D.
- "Next safe action: Steer the owner", followed by "The server has not established a safe action".
  The primary button contradicts its own explanation.
- "Workers: 0 active · 1 unknown", "Owner: unknown" and "Deliverable: unproved · PR ready" all get
  the same weight as real state.

### 6. ChatGPT is treated as a side panel (high)

The "Continue in ChatGPT" box carries meta-copy ("Each button sends this exact run and revision
to the active conversation after your click. It does not start Factory work.") and three generic
prompt buttons. The inline card repeats the same three buttons plus "Review in Luna Factory",
which is four actions where the guidelines suggest at most two. It reads as a second chatbot
launcher. The composer, which is ChatGPT's real control, isn't used. Selection context is already
sent through model context (`bridge.ts`) but isn't visible to you.

### 7. The graph is a list of form-backed cards (high)

`graph-view.ts` renders nodes as stacked buttons. The inspector is an editor: a prerequisites
multi-select, an execution-preference select, "Propose prerequisites" and an "Execution targets
and qualification" ledger. That is a manual graph editor on the primary path, which is the next
anti-goal on your list.

### 8. No agents, PRs, checks or diffs (high)

There is no per-worker view, no coordinator/subagent relationship, no activity timeline, no PR
list, no check status and no diff summary. Workers appear only as a count. You can't answer
"who is doing what right now" or "is this making progress".

### 9. Overview is marketing, not mission control (medium)

The home heading "A little attention. A lot of progress." and the "Delta first" activity cards
don't show campaign health, parallelism or bottlenecks. Sidebar items truncate the very
sentence that explains state ("Native owner liveness is unknown. Keep th…").

### 10. Surfaces are scaled copies, not designed per surface (medium)

- Inline (840×900 capture): a small card on a large empty field, with no visual of progress.
- Mobile (390×844): the header and two buttons take about 15% of the screen, the error banner
  about 10%, and the first screen ends before any state below the title is visible.
- Thread inspector: the same page without the sidebar.

### 11. Manual refresh chores (low)

"Refresh", "Refresh graph", "Refresh repositories" and a 30-second poll put housekeeping on you.
Polling is fine. Asking you to press refresh shouldn't be needed in normal use.

## Keep these (they're good)

- Server-authoritative actions: buttons are disabled with reasons, and the server rechecks at
  the action boundary.
- Stale-response fencing, no optimistic "stopped" claims, idempotent start retries.
- "A state label alone is not proof": evidence-backed status and no fake progress.
- Labeled HTML forms as a fallback for native elicitation, ARIA live regions, host theme variables.
- Bounded model context: only selected-run fields go to the model, never receipts or logs.
- Self-contained single-file bundle with no remote assets.

The redesign changes what is shown and how. It does not loosen any of these.
