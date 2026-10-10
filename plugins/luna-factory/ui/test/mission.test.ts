// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { roster } from "../src/agents";
import { fitGeometry, layoutWaves } from "../src/campaign-map";
import { WorkbenchController, type Bridge } from "../src/controller";
import { renderLanes } from "../src/lanes";
import { nowSentence, runTier, SinceTracker, statusLabel } from "../src/narrative";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureCampaignPlan, fixturePlanningRun, fixtureRun, fixtureSwarmRun, fixtureWorkbench } from "./fixtures";

const node = (id: string, dependencies: string[] = [], state = "candidate") => ({ id, title: id, dependencies, state, owner_thread: null });

describe("campaign map layout", () => {
  it("places tasks in dependency waves by longest prerequisite path", () => {
    const layout = layoutWaves([node("d", ["b", "c"]), node("a"), node("b", ["a"]), node("c", ["a"])]);
    expect(layout.waves.map(wave => [...wave].sort())).toEqual([["a"], ["b", "c"], ["d"]]);
    expect(layout.dependents.get("a")?.sort()).toEqual(["b", "c"]);
  });
  it("derives prerequisites-met and the longest remaining chain without claiming durations", () => {
    const layout = layoutWaves([node("a", [], "done"), node("b", ["a"]), node("c", ["b"]), node("x")]);
    expect([...layout.prerequisitesMet].sort()).toEqual(["b", "x"]);
    expect(layout.remainingChain).toEqual(["b", "c"]);
  });
  it("reports unknown prerequisites and survives a dependency loop", () => {
    const layout = layoutWaves([node("a", ["ghost"]), node("b", ["c"]), node("c", ["b"])]);
    expect(layout.missing.get("a")).toEqual(["ghost"]);
    expect(layout.cyclic).toBe(true);
    expect(layout.waves.flat().sort()).toEqual(["a", "b", "c"]);
  });
  it("fits wave columns to the pane before panning", () => {
    expect(fitGeometry(5, 1040).nodeWidth).toBeLessThan(216);
    expect(fitGeometry(2, 1040).nodeWidth).toBe(216);
    expect(fitGeometry(12, 600).nodeWidth).toBe(156);
  });
});

describe("narrative and attention", () => {
  it("keeps Needs you identical to the server-authorized decision rule", () => {
    expect(runTier(fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick one?" } }))).toBe("needs");
    expect(runTier(fixtureRun({ state: "BLOCKED", pending_decision: null }))).toBe("unverified");
    expect(runTier(fixturePlanningRun())).toBe("planned");
    expect(runTier(fixtureRun({ state: "CANCELLED" }))).toBe("stopped");
  });
  it("uses the server's result label outside the two UI-owned tiers", () => {
    const run = fixtureRun({ state: "RUNNING" });
    expect(statusLabel(run)).toBe(run.presentation?.result.label);
    expect(nowSentence(fixturePlanningRun())).toContain("Execution hasn't started");
  });
  it("reports only server changes after this session began", () => {
    let clock = 1_000;
    const tracker = new SinceTracker(() => clock);
    const run = fixtureRun({ updated_at: 900, receipts: [{ kind: "execution", summary: "Old receipt newer than updated_at", subject: "s", created_at: 950 }] });
    tracker.observe([run]);
    expect(tracker.changes(run)).toBeNull();
    clock = 1_200;
    const later = { ...run, updated_at: 1_100, receipts: [...run.receipts, { kind: "execution", summary: "New receipt", subject: "s", created_at: 1_150 }] };
    tracker.observe([later]);
    expect(tracker.changes(later)?.lines).toEqual([expect.stringContaining("New receipt")]);
  });
});

describe("agents and lanes", () => {
  it("builds the roster only from reported owner and workers", () => {
    const agents = roster(fixtureSwarmRun(), null);
    expect(agents.map(agent => [agent.label, agent.role, agent.liveness, agent.taskId])).toEqual([
      ["Luna", "coordinator", "active", "objective"], ["Worker 1", "worker", "active", "child:child-1"], ["Worker 2", "worker", "unknown", "child:child-2"],
    ]);
    expect(roster(fixturePlanningRun(), null)).toEqual([]);
  });
  it("never extends an old record to now and marks per-agent timelines unavailable", () => {
    const run = fixtureSwarmRun();
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    const html = renderLanes(run, roster(run, null), controller.state, (run.updated_at ?? 0) + 30 * 86_400);
    expect(html).toContain("last event");
    expect(html).not.toContain("now-line");
    expect(html).toContain("Per-agent timeline not reported by this server");
    expect(renderLanes(fixturePlanningRun(), [], controller.state)).toContain("Execution hasn't started on this plan");
  });
});

async function campaignController(run = fixtureCampaignPlan().run) {
  const plan = fixtureCampaignPlan();
  const graph = run.id === plan.run.id ? plan.graph : { graph: { ...plan.graph.graph, run_id: run.id, revision: run.control!.revision, planning_only: false, nodes: run.control!.tasks.map(task => ({ ...task, source: null, target_preference: null })), attempts: [] }, proposal: null };
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : tool === "get_factory_run" ? run : graph }));
  const context = vi.fn<Bridge["context"]>().mockResolvedValue();
  const controller = new WorkbenchController({ call, context }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: run });
  await controller.loadGraph();
  return { controller, context, call };
}

