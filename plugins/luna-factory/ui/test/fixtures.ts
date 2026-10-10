import type { BackendCatalog, GraphEnvelope, RunView, Workbench } from "../src/domain";

export function fixtureRun(overrides: Partial<RunView> = {}): RunView {
  const run: RunView = {
    id: "run-123", repository: "plugins", objective: "Make Luna Factory a first-class workbench",
    acceptance: ["Single-file build passes", "Run controls preserve identity"], non_goals: ["No merge or deployment"],
    finish: "pr", profile: "luna", capacity: 3, active_workers: 2, state: "VERIFYING",
    current_subject: "f51105bf08e37a19a91c8f174a938be9d12ea059:working-tree",
    owner_thread: "owner-123", turn_id: "turn-123", delta: "The workbench build passes. Checking same-run recovery and final evidence.",
    remaining_gap: "Owner acceptance of the current subject is still required.", blocker: null, deadline_at: 1791160800,
    claim_held: true,
    route: { requested_model: "gpt-6-luna", requested_effort: "xhigh", configured_model: "gpt-6-luna", configured_effort: "xhigh", observed_model: null, observed_effort: null },
    receipts: [{ kind: "execution", summary: "Single-file build and typecheck passed for the current subject.", subject: "f51105bf08e37a19a91c8f174a938be9d12ea059:working-tree", created_at: 1791140400 }],
    ...overrides,
  };
  if (!run.control) run.control = {
    schema_version: 1, revision: 7, intent_generation: 2, dispatch_generation: 3,
    criteria: [
      { id: "A1", description: run.acceptance[0] ?? "First criterion", status: "proven", reason: null, check_refs: ["check-1"] },
      { id: "A2", description: run.acceptance[1] ?? "Second criterion", status: run.state === "CONVERGED" ? "proven" : "unproved", reason: run.state === "CONVERGED" ? null : "Owner acceptance is pending", check_refs: run.state === "CONVERGED" ? ["check-2"] : [] },
    ],
    tasks: [{ id: "task-owner", title: "Complete the objective", criterion_ids: ["A1", "A2"], dependencies: [], state: "VERIFY", admission: "admitted", reason: null, owner_thread: run.owner_thread, attempt_ids: ["attempt-1"] }],
    attempts: [{ id: "attempt-1", task_id: "task-owner", intent_generation: 2, dispatch_generation: 3, subject: run.current_subject, thread_id: run.owner_thread, turn_id: run.turn_id, status: "returned" }],
    effects: [], child_policy: "cooperative_unverified",
  };
  if (!run.presentation) {
    const kind = run.state === "NEEDS_INPUT" && run.pending_decision ? "answer" : ["RUNNING", "VERIFYING"].includes(run.state) && run.turn_id ? "steer" : ["BLOCKED", "QUIESCENT", "INTERRUPTED", "FAILED", "CANCELLED"].includes(run.state) ? "resume" : ["CONVERGED", "CANCELLED"].includes(run.state) ? "inspect" : "wait";
    const tool = kind === "answer" ? "resume_factory_run" : kind === "steer" ? "steer_factory_run" : kind === "resume" ? "resume_factory_run" : null;
    const reason = kind === "answer" ? "A decision is waiting" : kind === "steer" ? "The owner turn is active" : kind === "resume" ? "The run is stopped" : "Wait for a new observed result";
    const primary = { kind, label: kind === "answer" ? "Answer the owner" : kind === "steer" ? "Steer the owner" : kind === "resume" ? "Resume same run" : kind === "inspect" ? "Inspect this outcome" : "Wait for an observed result", reason, tool, allowed: tool !== null } as const;
    const resultKind = run.state === "CONVERGED" ? "finished_verified" : ["CANCELLED", "INTERRUPTED", "FAILED", "QUIESCENT"].includes(run.state) ? "stopped_unresolved" : run.state === "NEEDS_INPUT" ? "needs_input" : ["RUNNING", "VERIFYING", "STARTING", "CANCELLING"].includes(run.state) ? "working" : "unverified";
    const resultLabel = resultKind === "finished_verified" ? "Finished and verified" : resultKind === "stopped_unresolved" ? "Stopped with work unresolved" : resultKind === "needs_input" ? "Waiting for your decision" : resultKind === "working" ? "Work in progress" : "Outcome unverified";
    run.presentation = {
      revision: run.control.revision,
      primary_action: primary,
      actions: [...(tool ? [primary] : []), ...(run.claim_held && !["CONVERGED", "CANCELLED"].includes(run.state) ? [{ kind: "cancel" as const, label: "Stop run", reason: "Stop owned execution", tool: "cancel_factory_run", allowed: true }] : [])],
      criteria: run.state === "CONVERGED" ? { proven: 2, failed: 0, unproved: 0, mandatory: 2 } : { proven: 1, failed: 0, unproved: 1, mandatory: 2 },
      result: { kind: resultKind, label: resultLabel },
      owner: { thread_id: run.owner_thread, turn_id: run.turn_id, liveness: run.state === "RUNNING" ? "active" : run.owner_thread ? "unknown" : "idle" },
      workers: run.active_workers ? [{ thread_id: "worker-1", liveness: "unknown" }] : [],
      budget: { time_remaining_seconds: 120, repair_attempts_remaining: 2, repairs_used: 1 },
      claim: { held: run.claim_held, status: run.claim_held ? "owned" : "released" },
      deliverable: { kind: run.finish === "pr" ? "pr_ready" : run.finish === "push" ? "push" : "local_candidate", status: run.state === "CONVERGED" ? "verified" : "unproved", subject: run.current_subject, reference: null },
    };
  }
  return run;
}

