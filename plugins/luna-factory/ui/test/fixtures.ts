import type { RunView, Workbench } from "../src/domain";

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
      { id: "A2", description: run.acceptance[1] ?? "Second criterion", status: "unproved", reason: "Owner acceptance is pending", check_refs: [] },
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
      criteria: { proven: 1, failed: 0, unproved: 1, mandatory: 2 },
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
