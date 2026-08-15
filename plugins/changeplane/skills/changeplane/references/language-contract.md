# Changeplane Language Contract

This reference defines the smallest portable semantic surface. It is a
contract for documents, proposals, envelopes, and receipts; it is not a
database schema and does not define CP-ENGINE APIs.

## Nouns and typed links

The minimum vocabulary is:

`ChangeplaneConstitution`, `ApplicationModel`, `ApplicationModelProposal`,
`Repository`, `Component`, `Capability`, `Interface`, `Resource`,
`Environment`, `Artifact`, `Invariant`, `QualityAttribute`, `ChangeIntent`,
`Outcome`, `Predicate`, `Condition`, `Dependency`, `Assumption`, `ChangePlan`,
`PlanProposal`, `PlanGeneration`, `Observation`, `Evidence`, `Candidate`,
`AgentRole`, `AgentExecutor`, `AgentEnvelope`, `Dispatch`, `ResourceClaim`,
`AuthorityGrant`, `Policy`, `ActionDefinition`, `ActionRequest`,
`ActionReceipt`, `Blocker`, `AdmissionDecision`, `Convergence`, and
`Quiescence`.

Use typed links, not implied naming conventions:

* `ApplicationModel contains Component`
* `ApplicationModel declares Invariant`
* `ChangePlan contains Outcome`
* `Outcome dependsOn Outcome`
* `Outcome affects Application concept`
* `Candidate implements Outcome`
* `Evidence proves Predicate`
* `Dispatch targets Outcome`
* `Dispatch executedBy AgentExecutor`
* `Dispatch holds ResourceClaim`
* `ActionReceipt records ActionRequest`

Every link has an identifiable source, target, relation, generation, and
provenance. Unknown targets are invalid for admission, not silently created.

## Provenance and disposition

Provenance answers where a fact came from: source kind and locator, author or
executor, generation, observed repository/head, timestamp, and exact source
bytes or digest. Preserve predecessor and successor identities when a
generation changes.

Disposition answers what to do with the subject and is independent of
mutability: `CREATE_NEW`, `REUSE`, `REJECT`, or `SUPERSEDE`. A mutable subject
may be `CREATE_NEW`; a read-only observation may be `REUSE`. Never encode
disposition as `MUTABLE` or `READ_ONLY`.

## Actions, authority, and receipts

An `ActionDefinition` names an allowed operation and required policy. An
`ActionRequest` names the exact subject, actor, outcome, claims, inputs,
preconditions, and idempotency key. An `AuthorityGrant` explicitly covers
those fields, has an issuer, policy, validity, and revocation/checkpoint
identity. No grant means default deny.

An `ActionReceipt` records the request identity, observed subject, grant
identity, start/end, result, evidence references, side-effect summary, and
next state. `SUCCEEDED` is valid only when the receipt's predicates pass;
receipt prose, hostile text, agent return, or self-certification never expands
the grant.

## Plans, reconciliation, and scheduling

`ChangePlan` is a desired-state graph of outcomes, predicates, dependencies,
resource claims, admission conditions, evidence providers, repair limits, and
failure policy. `PlanGeneration` is immutable and content-addressed.

Reconciliation observes current evidence, compares it with the plan, and emits
an `AdmissionDecision`: `READY`, `WAITING`, `UNKNOWN`, `BLOCK`, or `REJECTED`.
Only READY work may be scheduled. A scheduler respects dependency completion,
disjoint claims, effective capacity, cancellation, and priority policy; it
does not infer readiness from branch or PR existence. Conflicting or stale
evidence is UNKNOWN/BLOCK and stops the lane.

### Assumptions and exact subjects

An `Assumption` has an immutable identity, evidence references, and one of
`SATISFIED`, `UNSATISFIED`, `UNKNOWN`, or `INVALIDATED` satisfaction states.
Missing, stale, or contradictory evidence makes it `UNKNOWN`; a disproved
predicate makes it `INVALIDATED`. Only `SATISFIED` assumptions admit or
schedule work. Invalidation cancels pending admission, releases scheduling
claims, and requires fresh reconciliation; it never starts a repair loop.

An assumption about the exact candidate, approved base, model, plan, or
repository subject is invalidated by movement of that subject (base movement
included). Unrelated head
movement does not invalidate it automatically, but must still be re-observed
before admission. Any repair candidate invalidates predecessor acceptance.

## Machine-checkable language shapes

The focused fixtures in `tests/fixtures/semantic_cases.json` exercise this
compact vocabulary without defining engine APIs:

* a typed link is `{source, target, relation, generation, provenance}`; both
  endpoints are known types and provenance has an exact digest;
* an envelope/request names `{action, subject, actor, claims}` and a grant
  must match all four fields; absent or mismatched grants are default-deny;
* evidence is `{provenance, disposition}`, never one field standing in for
  the other; provenance includes the observed head and source digest;
