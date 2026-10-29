// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { roster } from "../src/agents";
import { fitGeometry, layoutWaves } from "../src/campaign-map";
import { WorkbenchController, type Bridge } from "../src/controller";
import { renderLanes } from "../src/lanes";
import { freshness, nowSentence, runTier, SinceTracker, statusLabel } from "../src/narrative";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureCampaign, fixtureCampaignPlan, fixturePlanningRun, fixtureRun, fixtureSwarmRun, fixtureTimelineCapability, fixtureTimelines, fixtureWorkbench } from "./fixtures";
import { campaignListSchema, classifyRun, needsOperatorDecision, parentLabel, type Capabilities } from "../src/domain";
import { MAX_TIMELINE_AGENTS, TIMELINE_INTERVAL_MS } from "../src/controller";

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
    expect(fitGeometry(2, 1040).nodeWidth).toBe(208);
    expect(fitGeometry(2, 1040, 1.25)).toEqual({ nodeWidth: 260, nodeHeight: 120, columnGap: 95, rowGap: 20, padX: 25, padY: 28 });
    expect(fitGeometry(12, 600).nodeWidth).toBe(208);
    expect(fitGeometry(12, 600).columnGap).toBe(36);
  });
});

describe("narrative and attention", () => {
  it("keeps Needs you identical to the server-authorized decision rule", () => {
    expect(runTier(fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick one?" } }))).toBe("needs");
    expect(runTier(fixtureRun({ state: "BLOCKED", pending_decision: null }))).toBe("stopped");
    expect(runTier(fixturePlanningRun())).toBe("planned");
    expect(runTier(fixtureRun({ state: "CANCELLED" }))).toBe("stopped");
  });
  it("uses plain state labels for active plans", () => {
    const run = fixtureRun({ state: "RUNNING" });
    expect(statusLabel(run)).toBe("Running");
    expect(nowSentence(fixturePlanningRun())).toBe("1 task in this plan.");
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
      ["Luna", "coordinator", "active", "objective"], ["Agent 1", "worker", "active", "child:child-1"], ["Agent 2", "worker", "unknown", "child:child-2"],
    ]);
    expect(roster(fixturePlanningRun(), null)).toEqual([]);
  });
  it("shows reported events without extending old activity to now", () => {
    const run = fixtureSwarmRun();
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    const html = renderLanes(run, roster(run, null), controller.state, (run.updated_at ?? 0) + 30 * 86_400);
    expect(html).toContain("Recent activity");
    expect(html).not.toContain("now-line");
    expect(html).toContain("Recent activity");
    const legacy = { ...run, receipts: run.receipts.map(({ thread_id: _thread, ...receipt }) => receipt) };
    expect(renderLanes(legacy, roster(legacy, null), controller.state)).toContain("Recent activity");
    expect(renderLanes(fixturePlanningRun(), [], controller.state)).toContain("No recent activity has been reported.");
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
  it("shares one task-and-agent selection across Tasks and Activity and sends it to ChatGPT without thread ids", async () => {
    const { controller, context } = await campaignController(fixtureSwarmRun());
    controller.selectAgent("child-2");
    expect(controller.state.selectedNodeId).toBe("child:child-2");
    await vi.waitFor(() => expect(context).toHaveBeenLastCalledWith(expect.objectContaining({ node_id: "child:child-2", agent_label: "Agent 2", agent_role: "worker", agent_liveness: "unknown" })));
    expect(JSON.stringify(context.mock.lastCall?.[0])).not.toMatch(/"(?:agent_)?thread/);
    controller.setViewMode("lanes");
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector('.activity-agent [data-thread-id="child-2"]')?.getAttribute("aria-pressed")).toBe("true");
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
    expect(root.querySelector(".inline-top .status")?.textContent).toContain("Planned");
    expect(root.querySelector(".token")).toBeNull();
  });
  it("shows a rerouted model in Details without escalating it", async () => {
    const run = fixtureSwarmRun();
    run.route.observed_model = "gpt-6.1-sol";
    run.route.observed_model_source = "model/rerouted";
    run.route.reroutes = [{ thread_id: "owner-a", turn_id: "turn-a", from_model: "gpt-6-luna", to_model: "gpt-6.1-sol", reason: "highRiskCyberActivity", source: "model/rerouted" }];
    const { controller } = await campaignController(run);
    const root = document.createElement("div");
    controller.setViewMode("details");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".model-note")?.textContent).toContain("model changed during this run");
    expect(root.querySelector('[class*="sol"], [class*="astra"]')).toBeNull();
    expect(root.querySelector(".plan-banner")).toBeNull();
    expect(root.querySelector(".page-primary")).toBeNull();
  });
  it("groups Home plans by current state without empty groups", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    const blocked = fixtureRun({ id: "b1", state: "BLOCKED" });
    blocked.presentation!.blocker_kind = { kind: "liveness_unknown" };
    blocked.presentation!.primary_action = { kind: "reconcile", label: "Check running work", reason: "read_only_native_reconciliation", tool: "reconcile_factory_run", allowed: true };
    const unknown = fixtureRun({ id: "u1", state: "RUNNING" });
    unknown.control = undefined;
    unknown.presentation = undefined;
    const terminal = fixtureRun({ id: "history-1", state: "CANCELLED", owner_thread: null, turn_id: null, active_workers: 0, claim_held: false });
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [fixtureRun({ id: "d1", state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick?" } }), fixtureSwarmRun(), fixtureCampaignPlan().run, blocked, unknown, terminal] } });
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(Array.from(root.querySelectorAll(".home-group h2")).map(heading => Array.from(heading.childNodes).filter(child => child.nodeType === Node.TEXT_NODE).map(child => child.textContent).join(""))).toEqual(["Needs you", "Running", "Planned"]);
    expect(root.querySelector(".group-needs")?.textContent).toContain("Pick?");
    expect(root.querySelectorAll(".plan-row")).toHaveLength(6);
    expect(root.querySelector<HTMLDetailsElement>(".home-history")?.open).toBe(false);
    expect(root.querySelector(".home-history > summary")?.textContent).toBe("Show 1 from history");
    const historyStatuses = Array.from(root.querySelectorAll(".home-history .status"), status => status.textContent);
    expect(historyStatuses).toEqual(["Stopped"]);
    expect(root.querySelector(".group-needs [data-run-id='b1'] .status")?.textContent).toContain("Blocked");
    expect(root.querySelector(".group-needs [data-run-id='u1'] .status")?.textContent).toContain("Status unknown");
    expect(root.querySelector(".home-group .group-empty")).toBeNull();
  });
});

describe("motion and message copy", () => {
  it("uses compact relative time and a short date for Home rows", () => {
    const now = Date.parse("2026-10-29T12:00:00Z") / 1000;
    expect(freshness(now - 30, now)).toBe("now");
    expect(freshness(now - 300, now)).toBe("5m");
    expect(freshness(now - 3_600, now)).toBe("1h");
    expect(freshness(Date.parse("2026-10-04T12:00:00Z") / 1000, now)).toBe("Oct 4");
    expect(freshness(undefined, now)).toBe("Time unknown");
  });
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
    expect(root.querySelectorAll('[data-action="chat-follow-up"]')).toHaveLength(1);
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: false, canExpand: true });
    expect(Array.from(root.querySelectorAll<HTMLButtonElement>('[data-action="chat-follow-up"]')).every(button => button.disabled)).toBe(true);
  });
});

