# Controller model

Desired state is approved predicates for a plan generation; observed state is a
fresh `FactorySnapshot`. Neither ticket count nor worker idleness is desired
state. `Converged(G)` requires every mandatory predicate to have valid
evidence for G. `AutonomouslyQuiescent(G)` means no safe authorized
transition can reduce known drift; it may remain unconverged.

Use exactly: `READY`, `PROGRESSING`, `WAITING`, `BLOCKED`,
`EXTERNALLY_BLOCKED`, `HUMAN_DECISION_REQUIRED`, `UNKNOWN`, and
`SATISFIED`. UNKNOWN is first-class. Blockers are local to their lane.
Capacity is a ceiling, never an objective; never dispatch filler.

One mutation owner controls each lane. Read-only research/review may fan out.
A persistent supervisor owns observation, admission, dispatch, receipts,
acceptance, terminalization, recycling, replanning, and quiescence. Worker
return, issue/PR creation, CI/review start, a suggested action, or an iteration
limit is an event, not completion.

The generic verb is **DISPATCH TO AGENT**. Executor, model, provider, effort,
and capacity are editable run configuration only. An implementation agent
cannot weaken its own acceptance, security, or architecture contract.
Independent authority is required. Full outcome terminalization requires
closing semantics plus post-merge live closure verification; otherwise record a
partial slice with non-closing `Progresses` semantics. External/human
blockers remain open with the exact blocker.
