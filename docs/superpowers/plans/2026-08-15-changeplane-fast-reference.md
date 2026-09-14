# Changeplane Fast Reference Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a runnable, repository-neutral Changeplane v1 reference that turns an observed repository snapshot and approved intent into deterministic, governed work admission, harness-neutral envelopes, exact evidence reconciliation, and a human-readable read-only MCP control room.

**Architecture:** The canonical skill defines normative language and conformance cases; a dependency-free Python engine enforces them with pure functions and immutable value records. A stdio MCP server reconstructs state from portable JSON, exposes read-only tools and a self-contained `ui://` control-room resource, while a demo fixture and eval suite prove the end-to-end seam without performing mutation.

**Tech Stack:** Python 3.11 standard library, `unittest`, JSON/NDJSON, stdio MCP JSON-RPC, self-contained HTML/CSS/JavaScript MCP App resource, existing repository generators.

## Global Constraints

- Work only on `experiment/changeplane-fast-reference`, based on `42f5d822d0163f715caaec2c0aacf33869d41dc9`; never mutate `main`, issues #39-#46, PR #45, or governed factory branches/worktrees/runtime.
- Language defines normative shapes, typed identities, bindings, invariants, transitions, reason codes, and conformance cases; Engine deterministically enforces them.
- Authority is explicit, default-deny, subject-scoped, and cannot be created by hostile input text.
- Evidence, predicates, proofs, checkpoints, drains, receipts, and acceptance bind exact model generation, plan generation, outcome, candidate, and subject where applicable.
- All MCP tools are read-only; they may return action requests and envelope previews but must never dispatch, mutate, merge, or claim execution.
- Default operator copy answers what happened, why work stopped, what needs the operator, and what approval would do; ontology/hash details remain drill-down.
- Generated plugin views are produced only by existing repository generators.

---

### Task 1: Canonical Language and Plugin Surface

**Files:**
- Create: `plugins/changeplane/.codex-plugin/plugin.json`
- Create: `plugins/changeplane/skills/changeplane/SKILL.md`
- Create: `plugins/changeplane/skills/changeplane/references/language.md`
- Create: `plugins/changeplane/skills/changeplane/references/conformance.json`
- Create: `plugins/changeplane/tests/test_language.py`
- Modify: `marketplaces/catalog.json`

**Interfaces:**
- Produces: normative JSON record fields and stable reason codes consumed verbatim by Tasks 2-4.
- Produces: canonical skill invocation `$changeplane` / `@changeplane` and `DISPATCH TO AGENT` harness-neutral wording.

- [ ] **Step 1: Write failing tests** asserting required record families, exact-binding fields, reason-code cases, default-deny authority, and required adversarial case IDs are present and internally coherent.
- [ ] **Step 2: Run `python3 -m unittest plugins/changeplane/tests/test_language.py -v`** and verify failure because the language artifacts do not exist.
- [ ] **Step 3: Add the minimal canonical skill, language reference, conformance corpus, manifest, and catalog entry**; keep normative declarations free of runtime enforcement code.
- [ ] **Step 4: Run the focused test and existing generator scripts**, then verify generated manifests/symlink views arise from generators only.
- [ ] **Step 5: Commit and push** with message `feat(changeplane): define normative language`.

### Task 2: Deterministic Engine and Governed Scheduling

**Files:**
- Create: `plugins/changeplane/core/changeplane/__init__.py`
- Create: `plugins/changeplane/core/changeplane/model.py`
- Create: `plugins/changeplane/core/changeplane/identity.py`
- Create: `plugins/changeplane/core/changeplane/engine.py`
- Create: `plugins/changeplane/core/changeplane/reconcile.py`
- Create: `plugins/changeplane/core/changeplane/envelope.py`
- Create: `plugins/changeplane/tests/test_engine.py`
- Create: `plugins/changeplane/tests/test_adversarial.py`

**Interfaces:**
- Consumes: conformance records and reason codes from Task 1.
- Produces: `canonical_hash(value)`, `evaluate_action(state, request)`, `evaluate_outcome(state, outcome_id)`, `schedule(state)`, `reconcile(state)`, and `compile_envelope(state, outcome_id, executor)` returning JSON-compatible deterministic values.