describe("Home plans", () => {
  const decision = () => fixtureRun({ id: "d1", state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick?" } });
  const home = () => ({ ...fixtureWorkbench, runs: [decision(), fixtureSwarmRun(), fixtureCampaignPlan().run, fixtureRun({ id: "b1", state: "CONVERGED", active_workers: 0, claim_held: false, blocker: null, remaining_gap: null })] });
  function homeController(list: () => unknown) {
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => tool === "list_factory_campaigns" ? list() : tool === "refresh_factory" ? { structuredContent: home() } : { isError: true, content: [{ type: "text", text: "unexpected" }] });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.setConnected(true);
    controller.receiveInitial({ structuredContent: home() });
    return { controller, call };
  }
  it("lists each campaign plan once with its source issue and keeps other plans by state", async () => {
    const { controller, call } = homeController(() => ({ structuredContent: { campaigns: [fixtureCampaign()] } }));
    await controller.refresh();
    expect(call).toHaveBeenCalledWith("list_factory_campaigns", { limit: 100 });
    expect(controller.state.campaigns).toHaveLength(1);
    expect(controller.state.campaigns?.[0]?.parent.display?.url).toBe("https://github.com/example/actions/issues/101");
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    const row = root.querySelector<HTMLElement>('.plan-row[data-run-id="campaign-actions-v2"]');
    expect(row?.querySelector<HTMLAnchorElement>(".plan-open")?.getAttribute("href")).toBe("/runs/campaign-actions-v2");
    expect(row?.outerHTML).toContain("14 tasks");
    expect(row?.querySelector(".plan-meta")?.innerHTML).toContain("from");
    expect(row?.querySelector(".plan-meta")?.textContent).toContain("from #101");
    expect(root.querySelectorAll('.plan-row[data-run-id="campaign-actions-v2"]')).toHaveLength(1);
    expect(root.querySelectorAll(".campaign-children, .group-campaigns")).toHaveLength(0);
    expect(root.querySelector(".group-needs [data-run-id='d1']")).not.toBeNull();
    expect(root.querySelector(".group-running [data-run-id='swarm-fixture']")).not.toBeNull();
    expect(root.querySelector(".home-done > summary")?.textContent).toBe("Show 1 done");
  });
  it("keeps the plan visible when the campaign read is unavailable", async () => {
    const { controller } = homeController(() => ({ isError: true, content: [{ type: "text", text: "unknown_tool" }] }));
    await controller.refresh();
    expect(controller.state.campaigns).toBeNull();
    expect(controller.state.error).toBeNull();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".group-campaigns")).toBeNull();
    expect(root.querySelector(".group-planned [data-run-id='campaign-actions-v2']")).not.toBeNull();
  });
  it("keeps the last valid plan list when a campaign read fails", async () => {
    let next: unknown = { structuredContent: { campaigns: [fixtureCampaign()] } };
    const { controller } = homeController(() => next);
    await controller.refresh();
    next = { isError: true, content: [{ type: "text", text: "read failed" }] };
    await controller.refresh();
    expect(controller.state.campaigns?.map(campaign => campaign.id)).toEqual(["campaign-entity-101"]);
    expect(controller.state.campaignsStale).toBe(true);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".home-read-note")?.textContent).toContain("Showing the last version");
  });
  it("links a source issue only when its GitHub URL is valid", async () => {
    const { controller } = homeController(() => ({ structuredContent: { campaigns: [fixtureCampaign()] } }));
    await controller.refresh();
    expect(controller.state.campaigns).toHaveLength(1);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector<HTMLAnchorElement>(".plan-row .plan-meta a")?.href).toBe("https://github.com/example/actions/issues/101");
    controller.state.campaigns = [fixtureCampaign({ parent: { ...fixtureCampaign().parent, display: { url: "javascript:alert(1)" } } })];
    renderWorkbench(root, controller.state, null, false);
    const row = root.querySelector('.plan-row[data-run-id="campaign-actions-v2"]');
    expect(row?.querySelector(".plan-meta a")).toBeNull();
    expect(row?.querySelector(".plan-meta")?.textContent).toContain("from #101");
  });
  it("counts unreadable records without showing their identifiers", async () => {
    const { controller } = homeController(() => ({ structuredContent: { campaigns: [fixtureCampaign()], skipped: [{ id: "broken", reason: "campaign_record_corrupt" }] } }));
    await controller.refresh();
    expect(controller.state.campaignsSkipped).toBe(1);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".home-read-note")?.textContent).toBe("1 plan could not be shown.");
    expect(root.textContent).not.toContain("broken");
  });
  it("accepts the server projection shape and rejects contradictory counts", () => {
    expect(campaignListSchema.safeParse({ campaigns: [fixtureCampaign()] }).success).toBe(true);
    expect(campaignListSchema.safeParse({ campaigns: [{ ...fixtureCampaign(), promotion: { allowed: true, reason: "explicit_revision_approval_required" } }] }).success).toBe(true);
    const counts = fixtureCampaign();
    counts.planning.tasks.total += 1;
    expect(campaignListSchema.safeParse({ campaigns: [counts] }).success).toBe(false);
    expect(campaignListSchema.safeParse({ campaigns: [{ ...fixtureCampaign(), planning: { ...fixtureCampaign().planning, planning_only: false } }] }).success).toBe(false);
    const parent = fixtureCampaign().parent;
    for (const bad of [
      { ...parent, key: "github:example/actions#102" },
      { ...parent, forge: { ...parent.forge, provider: "GitHub" } },
      { ...parent, forge: { ...parent.forge, repository: "Example/Actions" }, key: "github:Example/Actions#101" },
      { ...parent, provenance: "reported_by_github" },
    ]) expect(campaignListSchema.safeParse({ campaigns: [{ ...fixtureCampaign(), parent: bad }] }).success, JSON.stringify(bad)).toBe(false);
  });
  it("keeps the source repository label in its friendly form", () => {
    expect(parentLabel(fixtureCampaign())).toBe("GitHub example/actions#101");
    expect(parentLabel(fixtureCampaign({ parent: { ...fixtureCampaign().parent, provenance: "github_app" } }))).toBe("GitHub example/actions#101");
  });
});
async function observedController(capability: Capabilities["agent_timeline"] | null = fixtureTimelineCapability, reply?: (thread: string) => unknown) {
  const run = fixtureSwarmRun();
  const plan = fixtureCampaignPlan();
  const graph = { graph: { ...plan.graph.graph, run_id: run.id, revision: run.control!.revision, planning_only: false, nodes: run.control!.tasks.map(task => ({ ...task, source: null, target_preference: null })), attempts: [] }, proposal: null };
  const timelines = fixtureTimelines(run.id);
  const call = vi.fn<Bridge["call"]>().mockImplementation(async (tool, args) => {
    if (tool === "read_factory_agent_timeline") return reply ? reply(String(args.thread_id)) : { structuredContent: timelines[String(args.thread_id)] };
    return { structuredContent: tool === "get_factory_backends" ? fixtureBackends : tool === "get_factory_run" ? run : graph };
  });
  const context = vi.fn<Bridge["context"]>().mockResolvedValue();
  const controller = new WorkbenchController({ call, context }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [run], selected_run: run, capabilities: { ...fixtureWorkbench.capabilities, ...(capability ? { agent_timeline: capability } : {}) } } });
  await controller.loadGraph();
  const timelineCalls = () => call.mock.calls.filter(([tool]) => tool === "read_factory_agent_timeline");
  return { controller, context, call, run, timelineCalls };
}