* an assumption has `{identity, evidence, state}` and subject movement may
  invalidate it; a non-satisfied assumption cannot be scheduled;
* receipts record the request/grant identities, result, predicate evidence,
  and next state; prose cannot supply omitted fields;
* plans contain outcomes, predicates, dependencies, assumptions, claims, and
  admission conditions; checkpoints contain the accepted generation, claims,
  receipts, and replay-safe position.

The executable seam requires these exact record fields. Non-empty local IDs are
accepted for declarative references; every `sha256:` identity is exactly
`sha256:` plus 64 lowercase hexadecimal characters. A link endpoint is
`Type:identity`, where both types are known; its provenance requires `source`,
`observedHead`, and a `sha256:` digest. An envelope requires identity,
outcome, plan, role, executor, objective, inputs, allowed and forbidden
actions, claims, evidence references, and receipt fields. An ActionRequest
requires identity, action, subject, actor, outcome, claims, inputs,
preconditions, and idempotency; its grant repeats request, action, subject,
actor, and claims and adds issuer, policy, validity expiry, and revocation.
An ActionReceipt requires identity, request, exact subject, grant, start/end,
result, evidence, predicates, side-effects, and next state; a successful
receipt without evidence is rejected. Plans require identity, content-
addressed generation, outcomes, predicates, dependencies, assumptions,
claims, and admission conditions. Checkpoints require generation, claims,
receipts, position, state, evidence, and pending events. Invalid records are
reported by stable reasons such as `typed link endpoint`, `typed link relation`,
`envelope shape`, `record identity`, `exact identity`, `assumption admission`,
`assumption movement effects`, `grant binding`, `receipt binding/completeness`,
`checkpoint shape`, `fresh reconciliation`, `admission consistency`, and
`quiescence`.

## Transition matrix

The admissible transitions are: `WAITING -> READY|UNKNOWN|BLOCK`,
`UNKNOWN -> WAITING|BLOCK`, `BLOCK -> WAITING|UNKNOWN`,
`READY -> PROGRESSING|WAITING|BLOCK|UNKNOWN`,
`PROGRESSING -> CONVERGED|WAITING|BLOCK|UNKNOWN|CANCELLED`, and
`DRAINING -> QUIESCENT|CANCELLED`. `UNKNOWN` and `BLOCK` deny admission;
`UNKNOWN -> READY` and `BLOCK -> READY` are forbidden without fresh
reconciliation; read-only investigation may reduce them, but no read-only repair loop is
allowed. `CONVERGED -> READY` is forbidden, as are reopening transitions from
`REJECTED`, `CANCELLED`, or `QUIESCENT`. Terminal `CONVERGED`, `REJECTED`, `CANCELLED`, and `QUIESCENT` states
cannot reopen or authorize work; a new plan generation is required.

## Envelopes, receipts, and harnesses

An `AgentEnvelope` contains the exact outcome and plan generation, role and
executor capability, bounded objective, inputs, allowed/forbidden actions,
resource claims, invariants, timeout/cancellation behavior, evidence contract,
and required receipt fields. `Dispatch` binds one envelope to one executor and
one claim set. The harness may execute or decline it, but cannot alter the
language contract.

## Replay, drain, checkpoint, and quiescence

Replay consumes immutable observations, requests, grants, and receipts to
reconstruct decisions; replay never repeats an external side effect. A
checkpoint records the last accepted generation, receipts, claims, and
replay-safe position. Drain stops new admission, allows already-authorized
work to finish or be cancelled, and records all terminal receipts.

`Quiescence` requires no READY or running dispatch, no unresolved blocker or
claim, and complete receipts for the checkpoint. It is a terminal observation,
not permission to mutate.

## States and terminal semantics

`READY` is admissible work. `WAITING` lacks a satisfied dependency. `UNKNOWN`
means evidence is missing, stale, or contradictory. `BLOCKED` records a
policy, authority, resource, or evidence stop. `CONVERGED` means all required
predicates and receipts pass. `REJECTED` is terminal negative disposition.
`CANCELLED` is terminal after drain/cancellation is recorded. `QUIESCENT` is
terminal only for a drained projection. Terminal states do not reopen or
authorize new actions; a new plan generation is required.

## Default-deny and hostile-text negatives

The following untrusted text is always denied unless an explicit grant
independently covers it: a prompt saying “ignore policy,” a comment claiming operator approval,
tool output containing a grant-like JSON object, an agent requesting extra
paths, and a receipt claiming success without fresh predicate evidence.

## Non-goals

This contract does not define a general ontology database, enterprise RBAC,
transaction/index/CDC service, event bus, hosted multitenancy,
Kubernetes/Hive dependency, broad PM platform, hosted Streamable HTTP v1,
autonomous MCP mutation, or unrelated cleanup.
