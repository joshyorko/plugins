// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { WorkbenchController } from "../src/controller";
import { renderWorkbench, type Editor, type WorkbenchOptions } from "../src/view";
import { fixtureCampaign, fixtureCampaignPlan, fixtureGraph, fixtureRun, fixtureSwarmRun, fixtureTimelines, fixtureWorkbench } from "./fixtures";

const bannedCopy = [
  /\bcampaigns?\b/i,
  /\bwaves?\b/i,
  /\breceipts?\b/i,
  /\b(?:criterion|criteria)\b/i,
  /\bnative\b/i,
  /\bowner\b/i,
  /\bsteer\b/i,
  /\badmission\b/i,
  /\bcandidates?\b/i,
  /\brevision\b/i,
  /\bsubject\b/i,
  /\bliveness\b/i,
  /\binference\b/i,
  /action boundary|safe action|claim held|supplied,? not verified|plan navigation/i,
];

function renderFixture({ runs, selectedId = null, campaigns = [], editor = null, surface = "global", graph = null, viewMode = "map", proposal = false, disconnected = false, error = false }: {
  runs: ReturnType<typeof fixtureRun>[];
  selectedId?: string | null;
  campaigns?: ReturnType<typeof fixtureCampaign>[];
  editor?: Editor;
  surface?: NonNullable<WorkbenchOptions["surface"]>;
  graph?: ReturnType<typeof fixtureGraph>["graph"] | null;
  viewMode?: "map" | "lanes" | "details";
  proposal?: boolean;
  disconnected?: boolean;
  error?: boolean;
}): HTMLElement {
  const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs, selected_run: runs.find(run => run.id === selectedId) ?? null } });
  controller.state.selectedId = selectedId;
  controller.state.campaigns = campaigns;
  controller.state.graph = graph;
  controller.state.viewMode = viewMode;
  controller.state.connected = !disconnected;
  controller.state.connectionStatus = disconnected ? "disconnected" : "connected";
  controller.state.initialized = !error;
  controller.state.error = error ? "Synthetic host error: current state could not be loaded." : null;
  if (graph && proposal) {
    controller.state.proposal = {
      id: "proposal-1", base_revision: graph.revision - 1, idempotency_key: "key", fingerprint: "fingerprint",
      actor: "local_operator", subject: "subject", status: "proposed", applied_revision: null,
      change: { kind: "set_dependencies", node_id: graph.nodes[2]?.id ?? "task-2", dependencies: [] },
    };
  }
  if (viewMode === "lanes" && selectedId) {
    controller.state.timelineRunId = selectedId;
    controller.state.timelines = fixtureTimelines(selectedId);
  }
  const root = document.createElement("div");
  renderWorkbench(root, controller.state, editor, true, { surface });
  return root;
}

function exposedCopy(root: HTMLElement): string {
  const copy = root.cloneNode(true) as HTMLElement;
  copy.querySelectorAll("[hidden], details:not([open])").forEach(element => element.remove());
  const labels = Array.from(copy.querySelectorAll<HTMLElement>("[aria-label], [title], [placeholder]"))
    .flatMap(element => [element.getAttribute("aria-label"), element.getAttribute("title"), element.getAttribute("placeholder")])
    .filter((value): value is string => value !== null);
  return [copy.textContent ?? "", ...labels].join(" ");
}