describe("mission control surfaces", () => {
  it("renders the plan as waves and keeps internal identities out of the primary view", async () => {
    const { controller } = await campaignController();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelectorAll(".map-node")).toHaveLength(14);
    expect(root.querySelectorAll(".wave-label")).toHaveLength(5);
    const primary = root.cloneNode(true) as HTMLElement;
    primary.querySelector("#evidence-audit")?.remove();
    primary.querySelector(".planning-editor")?.remove();
    primary.querySelector(".inspector .route-list")?.remove();
    for (const internal of ["Revision", "claim", "subject", "1148934299", "f51105bf"]) expect(primary.textContent).not.toContain(internal);
  });
  it("shares one task-and-agent selection across Map and Lanes and sends it to ChatGPT without thread ids", async () => {
    const { controller, context } = await campaignController(fixtureSwarmRun());
    controller.selectAgent("child-2");
    expect(controller.state.selectedNodeId).toBe("child:child-2");
    await vi.waitFor(() => expect(context).toHaveBeenLastCalledWith(expect.objectContaining({ node_id: "child:child-2", agent_label: "Worker 2", agent_role: "worker", agent_liveness: "unknown" })));
    expect(JSON.stringify(context.mock.lastCall?.[0])).not.toMatch(/"(?:agent_)?thread/);
    controller.setViewMode("lanes");
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector('.lanes [data-thread-id="child-2"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector('.crew [data-thread-id="child-2"]')?.getAttribute("aria-pressed")).toBe("true");
    controller.selectNode("objective");
    expect(controller.state.selectedAgent).toBeNull();
    controller.selectAgent("not-a-reported-thread");
    expect(controller.state.selectedAgent).toBeNull();
  });
  it("keeps the inline card to two actions and shows no invented agents for a plan", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [fixtureCampaignPlan().run] } });
    controller.setConnected(true);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canExpand: true, canSendFollowUps: true });
    expect(root.querySelectorAll(".inline-card button")).toHaveLength(2);
    expect(root.textContent).toContain("Planned · not started");
    expect(root.querySelector(".token")).toBeNull();
  });
  it("shows a rerouted model only as stop evidence, never as an escalation treatment", async () => {
    const run = fixtureSwarmRun();
    run.route.observed_model = "gpt-6.1-sol";
    run.route.observed_model_source = "model/rerouted";
    run.route.reroutes = [{ thread_id: "owner-a", turn_id: "turn-a", from_model: "gpt-6-luna", to_model: "gpt-6.1-sol", reason: "highRiskCyberActivity", source: "model/rerouted" }];
    const { controller } = await campaignController(run);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector("#evidence-audit")?.textContent).toContain("Execution was stopped for review");
    expect(root.querySelector('[class*="sol"], [class*="astra"]')).toBeNull();
    const primary = root.cloneNode(true) as HTMLElement;
    primary.querySelector("#evidence-audit")?.remove();
    expect(primary.textContent).not.toContain("gpt-6.1-sol");
  });
  it("groups the home view by attention tier", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [fixtureRun({ id: "d1", state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick?" } }), fixtureSwarmRun(), fixtureCampaignPlan().run, fixtureRun({ id: "b1", state: "BLOCKED" })] } });
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(Array.from(root.querySelectorAll(".home-group h2")).map(heading => Array.from(heading.childNodes).filter(child => child.nodeType === Node.TEXT_NODE).map(child => child.textContent).join(""))).toEqual(expect.arrayContaining(["Needs you", "In progress", "Planned · not started", "History"]));
    expect(root.querySelector(".group-needs")?.textContent).toContain("Pick?");
    expect(root.querySelector(".home-head p")?.textContent).toBe("1 needs you · 1 in progress · 1 planned · 1 in history");
  });
});

describe("motion and message copy", () => {
  it("marks a task changed only when a later snapshot reports a different state", async () => {
    const { controller } = await campaignController(fixtureSwarmRun());
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".map-node.changed")).toBeNull();
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".map-node.changed")).toBeNull();
    controller.state.graph!.nodes.find(node => node.id === "child:child-2")!.state = "done";
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector('[data-node-id="child:child-2"]')?.classList.contains("changed")).toBe(true);
    expect(root.querySelectorAll(".map-node.changed")).toHaveLength(1);
  });
  it("blames the host only when the host cannot send messages", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureCampaignPlan().run }); controller.setConnected(true);
    const root = document.createElement("div");
    controller.state.refreshing = true;
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true, canExpand: true });
    expect(root.textContent).not.toContain("unavailable in this host");
    expect(root.textContent).toContain("Available once the current read finishes");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: false, canExpand: true });
    expect(root.textContent).toContain("unavailable in this host");
  });
});