describe("observed agent timelines", () => {
  it("reads only rostered agents, only in Lanes, at a bounded rate", async () => {
    let clock = 1_791_142_800_000;
    const now = vi.spyOn(Date, "now").mockImplementation(() => clock);
    try {
      const { controller, timelineCalls, run } = await observedController();
      await controller.loadTimelines();
      expect(timelineCalls()).toHaveLength(0);
      controller.setViewMode("lanes");
      await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(3));
      expect(timelineCalls().map(([, args]) => args)).toEqual(["owner-a", "child-1", "child-2"].map(thread_id => ({ run_id: run.id, thread_id })));
      controller.setViewMode("map"); controller.setViewMode("lanes"); controller.selectAgent("child-1");
      await controller.loadTimelines();
      expect(timelineCalls()).toHaveLength(3);
      clock += TIMELINE_INTERVAL_MS;
      await controller.loadTimelines();
      expect(timelineCalls()).toHaveLength(6);
      await controller.select(null);
      expect(controller.state.timelines).toEqual({});
    } finally { now.mockRestore(); }
  });
  it("renders agent rows and a newest-first Activity feed", async () => {
    const { controller, run } = await observedController();
    controller.setViewMode("lanes");
    await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(3));
    controller.selectAgent("child-1");
    const root = document.createElement("div");
    root.innerHTML = renderLanes(run, roster(run, controller.state.graph), controller.state, (run.updated_at ?? 0) + 60);
    expect(root.querySelectorAll(".activity-agent")).toHaveLength(3);
    expect(root.querySelectorAll(".activity-event").length).toBeGreaterThan(13);
    expect(root.querySelector(".activity-event small")).toBeNull();
    expect(root.textContent).not.toContain("Observed in Codex");
    expect(root.querySelectorAll(".lane-track, .run-lane, .lanes-legend")).toHaveLength(0);
    const times = Array.from(root.querySelectorAll<HTMLTimeElement>(".activity-event time"), time => Date.parse(time.dateTime));
    expect(times.every((time, index) => index === 0 || times[index - 1]! >= time)).toBe(true);
    controller.selectAgent("owner-a");
    root.innerHTML = renderLanes(run, roster(run, controller.state.graph), controller.state, (run.updated_at ?? 0) + 60);
    expect(root.querySelector('.activity-agent [data-thread-id="owner-a"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.textContent).not.toMatch(/cas_|child-1|owner-a/);
  });
  it("keeps recorded events and Codex observations distinct in one feed", async () => {
    const { controller, run } = await observedController();
    controller.setViewMode("lanes");
    await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(3));
    const root = document.createElement("div");
    root.innerHTML = renderLanes(run, roster(run, controller.state.graph), controller.state, (run.updated_at ?? 0) + 60);
    const events = Array.from(root.querySelectorAll(".activity-event"));
    expect(events.length).toBeGreaterThan(run.receipts.length);
    expect(events.map(event => event.querySelector("strong")?.textContent)).toContain("Agent 1");
    expect(events.some(event => event.querySelector("small")?.textContent === "Observed in Codex")).toBe(false);
    expect(root.textContent).not.toMatch(/receipts?|Factory proof/i);
  });
  it("does not turn missing observations into agent activity", async () => {
    const run = fixtureSwarmRun();
    run.receipts = run.receipts.map(({ thread_id: _thread, ...receipt }) => receipt);
    const view = async (capability: Capabilities["agent_timeline"] | null, reply?: (thread: string) => unknown) => {
      const { controller } = await observedController(capability, reply);
      controller.setViewMode("lanes");
      if (capability?.enabled) await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(3));
      const root = document.createElement("div");
      root.innerHTML = renderLanes(run, roster(run, null), controller.state);
      return root;
    };
    for (const root of [
      await view(null),
      await view({ source: "codex-action-server", enabled: false, detail: "Codex observation is not configured on this server." }),
      await view(fixtureTimelineCapability, () => ({ isError: true, content: [{ type: "text", text: "agent_thread_not_in_run" }] })),
      await view(fixtureTimelineCapability, () => ({ structuredContent: { ...fixtureTimelines(run.id)["owner-a"], binding_verified: true } })),
    ]) {
      expect(root.querySelectorAll(".activity-agent")).toHaveLength(3);
      expect(root.querySelectorAll(".activity-event").length).toBeGreaterThan(0);
      expect(root.textContent).not.toContain("No activity has been reported");
    }
  });
  it("reads at most the first eight agents while listing every reported agent", async () => {
    const run = fixtureSwarmRun();
    run.presentation!.workers = Array.from({ length: 10 }, (_, index) => ({ thread_id: `worker-${index + 1}`, liveness: "unknown" as const }));
    const template = fixtureTimelines(run.id)["child-1"]!;
    const call = vi.fn<Bridge["call"]>().mockImplementation(async (tool, args) => {
      if (tool !== "read_factory_agent_timeline") return { structuredContent: run };
      const thread = String(args.thread_id);
      return { structuredContent: { ...template, thread_id: thread, events: template.events.map(event => ({ ...event, thread_id: thread })) } };
    });
    const controller = new WorkbenchController({ call, context: vi.fn<Bridge["context"]>().mockResolvedValue() }, () => undefined);
    controller.setConnected(true);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [run], selected_run: run, capabilities: { ...fixtureWorkbench.capabilities, agent_timeline: fixtureTimelineCapability } } });
    controller.setViewMode("lanes");
    await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(MAX_TIMELINE_AGENTS));
    await controller.loadTimelines();
    expect(call.mock.calls.filter(([tool]) => tool === "read_factory_agent_timeline")).toHaveLength(MAX_TIMELINE_AGENTS);
    const root = document.createElement("div");
    root.innerHTML = renderLanes(run, roster(run, null), controller.state);
    expect(root.querySelectorAll(".activity-agent")).toHaveLength(11);
    expect(root.querySelectorAll('.activity-event small')).toHaveLength(0);
    expect(root.textContent).not.toContain("worker-9");
    expect(root.textContent).not.toContain("only the first 8 agents");
  });
  it("stops reading as soon as Lanes closes or the page is hidden", async () => {
    let release: () => void = () => undefined;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const { controller, timelineCalls } = await observedController(fixtureTimelineCapability, async thread => { await pending; return { structuredContent: fixtureTimelines("swarm-fixture")[thread] }; });
    controller.setViewMode("lanes");
    await vi.waitFor(() => expect(timelineCalls()).toHaveLength(1));
    controller.setViewMode("map");
    release();
    await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(1));
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(timelineCalls()).toHaveLength(1);
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    try {
      controller.setViewMode("lanes");
      await controller.loadTimelines();
      expect(timelineCalls()).toHaveLength(1);
    } finally { hidden.mockRestore(); }
    await controller.loadTimelines();
    await vi.waitFor(() => expect(timelineCalls()).toHaveLength(3));
  });
  it("stops between agents when the page becomes hidden mid-read", async () => {
    let release: () => void = () => undefined;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const { controller, timelineCalls } = await observedController(fixtureTimelineCapability, async thread => { await pending; return { structuredContent: fixtureTimelines("swarm-fixture")[thread] }; });
    controller.setViewMode("lanes");
    await vi.waitFor(() => expect(timelineCalls()).toHaveLength(1));
    // Lanes stays selected; only the page visibility changes while the first read is in flight.
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    try {
      release();
      await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(1));
      await new Promise(resolve => setTimeout(resolve, 0));
      expect(controller.state.viewMode).toBe("lanes");
      expect(timelineCalls()).toHaveLength(1);
    } finally { hidden.mockRestore(); }
    // The unread agents stay due once the page is visible again.
    await controller.loadTimelines();
    await vi.waitFor(() => expect(timelineCalls()).toHaveLength(3));
  });
  it("never changes attention, proof, the Map or the ChatGPT context", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_791_142_800_000);
    try {
      const { controller, context, run } = await observedController();
      const root = document.createElement("div");
      // The first render settles the snapshot-diff motion tracker shared across renders.
      renderWorkbench(root, controller.state, null, false);
      renderWorkbench(root, controller.state, null, false);
      const map = root.innerHTML;
      await vi.waitFor(() => expect(context).toHaveBeenCalled());
      const contextBefore = context.mock.lastCall?.[0];
      const attention = [needsOperatorDecision(run), classifyRun(run), JSON.stringify(controller.selected?.presentation), JSON.stringify(controller.selected?.control), JSON.stringify(controller.state.graph)];
      controller.setViewMode("lanes");
      await vi.waitFor(() => expect(Object.keys(controller.state.timelines)).toHaveLength(3));
      controller.setViewMode("map");
      renderWorkbench(root, controller.state, null, false);
      expect(root.innerHTML).toBe(map);
      expect([needsOperatorDecision(controller.selected!), classifyRun(controller.selected!), JSON.stringify(controller.selected?.presentation), JSON.stringify(controller.selected?.control), JSON.stringify(controller.state.graph)]).toEqual(attention);
      expect(context.mock.lastCall?.[0]).toEqual(contextBefore);
      expect(JSON.stringify(context.mock.calls)).not.toMatch(/Observed|summary|cargo test|events/);
    } finally { now.mockRestore(); }
  });
});
