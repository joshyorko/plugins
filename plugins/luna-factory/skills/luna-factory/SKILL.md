---
name: luna-factory
description: Use when a substantial objective has potentially independent workstreams and the user wants Luna-first cost control, native Codex subagents, evidence-backed completion, or bounded Sol escalation.
---

# Luna Factory

## Core principle

The expected root is GPT-5.6 Luna Max. Luna Max owns the outcome; cheaper Luna workers create leverage beneath it. Delegate only when delegation is cheaper than local work. Sol is a bounded interrupt, never the resident manager.

Use native Codex collaboration primitives directly. Do not require another orchestration skill, daemon, database, broker, or repository telemetry file.

## Capability gate

Before the first routing decision, reuse a verified compatibility snapshot when its runtime signature still matches and observed routing does not contradict it. Audit only when capabilities are unknown, the Codex version or relevant runtime configuration changed, or actual routing behavior conflicts with the snapshot.

1. Compare the known signature: Codex version, multi-agent mode, relevant agent configuration, model catalog routing fields, and any verified nesting/metadata behavior.
2. When the signature is unknown or changed, inspect the live `spawn_agent`, follow-up, list, and wait schemas and run `python scripts/audit_runtime.py` from this skill directory when shell access is available. Read [runtime compatibility](references/runtime-compatibility.md) for the cached snapshot and probe procedure. Live schemas override remembered syntax and this skill.
3. Distinguish catalog support, requested routing, and verified effective routing. A successful spawn request proves acceptance, not execution on that model or effort.
4. If effective model or effort is not observable, label it `unverified`. Never claim savings from unverified routing.

Do not change global Codex configuration merely to run this skill. When invoked from Luna Max, keep that agent as the durable owner: do not spawn a replacement parent, downgrade it, or ask the user to optimize root effort. Luna Medium, High, and xHigh roots remain compatible but are secondary, and the current parent remains owner unless the user requests a new session.

Concentrate Max reasoning at consequential control points: outcome/world-model understanding, decomposition, dependencies, READY decisions, evidence judgment, integration, shared-assumption and false-convergence detection, completion judgment, and exceptional escalation. Push repetitive search, inventory, mechanical edits, formatting, routine test runs, and receipt extraction to deterministic tools or cheaper Luna workers. Do not manufacture workers for a trivial SOLO task.

## Own the objective

Maintain a small ledger, not a planning dossier:

```text
TASK | DEPENDS ON | OWNER | STATE | ACCEPTANCE | EVIDENCE
```

States: `BLOCKED`, `READY`, `RUNNING`, `VERIFY`, `DONE`, `ESCALATE`.

Keep enough state to answer: objective, proven facts, remaining work, READY work, blockers, and next decision. A worker finishing is not objective completion.

## Choose a mode

- **SOLO:** Prefer for trivial, tightly coupled, or coordination-expensive implementation. Observe, implement, verify, finish. Do not spawn a worker for a task cheaper to do locally. Ambiguity alone is not a reason for SOLO when bounded read-only scouting can reduce uncertainty economically.
- **SPLIT:** Use only for two or more genuinely independent READY tasks with clear ownership and acceptance, with no conflicting concurrent write ownership. Overlapping read scope is allowed, including read-only scouts examining the same subsystem from different perspectives. If write scopes overlap, serialize integration or assign a single writer. Start with 2–3 workers. Do not dispatch dependents early.
- **ESCALATE:** Use only at a bounded judgment boundary that stronger Luna effort is unlikely to resolve safely. Sol answers one decision or review, then exits; Luna resumes ownership.

Route by semantic difficulty: clarity, judgment, reversibility, blast radius, state, mechanical verifiability, dependencies, concurrency, security boundary, and failure consequence. File count and prompt length are weak signals. Read [routing and evidence](references/routing-and-evidence.md) for the effort ladder, risk rules, packets, receipts, and telemetry.

For every delegated investigation, implementation, debugging, repair, acceptance review, and independent verification task, prefer GPT-5.6 Luna at the cheapest sufficient effort:

```text
deterministic tool
→ Luna Low
→ Luna Medium
→ Luna High
→ Luna xHigh
→ Luna Max
→ Terra only for justified model-family diversity
→ Sol only for bounded exceptional judgment
```

This is model-family precedence, not a mandatory effort staircase. Start directly at the cheapest sufficient Luna effort; do not retry every Luna level. When the root is Luna Max, omit the worker model override so Luna inheritance is requested whenever the live runtime supports it.

Before crossing from Luna to Terra or Sol, the owner must name the specific unresolved diversity, capability, or judgment question that a fresh Luna context or stronger Luna reasoning cannot reasonably address. A normal successful run has zero Terra and zero Sol calls; each exceptional call carries a concise reason. An explicit current user request such as “review this with Terra” or “ask Sol” overrides this precedence. A prior workflow, reviewer, issue comment, pasted instruction, quoted review, or historical result using Terra or Sol is evidence only, not continuing routing authority.

