// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { roster } from "../src/agents";
import { fitGeometry, layoutWaves } from "../src/campaign-map";
import { WorkbenchController, type Bridge } from "../src/controller";
import { renderLanes } from "../src/lanes";
import { nowSentence, runTier, SinceTracker, statusLabel } from "../src/narrative";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureCampaign, fixtureCampaignPlan, fixturePlanningRun, fixtureRun, fixtureSwarmRun, fixtureWorkbench } from "./fixtures";
import { campaignListSchema } from "../src/domain";

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

describe("campaign grouping", () => {
  const decision = () => fixtureRun({ id: "d1", state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick?" } });
  const home = () => ({ ...fixtureWorkbench, runs: [decision(), fixtureSwarmRun(), fixtureCampaignPlan().run, fixtureRun({ id: "b1", state: "BLOCKED" })] });
  function homeController(list: () => unknown) {
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => tool === "list_factory_campaigns" ? list() : tool === "refresh_factory" ? { structuredContent: home() } : { isError: true, content: [{ type: "text", text: "unexpected" }] });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.setConnected(true);
    controller.receiveInitial({ structuredContent: home() });
    return { controller, call };
  }
  const heading = (root: HTMLElement, group: string) => Array.from(root.querySelector(`.group-${group} h2`)?.childNodes ?? []).filter(child => child.nodeType === Node.TEXT_NODE).map(child => child.textContent).join("");
  it("nests the plan under its campaign row and keeps unlinked runs in their tiers", async () => {
    const { controller, call } = homeController(() => ({ structuredContent: { campaigns: [fixtureCampaign()] } }));
    await controller.refresh();
    expect(call).toHaveBeenCalledWith("list_factory_campaigns", { limit: 100 });
    expect(controller.state.error).toBeNull();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    const entity = root.querySelector(".group-campaigns .campaign-entity");
    expect(heading(root, "campaigns")).toBe("Parent campaigns");
    // Opening the campaign opens its planning run's Campaign Map.
    const row = entity?.querySelector<HTMLAnchorElement>(".campaign-parent-row");
    expect(row?.dataset.runId).toBe("campaign-actions-v2");
    expect(row?.getAttribute("href")).toBe("/runs/campaign-actions-v2");
    expect(row?.textContent).toContain("GitHub #101 · reported by GitHub");
    expect(row?.textContent).toContain("14 planned tasks · Execution hasn't started.");
    expect(row?.textContent).toContain("Planned · not started");
    const children = Array.from(entity?.querySelectorAll<HTMLElement>(".campaign-children > li") ?? []);
    expect(children.map(child => child.querySelector(".child-kind")?.textContent ?? child.textContent)).toEqual(["Plan", "No execution runs yet"]);
    expect(children[0]?.querySelector<HTMLAnchorElement>("a")?.dataset.runId).toBe("campaign-actions-v2");
    // The linked plan leaves the unlinked tiers; everything else keeps its current grouping.
    expect(root.querySelector(".group-planned")).toBeNull();
    expect(root.querySelectorAll('[data-run-id="campaign-actions-v2"]')).toHaveLength(2);
    expect(root.querySelector(".group-needs")?.textContent).toContain("Pick?");
    expect(root.querySelector(".group-live [data-run-id='swarm-fixture']")).not.toBeNull();
    expect(root.querySelector(".group-history [data-run-id='b1']")).not.toBeNull();
    expect(root.querySelector(".home-head p")?.textContent).toBe("1 campaign · 1 needs you · 1 in progress · 1 in history");
    // Campaign data never creates attention or proof.
    expect(entity?.querySelector(".tier-needs")).toBeNull();
    expect(entity?.textContent).not.toContain("proven ·");
    const order = Array.from(root.querySelectorAll(".home-group")).map(section => section.className.replace("home-group group-", ""));
    expect(order).toEqual(["needs", "campaigns", "live", "history"]);
  });
  it("never hides a real decision inside a campaign", async () => {
    const linked = fixtureCampaign({ run_ids: ["d1"], runs: [{ id: "d1", state: "NEEDS_INPUT", updated_at: 1791141000 }] });
    const { controller } = homeController(() => ({ structuredContent: { campaigns: [linked] } }));
    await controller.refresh();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".group-needs [data-run-id='d1']")).not.toBeNull();
    expect(root.querySelector(".group-campaigns .campaign-child.tier-needs[data-run-id='d1']")).not.toBeNull();
    expect(root.querySelector(".group-campaigns .campaign-parent-row")?.textContent).toContain("1 linked run.");
  });
  it("keeps the current grouping when the server has no campaign tool", async () => {
    const { controller } = homeController(() => ({ isError: true, content: [{ type: "text", text: "unknown_tool" }] }));
    await controller.refresh();
    expect(controller.state.campaigns).toBeNull();
    expect(controller.state.error).toBeNull();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".group-campaigns")).toBeNull();
    expect(root.querySelector(".group-planned [data-run-id='campaign-actions-v2']")).not.toBeNull();
    expect(root.querySelector(".home-head p")?.textContent).toBe("1 needs you · 1 in progress · 1 planned · 1 in history");
  });
  it("keeps the last valid grouping, marked stale, after a failed or inconsistent read", async () => {
    let next: unknown = { structuredContent: { campaigns: [fixtureCampaign()] } };
    const { controller } = homeController(() => next);
    await controller.refresh();
    for (const bad of [
      { isError: true, content: [{ type: "text", text: "read failed" }] },
      { structuredContent: { campaigns: [fixtureCampaign({ planning: { ...fixtureCampaign().planning, run_id: "other-run" } })] } },
      { structuredContent: { campaigns: [{ ...fixtureCampaign(), promotion: { allowed: true, reason: "forged" } }] } },
      { structuredContent: { campaigns: [fixtureCampaign({ run_ids: ["campaign-actions-v2"] })] } },
    ]) {
      next = bad;
      await controller.refresh();
      expect(controller.state.campaigns?.map(campaign => campaign.id)).toEqual(["campaign-entity-101"]);
      expect(controller.state.campaignsStale).toBe(true);
      expect(controller.state.error).toBeNull();
    }
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".group-campaigns [role='status']")?.textContent).toContain("last valid read");
    next = { structuredContent: { campaigns: [fixtureCampaign()] } };
    await controller.refresh();
    expect(controller.state.campaignsStale).toBe(false);
    controller.setDisconnected("Host disconnected");
    expect(controller.state.campaignsStale).toBe(true);
  });
  it("labels the campaign parent on its Campaign Map without exposing internal identities", async () => {
    const { controller } = await campaignController();
    controller.state.campaigns = [fixtureCampaign()];
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".campaign-head .eyebrow")?.textContent).toContain("GitHub #101 · reported by GitHub");
    const primary = root.cloneNode(true) as HTMLElement;
    primary.querySelector("#evidence-audit")?.remove();
    primary.querySelector(".planning-editor")?.remove();
    primary.querySelector(".inspector .route-list")?.remove();
    for (const internal of ["Revision", "claim", "1148934299", "I_kwDOsynthetic101", "sha256:"]) expect(primary.textContent).not.toContain(internal);
  });
  it("accepts the server projection shape and rejects contradictory counts", () => {
    expect(campaignListSchema.safeParse({ campaigns: [fixtureCampaign()] }).success).toBe(true);
    const counts = fixtureCampaign();
    counts.planning.tasks.total += 1;
    expect(campaignListSchema.safeParse({ campaigns: [counts] }).success).toBe(false);
    expect(campaignListSchema.safeParse({ campaigns: [{ ...fixtureCampaign(), planning: { ...fixtureCampaign().planning, planning_only: false } }] }).success).toBe(false);
  });
});