- [ ] **Step 1: Write failing table-driven tests** for canonical identity, stale fences, default deny, idempotency, dependencies, evidence, candidate repair invalidation, claims/capacity, receipts, checkpoint/drain/quiescence, and two equivalent host profiles.
- [ ] **Step 2: Run the two focused test modules** and verify expected import/behavior failures.
- [ ] **Step 3: Implement immutable value parsing and canonical hashing** with sorted normalized JSON and non-empty typed identities.
- [ ] **Step 4: Implement deterministic authority, evidence, dependency, action, receipt, and lifecycle enforcement** returning stable decisions and reason codes without side effects.
- [ ] **Step 5: Implement level-triggered reconciliation and conflict-aware scheduling** where stale observation denies mutation, waiting consumes no writer slot, unknown lanes do not serialize unrelated ready lanes, and overlapping claims are separated.
- [ ] **Step 6: Implement bounded harness-neutral envelope compilation** containing exact objective, generations, head, claims, authority, acceptance, budget, stop conditions, and receipt schema.
- [ ] **Step 7: Run focused and language tests**, commit, and push with message `feat(changeplane): add deterministic engine`.

### Task 3: Read-only MCP Control Room

**Files:**
- Create: `plugins/changeplane/mcp/server.py`
- Create: `plugins/changeplane/mcp/control_room.html`
- Create: `plugins/changeplane/.mcp.json`
- Create: `plugins/changeplane/tests/test_mcp.py`
- Create: `plugins/changeplane/tests/test_control_room.py`

**Interfaces:**
- Consumes: Task 2 reconciliation, scheduling, and envelope preview APIs.
- Produces: MCP `initialize`, `tools/list`, `tools/call`, `resources/list`, and `resources/read`; tools `changeplane_snapshot`, `changeplane_understand`, `changeplane_plan`, and `changeplane_control`; resource `ui://changeplane/control-room` with `text/html;profile=mcp-app`.

- [ ] **Step 1: Write failing protocol tests** that execute the real server over stdio and assert initialize/list/call/read, structured content plus text fallback, UI resource linkage, and rejection of unknown/mutating operations.
- [ ] **Step 2: Write failing UI behavior tests** for decision-language summaries, delta-first status, regular/blank/error/no-permission/reduced-motion states, drill-down, and no executable mutation controls.
- [ ] **Step 3: Run focused tests** and verify failures because server/UI are absent.
- [ ] **Step 4: Implement the stdio MCP server** using newline-delimited JSON-RPC, portable state-file reconstruction, strictly read-only tools, and action/envelope previews labeled as not executed.
- [ ] **Step 5: Implement the self-contained control room** with human summaries such as work planned/ready/running, one explicit decision card, approval consequence copy, detail disclosure, accessible semantics, light/dark styling, and reduced-motion support.
- [ ] **Step 6: Run MCP/UI tests**, commit, and push with message `feat(changeplane): add read-only control room`.

### Task 4: Evals, Demo, Distribution, and Learning Evidence

**Files:**
- Create: `plugins/changeplane/evals/adversarial.json`
- Create: `plugins/changeplane/examples/self_hosting.json`
- Create: `plugins/changeplane/examples/blocked_decision.json`
- Create: `plugins/changeplane/tests/test_vertical.py`
- Create: `plugins/changeplane/README.md`
- Create: `docs/changeplane/fast-path-learning-report.md`
- Modify generated outputs through: `scripts/build_marketplaces.py`, `scripts/build_runtime_views.py`, `scripts/build_hermes_plugins.py`

**Interfaces:**
- Consumes: all prior public APIs and MCP entrypoint.
- Produces: `python3 plugins/changeplane/mcp/server.py --demo <fixture>` runnable demo and a complete fast-path learning report.

- [ ] **Step 1: Write failing vertical tests** that load the self-hosting fixture, reconcile it, schedule two non-conflicting ready outcomes, explain an architecture decision stop, preview an envelope, reconstruct after restart, and execute every required adversarial eval.
- [ ] **Step 2: Run the vertical test** and verify expected fixture/demo failures.
- [ ] **Step 3: Add bounded fixtures, demo output, README run commands, and the learning report** covering design acceleration, disagreements, governed catches, unnecessary complexity, corrected semantics, divergent architecture, operator UX, important evals, and intentional omissions.
- [ ] **Step 4: Run all generators, `python3 -m unittest discover -s plugins/changeplane/tests -v`, `bin/check`, `git diff --check`, and real MCP/demo smokes**; inspect rendered control room in light/dark at narrow and wide widths.
- [ ] **Step 5: Commit and push** with message `test(changeplane): prove fast reference vertical` and update the draft PR body with architecture, verification, learning evidence, and deferred work.