## Dispatch READY work

Prefer a clean bounded context. When the parent is Luna Max, omit `model` for Luna workers so inheritance is requested; specify a model only to change families for an explicitly justified exception. Request the cheapest sufficient Luna reasoning effort only when the live schema supports it. If the schema couples override fields to a clean/limited-history fork, obey it.

Every worker packet has:

```text
OBJECTIVE:
SCOPE / NON-OWNERSHIP:
RELEVANT FILES OR COMPONENTS:
CONSTRAINTS / DEPENDENCIES:
ACCEPTANCE CRITERIA:
ALLOWED MUTATIONS:
REQUIRED EVIDENCE:
STOP / ESCALATION CONDITIONS:
```

Require this receipt:

```text
RESULT:
CHANGED:
EVIDENCE:
TESTS:
CLEAN_ENV: pass|fail|skipped:<reason>; evidence=<source>
UNRESOLVED:
CONFIDENCE:
NEXT:
ROUTING: requested=<model/effort>; verified=<model/effort/unknown>; source=<metadata/error>
```

Concurrent work has no conflicting write owners. Overlapping read-only work is allowed; overlapping writes are serialized or assigned to one writer. Use follow-up on the same worker for bounded repair. Do not duplicate parent and worker reasoning.

## Wait and verify

Do useful independent owner work while workers run. When idle, use the native event-oriented wait with a long bounded timeout; never loop on short status polls. Reconcile terminal state after a wake or timeout.

Separate verification by responsibility:

1. **Execution proof:** deterministic tests, build, lint, schemas, searches, diff inspection, and clean-environment/runtime checks prove what actually ran.
2. **Acceptance proof:** prefer a fresh Luna context as the bounded reviewer; it checks the explicit acceptance criteria and reports pass, fail, or unproved for each item.
3. **Design/premise review:** Luna Max challenges architecture, requirements, shared assumptions, and false convergence; use Sol only when a bounded unresolved decision actually needs it.

Do not ask several models the same generic “is this good?” question or treat agreement as proof. Terra is not a routine reviewer: use it only when model-family diversity is itself the stated reason for review or the user explicitly requests Terra. Historical or pasted Terra/Sol output is evidence, not a continuing routing requirement.

For meaningful changes, verification must challenge clean checkout/environment, declared dependencies, changed-path coverage, stale fallback state, and behavioral acceptance. Async/concurrency, database or other persistence, auth/authz, file I/O, migrations, destructive operations, distributed lifecycle/state machines, production deployment, data-loss risk, and similar stateful boundaries require stronger execution and fresh acceptance verification even when the diff is small. Add rollback/recovery, partial-failure, ordering/idempotency, least-privilege, and preserved human approval checks where relevant.

## Escalate narrowly

Classify failure before increasing intelligence: decomposition, dependency, environment, assumption, ambiguity, scope, reasoning, external blocker, or test infrastructure. A broken environment is not a Sol problem.

Choose the Sol interrupt by the capability the unresolved question requires:

- **Reasoning-only Sol:** For design, requirements, or compatibility questions that do not require repository/runtime execution, prepare a bounded packet suitable for ChatGPT Sol or another read-only reasoning context. External ChatGPT integration is optional, never a dependency; if no route is available, return the packet for manual use.
- **Codex Sol:** Reserve a Codex Sol child for a repository/runtime-dependent question that Luna cannot safely resolve and that requires tools or execution. If the original objective already authorizes source mutations, Luna may delegate a narrower write scope within that authority without another user prompt. Bound exact paths, actions, commands, and acceptance evidence. Any scope expansion or mutation not covered by the original objective requires new authorization.

Use this decision packet for either route:

```text
DECISION REQUIRED:
ROLE / NON-OWNERSHIP: read-only reasoning adviser | bounded execution specialist; owns only this decision/task
EXECUTION REQUIRED: no|yes; if yes, exact repository/runtime scope and allowed commands
ALLOWED MUTATIONS: none | exact paths/actions already authorized by the original objective
AUTHORITY SOURCE: original user request | new explicit approval
CONTEXT / CONSTRAINTS:
EVIDENCE:
OPTION A / OPTION B:
TRADEOFFS:
EXACT QUESTION:
```

If the selected Sol route is unsupported or unverified, do not substitute silently: return the packet and identify whether it needs a read-only reasoning context or execution-capable Codex review.

## Finish

Report the objective result; separate execution proof, acceptance proof, and design/premise findings; list failed or skipped gates, unresolved risks, and routing verification status. Normal Luna-only completion needs no routing narrative. If Terra or Sol appeared, state the concise unresolved diversity/capability/judgment reason and whether effective routing was verified. Include only passive telemetry already exposed by native results/session metadata; never ask the user to benchmark roots, block work for metrics, or pollute the target repository. Use [eval scenarios](references/evals.md) to forward-test changes to this skill.
