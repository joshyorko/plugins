# Drain and quiesce

The sequence is:

```
DRAIN -> durable CHECKPOINT -> QUIESCE
```

DRAIN stops new implementation admission. Finish, reject, or park authorized
lanes; process outstanding required evidence/events; publish current state; and
persist an exact resume pointer, generation, head, owners, blockers, and
decisions. Clean ephemeral workers/worktrees only after receipts and resume data
are durable.

Enter QUIESCE only with a durable checkpoint, zero active writers, and zero
unprocessed required events. Reconcile the checkpoint against current reality
before resume. Report SATISFIED separately from UNKNOWN, blocked, and
incomplete outcomes. Quiescence is not convergence. A budget, stale state,
external dependency, or human choice must be named, never converted to success.
