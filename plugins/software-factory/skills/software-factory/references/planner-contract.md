# Planner contract

Accept raw intent, existing issues/PRs, an epic, a ledger, or an approved
generation requiring bounded replanning. Resolve an explicit repository;
otherwise use an unambiguous current or conversational target, or ask the
operator. First perform a small read-only observation of repository, branch,
exact head, overlapping work, architecture/tests, ownership, satisfied
outcomes, and relevant release state. Current reality wins over caches and
claims.

`FactoryTarget` is repository, targetBranch, observedHead, and optional
trackerIssue. `FactoryIntent` is goal, constraints, acceptance, and optional
appetite. `FactoryPlan` is generation, observedRepositoryHead, and outcomes.
Each outcome has id, desired predicate, acceptance/evidence, dependencies,
assumptions, existing-work reference, and proposed materialization.
`FactorySnapshot` projects work, candidates, checks, reviews, blockers,
ownership, and exact subjects. `FactoryControlIntent` carries an action,
target, optional generation/head, editable run configuration, and constraints.

Produce a bounded outcome DAG, not a task checklist. Explain each node's
property, rationale, acceptance, dependencies, assumptions, and ownership.
Classify each as `REUSE_EXISTING`, `REFINE_EXISTING`, `CREATE_NEW`,
`ALREADY_SATISFIED`, `INVESTIGATE_UNKNOWN`, or `OUT_OF_SCOPE`. Search
before creating and never silently redefine the goal.

Planning, observation, and export are read-only. Materialization requires
explicit approval bound to repository, branch, exact observed head, generation,
graph/materialization proposal, constraints, and appetite. Changed fences
require re-observation and an explainable delta; G approval never approves G+1.
Keep discovery findings in an inbox as candidate objectives. Show bounded
nodes, writer/repair/replan budgets, no-filler policy, explainability receipts,
and portable JSON/YAML/Markdown export. Budget hits checkpoint for a human
decision; they never fake success.