export const fixtureWorkbench: Workbench = {
  runs: [fixtureRun()],
  capabilities: { repositories: [{ alias: "plugins", max_finish: "pr" }], profiles: [{ alias: "luna", effort: "xhigh" }], limits: { capacity: 4, repair_attempts: 3, wall_seconds: 3600 } },
  settings: { capacity: 2, profile: "luna", finish: "local_candidate" },
};

/** Disposable reproduction of the reported planning state, never the live canary. */
export function fixturePlanningRun(): RunView {
  const run = fixtureRun({ id: "dogfood-plan", repository: "canary", objective: "Persist a disposable planning graph", planning_only: true, state: "QUIESCENT", pending_decision: null, owner_thread: null, turn_id: null, active_workers: 0, claim_held: false, deadline_at: 1, receipts: [], blocker: null, remaining_gap: "Evidence has not been collected", finish: "local_candidate" });
  if (!run.control || !run.presentation) throw new Error("Missing fixture projection");
  run.control.revision = 6;
  run.control.criteria = [{ id: "A1", description: run.objective, status: "unproved", reason: "evidence_missing", check_refs: [] }];
  run.control.tasks = [{ id: "objective", title: run.objective, criterion_ids: ["A1"], dependencies: [], state: "candidate", admission: "candidate", reason: "planning_candidate_no_authority", owner_thread: null, attempt_ids: [] }];
  run.control.attempts = [];
  run.presentation.revision = 6;
  run.presentation.primary_action = { kind: "refresh", label: "Refresh planning graph", reason: "planning_only", tool: "refresh_factory", allowed: true };
  run.presentation.actions = [run.presentation.primary_action];
  run.presentation.result = { kind: "unverified", label: "Planning only; execution has not been authorized" };
  run.presentation.criteria = { proven: 0, failed: 0, unproved: 1, mandatory: 1 };
  run.presentation.owner = { thread_id: null, turn_id: null, liveness: "unknown" };
  run.presentation.workers = [];
  run.presentation.budget.time_remaining_seconds = 0;
  run.presentation.claim = { held: false, status: "released" };
  return run;
}

export function fixtureGraph(revision = 7): GraphEnvelope {
  const run = fixtureRun();
  return { graph: { run_id: run.id, revision, repository: { alias: run.repository, identity: "opaque-repository", base_head: "abc", subject: run.current_subject }, planning_only: true, nodes: run.control!.tasks.map(task => ({ ...task, state: "CANDIDATE", admission: "candidate", owner_thread: null, attempt_ids: [], source: null, target_preference: null })), criteria: run.control!.criteria, attempts: [], claim: { held: false, status: "released" }, changes: [] }, proposal: null };
}
const operation = { advertised: false, enabled: false, qualified: false };
export const fixtureBackends: BackendCatalog = {
  schema_version: 1, discovery: "configuration_only", policy: "subscription_only",
  targets: ["native-local", "codex-cloud"].map(id => ({ id, label: id === "native-local" ? "Native local" : "Codex Cloud", kind: id, namespace: id, operator_enabled: id === "native-local", planning_eligible: id === "native-local", execution_eligible: false, qualification: "unverified", authentication: "unknown", entitlement: "unknown", reason: "Execution qualification has not been established", operations: { discover: operation, start: operation, observe: operation, steer: operation, stop: operation, reconcile: operation }, limits: [] })),
};
