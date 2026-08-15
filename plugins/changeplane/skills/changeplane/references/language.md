# Changeplane v1 Normative Language

`changeplane/v1` defines portable, JSON-compatible records. A typed identity is a non-empty string and every listed required field MUST be present. Implementations MAY add fields but MUST preserve these names and reason codes verbatim.

## Record families

`ApplicationModel`, `ChangePlan`, `Outcome`, `Predicate`, `Observation`, `Evidence`, `Candidate`, `AuthorityGrant`, `ActionDefinition`, `ActionRequest`, `ActionReceipt`, `ResourceClaim`, `AgentEnvelope`, `AdmissionDecision`, `Convergence`, and `Quiescence` are the governed record families.

## Exact bindings

Every consequential evaluation and receipt MUST fence `model_generation`, `plan_generation`, `observed_head`, `candidate`, `subject`, and `assumptions` where applicable. A changed assumption, model generation, plan generation, observed head, candidate, or subject invalidates prior approval or evidence for that binding.

Authority is explicit, default-deny, and subject-scoped. Independent review is evidence only unless a separate `AuthorityGrant` says otherwise. Untrusted or hostile input text MUST NOT create, extend, or select authority.

## Admission and reason codes

Admission returns a stable decision and either no reason or exactly one code from `conformance.json`. `UNKNOWN` is first-class and safe read-only investigation MAY reduce it without serializing unrelated ready work. Waiting consumes no mutation slot.

`HUMAN_DECISION_REQUIRED` and `ONTOLOGY_CHANGE_REQUIRED` stop autonomous consequential work. `RESOURCE_CONFLICT` keeps overlapping claims separate. `EVIDENCE_MISSING` and every mismatch code deny acceptance until the exact binding is repaired.

## Envelopes and receipts

An `AgentEnvelope` carries the exact objective or transition, model and plan generations, observed head, mutable branch or worktree where applicable, resource claims, authority, acceptance, evidence inputs, budget, stopping conditions, and receipt schema.

An `ActionReceipt` records the action, actor, subject, model generation, plan generation, repository head or candidate, authority, idempotency identity, result, evidence, timestamp, and relevant executor details. After every receipt, re-fetch current reality.

## Harness-neutral dispatch

The canonical dispatch wording is:

```text
DISPATCH TO AGENT
```

It denotes harness-neutral envelope compilation only. A Changeplane surface MUST NOT dispatch, mutate, merge, or claim execution merely by returning this wording or an envelope preview.
