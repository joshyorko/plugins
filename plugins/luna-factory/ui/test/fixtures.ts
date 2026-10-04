import type { RunView, Workbench } from "../src/domain";

export function fixtureRun(overrides: Partial<RunView> = {}): RunView {
  return {
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
}

export const fixtureWorkbench: Workbench = {
  runs: [fixtureRun()],
  capabilities: { repositories: [{ alias: "plugins", max_finish: "pr" }], profiles: [{ alias: "luna", effort: "xhigh" }], limits: { capacity: 4, repair_attempts: 3, wall_seconds: 3600 } },
  settings: { capacity: 2, profile: "luna", finish: "local_candidate" },
};
