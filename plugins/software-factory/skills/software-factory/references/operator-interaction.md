# Operator interaction

Native structured prompts, optional UI actions, and ordinary text fallback all
produce the same `FactoryControlIntent`: PLAN, APPROVE_PLAN, MATERIALIZE,
START, RESUME, RECONCILE, STATUS, DRAIN, CHECKPOINT, or QUIESCE; target;
optional generation and exact observed head; editable orchestration preset; and
constraints.

Capability-detect structured questions, approvals, and input requests; never
branch on product names. Without them, ask the equivalent plain-text question.
An optional MCP App v1 control room is read-only and reconstructable: it may
inspect snapshots, plans, generation diffs, and generate prompts, but a prompt
is not execution and UI state is not authority or a hidden database. Refresh
reconstructs explicit durable state, and losing the UI does not lose the
workflow.

Before consequential approval show repository/branch/head, generation,
graph/materialization proposal, constraints/appetite, stale delta, and
independent acceptance. Keep inline/text operation for all control actions.
