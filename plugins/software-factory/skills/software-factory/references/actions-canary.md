# Actions canary

This is read-only and not durable authority. At execution time re-fetch the
repository, target branch, exact head, issues #82, #83, #84, #90, #101, all
current PRs, checks, reviews, branches, and tracker state. Do not rely on the
issue snapshot or let a live Actions SHA become authoritative. Record time and
exact subjects.

For existing work, reconstruct the factory, detect stale tracker claims,
classify overlap, distinguish green CI from complete acceptance, distinguish
affected from unrelated base movement, and keep blockers lane-local. Drain or
honestly park existing work before new admission. Generate read-only PLAN,
RECONCILE, STATUS, DRAIN, CHECKPOINT, and QUIESCE prompts with zero issue, PR,
branch, comment, label, merge, or dispatch mutation.

For raw intent, re-observe the same surface, deduplicate against issues/PRs/code,
and produce a bounded DAG with reuse/refine/create/satisfied/investigate/
out-of-scope classifications, acceptance, dependencies, assumptions, appetite,
and proposed materialization. Require an approval fence; this canary performs
none.

Every prompt states fresh re-observation, zero mutation, exact generation/SHA
fencing, independent exact-subject acceptance, no stale snapshot authority, no
filler, and convergence versus autonomous quiescence. Re-fetch before every
decision. Changed head or affected base requires reconciliation and renewed
approval; unrelated movement requires justification. Report findings, unknowns,
blockers, graph, and control intents without claiming UI/prompt execution.