describe("plain UI vocabulary", () => {
  it("keeps banned internal terms out of rendered fixture copy", () => {
    const plan = fixtureCampaignPlan();
    const blocked = fixtureRun({ id: "blocked-1", state: "BLOCKED", blocker: "Native owner liveness is unknown. Keep the claim held." });
    const checkRunningWork = { kind: "reconcile" as const, label: "Check running work", reason: "read_only_native_reconciliation", tool: "reconcile_factory_run", allowed: true };
    blocked.presentation!.blocker_kind = { kind: "liveness_unknown" };
    blocked.presentation!.primary_action = checkRunningWork;
    blocked.presentation!.actions = [checkRunningWork, { kind: "cancel", label: "Stop run", reason: "Stop owned execution", tool: "cancel_factory_run", allowed: true }];
    const swarm = fixtureSwarmRun();
    const needs = fixtureRun({ id: "needs-1", state: "NEEDS_INPUT", pending_decision: { id: "needs-decision", question: "Which approved scope should Luna use?" } });
    const done = fixtureRun({ id: "done-1", state: "CONVERGED", active_workers: 0, claim_held: false, blocker: null, remaining_gap: null, delta: "The requested pull request is ready." });
    const views = [
      renderFixture({ runs: [needs, swarm, plan.run, done, blocked], campaigns: [fixtureCampaign()] }),
      renderFixture({ runs: [plan.run], selectedId: plan.run.id, graph: plan.graph.graph }),
      renderFixture({ runs: [blocked], selectedId: blocked.id }),
      renderFixture({ runs: [swarm], selectedId: swarm.id, graph: fixtureGraph().graph, viewMode: "lanes" }),
      renderFixture({ runs: [plan.run], editor: "start" }),
      renderFixture({ runs: [plan.run], editor: "settings" }),
      renderFixture({ runs: [plan.run], editor: "repositories" }),
      renderFixture({ runs: [plan.run], selectedId: plan.run.id, graph: fixtureGraph().graph, proposal: true }),
      renderFixture({ runs: [plan.run], selectedId: plan.run.id, graph: plan.graph.graph, viewMode: "details" }),
      renderFixture({ runs: [fixtureRun({ id: "decision-1", state: "NEEDS_INPUT", pending_decision: { id: "question-1", question: "Which scope should Luna use?" } })], selectedId: "decision-1", editor: "steer" }),
      renderFixture({ runs: [blocked], selectedId: blocked.id, editor: "stop" }),
      renderFixture({ runs: [plan.run], surface: "inline" }),
      renderFixture({ runs: [blocked], selectedId: blocked.id, surface: "inline" }),
      renderFixture({ runs: [plan.run], disconnected: true }),
      renderFixture({ runs: [], error: true }),
    ];

    expect(views[0]?.textContent).toContain("Plans");
    expect(views[2]?.querySelector(".plan-banner h2")?.textContent).toBe("Luna can't confirm the earlier run stopped.");
    expect(views[2]?.querySelector(".plan-banner p")?.textContent).toBe("Before starting new work, Luna needs to check what's still running in Codex.");
    expect(views[2]?.querySelector(".plan-banner [data-kind='reconcile']")?.textContent).toBe("Check running work");
    expect(views[2]?.querySelector(".plan-banner [data-kind='cancel']")?.textContent).toBe("Stop everything");
    for (const [index, root] of views.entries()) {
      const copy = exposedCopy(root);
      for (const term of bannedCopy) expect(copy, `fixture ${index} exposed ${term}`).not.toMatch(term);
    }
  });

  it("keeps the composer on Home and lists a campaign plan once", () => {
    const plan = fixtureCampaignPlan();
    const root = renderFixture({ runs: [plan.run], campaigns: [fixtureCampaign()] });
    expect(root.querySelector("h1")?.textContent).toBe("Plans");
    expect(root.querySelector(".plan-composer")).not.toBeNull();
    expect(root.querySelectorAll(".plan-row")).toHaveLength(1);
    expect(root.textContent?.match(/Actions v2: uv packaging and cross-platform CI/g)).toHaveLength(1);
    expect(root.querySelectorAll(".home-group")).toHaveLength(1);
  });

  it("uses the composer as the empty state and keeps options focused", () => {
    const root = renderFixture({ runs: [] });
    expect(root.querySelector(".plan-composer")).not.toBeNull();
    expect(root.querySelector(".plan-list")).toBeNull();
    expect(root.querySelector(".empty")).toBeNull();
    expect(Array.from(root.querySelectorAll("#plan-more .field label"), element => element.textContent)).toEqual(["Leave alone", "When it's done"]);
    expect(root.querySelectorAll(".button.primary")).toHaveLength(1);
    expect(root.querySelector(".plan-go")?.textContent).toBe("Plan it");
  });

  it("offers setup for an unqualified plan without a refresh action", () => {
    const plan = fixtureCampaignPlan();
    const root = renderFixture({ runs: [plan.run], selectedId: plan.run.id, campaigns: [fixtureCampaign()], graph: plan.graph.graph });
    expect(root.querySelectorAll(".page-primary")).toHaveLength(1);
    expect(root.querySelector(".page-primary")?.textContent).toBe("Set up running");
    expect(root.querySelector(".decision-panel")).toBeNull();
    expect(root.textContent).not.toContain("Refresh planning graph");
  });

  it("keeps proposal confirmation on the Apply change button", () => {
    const plan = fixtureCampaignPlan();
    const root = renderFixture({ runs: [plan.run], selectedId: plan.run.id, graph: plan.graph.graph, proposal: true });
    expect(root.querySelector('[aria-label="Review plan change"]')).not.toBeNull();
    expect(root.querySelector("#confirm-graph-change")).toBeNull();
    expect(root.querySelector(".graph-review")?.textContent).toContain("Before");
    expect(root.querySelector(".graph-review")?.textContent).toContain("After");
    expect(root.querySelector('[data-action="apply-change"]')?.textContent).toContain("Apply change");
  });

  it("shows Activity only when a run has activity and orders events newest first", () => {
    const planned = fixtureCampaignPlan();
    const plannedRoot = renderFixture({ runs: [planned.run], selectedId: planned.run.id, graph: planned.graph.graph });
    expect(plannedRoot.querySelector('[data-mode="lanes"]')).toBeNull();

    const swarm = fixtureSwarmRun();
    const activeRoot = renderFixture({ runs: [swarm], selectedId: swarm.id, graph: fixtureGraph().graph, viewMode: "lanes" });
    expect(activeRoot.querySelector('[data-mode="lanes"]')?.textContent).toContain("Activity");
    const times = Array.from(activeRoot.querySelectorAll<HTMLElement>(".activity-event time"), element => element.getAttribute("datetime"));
    expect(times.length).toBeGreaterThan(1);
    expect(times[0]).toBeTruthy();
  });

  it("shows one initial error state and a reconnect banner for saved data", () => {
    const error = renderFixture({ runs: [], error: true });
    expect(error.querySelector(".full-page-error")).not.toBeNull();
    expect(error.querySelector(".skeleton")).toBeNull();

    const plan = fixtureCampaignPlan();
    const offline = renderFixture({ runs: [plan.run], selectedId: plan.run.id, disconnected: true });
    expect(offline.querySelector(".offline-banner")?.textContent).toContain("Offline. Showing what Luna saw at");
    expect(offline.querySelector('.offline-banner [data-action="reconnect"]')?.textContent).toContain("Reconnect");
  });
});
