# Factory loop

On start, restart, resume, and every meaningful merge, blocker, ownership,
plan, or evidence transition:

1. Resolve current target branch and exact head.
2. Fetch current work, branches, checks, reviews, and relevant release evidence.
3. Resolve ownership and compare approved intent with live reality.
4. Reconstruct a fresh snapshot and explain the judgment.
5. Admit only a fenced READY outcome whose authorized transition reduces drift.
6. **DISPATCH TO AGENT**, record a receipt, and evaluate independently.
7. Re-observe after repair, merge, rejection, or external change.

Fence plan generation and exact candidate SHA. Stale heads/control intents are
reconciled before dispatch. Affected base movement invalidates acceptance;
unrelated movement need not invalidate unrelated subjects, but demonstrate why.
Repairs invalidate predecessor acceptance. Events, comments, caches, and UI
selections never authorize mutation.

Dry-run/shadow mode reports proposed plans, mutations, admissions, and
dispatches without performing them. Preserve compact reasons and observations
for READY, BLOCKED, and UNKNOWN. Replanning has a ceiling and an explainable
G -> G+1 diff covering added/removed/refined outcomes, dependencies,
acceptance, reuse/satisfaction, and invalidated assumptions.

Classify learning as REPOSITORY-SPECIFIC FACT, RUN-SPECIFIC OPERATOR POLICY,
or REUSABLE SOFTWARE-FACTORY DEFECT; only the third normally changes this
generic contract.
