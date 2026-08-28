# Routing and evidence

Read this reference when choosing worker effort, constructing packets, verifying changes, handling high-risk work, or recording an experiment run.

## Luna Max control plane

The canonical root is Luna Max. Keep its effort fixed and spend its reasoning on consequential control-plane judgment: the outcome model, dependency graph, READY decisions, evidence trust, integration, hidden assumptions, false convergence, final acceptance, and escalation boundaries.

The table below routes delegated workers and independent verification. It does not recommend changing the root. Max-root efficiency comes from cheap execution underneath it, not from making the user tune the owner every invocation.

## Cheapest sufficient worker effort

| Effort | Use when | Avoid spending it on |
|---|---|---|
| Low | Deterministic search, inventory, extraction, formatting, repetitive edits, known commands, clear fact checks | Open-ended design or debugging |
| Medium | Coordination, READY-state maintenance, repository exploration, ordinary research, test triage, clear implementation, receipt synthesis | Hard local reasoning already shown to need depth |
| High | Substantive or multi-file implementation with known design, nontrivial debugging, integration, worker review | Polling and clerical status work |
| xHigh | High was incomplete; several components or plausible causes require deeper synthesis; consequential verification needs more adversarial depth | Automatic retry steps |
| Max | Hard bounded root cause, complicated implementation with clear requirements, contradictory evidence, difficult local reasoning, high-value independent verification when another independent Max context is worth its cost | Durable coordination, waiting, copying receipts, obvious dispatch |

Reasoning follows the decision shape, not the role title. Start directly at the sufficient level; never force `low -> medium -> high -> xhigh -> max`.

## Model-family precedence

Apply this order to delegated investigation, implementation, debugging, repair, acceptance review, and independent verification:

```text
deterministic tool
→ Luna Low
→ Luna Medium
→ Luna High
→ Luna xHigh
→ Luna Max
→ Terra for justified model-family diversity only
→ Sol for bounded exceptional judgment only
```

This is a preference order, not a retry staircase. Start at the cheapest sufficient Luna effort. From a Luna Max root, omit the worker model override when the runtime supports inheritance. Before using Terra or Sol, record the exact unresolved diversity, capability, or judgment question that a fresh or stronger Luna context cannot reasonably answer. Normal successful runs use neither family.

A current explicit user request for Terra or Sol overrides the preference. A prior workflow, reviewer, issue comment, pasted instruction, quoted output, or historical result does not: treat it as evidence, not routing authority.

## Split test

Delegate only when all are true:

- the task is `READY`;
- scope and non-ownership are explicit;
- acceptance is independently checkable;
- there is no conflicting concurrent write ownership; overlapping reads are allowed, and overlapping write scopes are serialized or assigned to one writer;
- parallel time/context savings exceed handoff, integration, and verification cost.

Otherwise keep it local. A 100-file mechanical transform may be Low; a five-line authorization change may require Max or Sol judgment.

## Verification contracts

Worker testimony is evidence, not final proof. Keep three responsibilities distinct:

| Responsibility | Question | Evidence owner |
|---|---|---|
| Execution proof | Did the intended code and checks actually run in a representative clean environment? | Deterministic tools and runtime evidence |
| Acceptance proof | Does each explicit acceptance criterion pass? | Fresh bounded Luna checklist reviewer |
| Design/premise review | Are the requirements, architecture, and shared assumptions sound? | Luna Max; bounded Sol interrupt only when unresolved |

For a meaningful change, inspect the diff and rerun the narrow checks using clean context and declared dependencies. When hidden local state could affect the claim, run from a clean checkout/environment with isolated runtime state; if that environment is unavailable, record `CLEAN_ENV: skipped:<reason>` and leave execution or acceptance unproved. Model agreement cannot replace any of these evidence owners.

Challenge the premise, not only the patch:

- Would this pass from a clean checkout?
- Is an undeclared dependency or environment variable helping?
- Did the checks exercise the changed path?
- Is fallback behavior reading stale state?
- Does the test prove desired behavior or only its own expectation?
- Do implementer and verifier share the same untested assumption?

Treat async/concurrency, database or other persistence, auth/authz, file I/O, migrations, destructive operations, distributed lifecycle/state machines, production deployment, data-loss risk, and similar stateful boundaries as high risk regardless of diff size. Add ordering, atomicity/idempotency, recovery/rollback, consequences of partial failure, least-privilege checks, and the exact human approval required before live mutation. Multiple AI approvals are not proof of safety.

## Escalation predicates and destinations

Sol is justified for a bounded architectural choice, unresolved requirement ambiguity, contradictory independent evidence, public compatibility/API decision, auth or permission boundary, concurrency semantics, distributed lifecycle/state-machine correctness, irreversible migration/data-loss risk, or a reasoning failure that Luna Max has narrowed.

- Route a question that needs reasoning but no repository/runtime execution as a bounded read-only packet for ChatGPT Sol or another available reasoning context. Do not require an external integration.
- Route a question that requires repository/runtime tools or execution to Codex Sol only when Luna cannot safely resolve it. Codex Sol may inherit a strictly narrower write scope already authorized by the original objective; bind exact paths/actions and retain one writer. New files/actions outside that authority, destructive or live-state changes, and external mutations require authorization. Fail visibly if the requested route cannot be verified.

Terra is not a routine reviewer. Use it only when model-family diversity is the explicit review objective or the user directly requests Terra, with its own evidence contract. A historical Terra review is context, not a routing requirement. Sol similarly requires a bounded exceptional judgment/capability boundary or a current explicit user request. Never make Terra or Sol a mandatory ritual or duplicate a generic approval question across models.

## Passive telemetry

Reuse only fields native results/session metadata already expose. Collection must never cause extra polling, worker calls, user bookkeeping, repeated root-effort benchmarks, or repository files. If a field is not already observable, use `unknown` and continue.

```text
task_class:
root_model_effort_requested:
root_model_effort_verified:
root_usage_or_elapsed_observed:
workers: [{task, requested, verified, evidence_source, usage_or_elapsed_observed}]
worker_count:
retry_count:
elapsed_time_observed:
escalation_reason:
review_findings:
failed_acceptance_checks:
remaining_defects:
sol_calls_avoided:
sol_calls_required:
sol_usage_or_elapsed_observed:
comparison_baseline: unknown unless an automated evaluation already supplied one
```

Use `unknown` rather than fabricated token, subscription, elapsed, model, or effort values. Later automated evaluation may group comparable task classes and compare total worker + orchestration + retry + verification + integration + escalation usage against a simpler Sol baseline. Ordinary users do not run that comparison manually.
