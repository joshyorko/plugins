// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { App } from "@modelcontextprotocol/ext-apps";
import { AppBridge } from "@modelcontextprotocol/ext-apps/app-bridge";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { contextTitle, HostBridge } from "../src/bridge";
import { WorkbenchController, type Bridge } from "../src/controller";
import { SinceTracker } from "../src/narrative";
import { describeView, registerViewTools, viewToolHandlers } from "../src/view-tools";
import { fixtureBackends, fixtureCampaignPlan, fixtureRun, fixtureSwarmRun } from "./fixtures";

async function opened(run = fixtureSwarmRun()) {
  const plan = fixtureCampaignPlan();
  const graph = run.id === plan.run.id ? plan.graph : { graph: { ...plan.graph.graph, run_id: run.id, revision: run.control!.revision, planning_only: false, nodes: run.control!.tasks.map(task => ({ ...task, source: null, target_preference: null })), attempts: [] }, proposal: null };
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : tool === "get_factory_run" ? run : graph }));
  const context = vi.fn<Bridge["context"]>().mockResolvedValue();
  const selectContext = vi.fn();
  const controller = new WorkbenchController({ call, context, selectContext }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: run });
  await controller.loadGraph();
  call.mockClear(); selectContext.mockClear();
  return { controller, call, context, selectContext };
}

describe("model-callable view tools", () => {
  it("describe the view without raw thread identities", async () => {
    const { controller } = await opened();
    controller.selectAgent("child-1");
    const view = describeView(controller, "global");
    expect(view).toMatchObject({ view: "map", run: { run_id: "swarm-fixture" }, task: { task_id: "child:child-1" }, agent: { label: "Worker 1", role: "worker" } });
    expect(JSON.stringify(view)).not.toMatch(/"(?:agent_)?thread/);
  });
  it("move only the view and never call a server mutation", async () => {
    const { controller, call, selectContext } = await opened();
    const tools = viewToolHandlers(controller, { surface: () => "global", readPlan: () => undefined });
    expect((await tools.focusAgent({ label: "worker 2" })).isError).toBeUndefined();
    expect(controller.state.selectedAgent).toBe("child-2");
    expect(controller.state.selectedNodeId).toBe("child:child-2");
    expect((await tools.focusTask({ task_id: "objective" })).structuredContent).toMatchObject({ task: { task_id: "objective" } });
    expect((await tools.showView({ mode: "lanes" })).structuredContent).toMatchObject({ view: "lanes" });
    expect((await tools.focusTask({ task_id: "not-a-task" })).isError).toBe(true);
    expect((await tools.focusAgent({ label: "Worker 9" })).isError).toBe(true);
    expect((await tools.openRun({ run_id: "missing" })).isError).toBe(true);
    // Model-driven focus never reattaches context the user removed.
    expect(selectContext).not.toHaveBeenCalled();
    const mutations = ["start_factory", "steer_factory_run", "cancel_factory_run", "resume_factory_run", "reconcile_factory_run", "propose_factory_change", "apply_factory_change", "create_factory_graph"];
    expect(call.mock.calls.filter(([tool]) => mutations.includes(tool))).toEqual([]);
  });
  it("are listed to the host with read-only annotations and callable over MCP Apps", async () => {
    const { controller } = await opened();
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.2.1" }, { tools: { listChanged: false } }, { autoResize: false });
    registerViewTools(app as never, controller, { surface: () => "global", readPlan: () => undefined });
    const host = new AppBridge(null, { name: "Synthetic host", version: "1" }, { serverTools: {} });
    await host.connect(hostTransport);
    await app.connect(appTransport);
    const listed = await host.listTools({});
    expect(listed.tools.map(tool => tool.name).sort()).toEqual(["luna_focus_agent", "luna_focus_task", "luna_open_run", "luna_read_view", "luna_show_view"]);
    expect(listed.tools.every(tool => tool.annotations?.readOnlyHint === true && tool.annotations?.destructiveHint === false)).toBe(true);
    const result = await host.callTool({ name: "luna_focus_agent", arguments: { label: "Luna" } });
    expect(result.structuredContent).toMatchObject({ agent: { label: "Luna", role: "coordinator" } });
  });
});

describe("context chip title", () => {
  it("labels the selection from bounded fields only", () => {
    expect(contextTitle({ run_id: "r", agent_label: "Worker 2", node_title: "Campaign entity grouping a parent issue, its plan and its runs" })).toBe("Worker 2 · Campaign entity grouping a parent issue, its pl…");
    expect(contextTitle({ run_id: "r", objective: "Ship it", repository: "plugins" })).toBe("Ship it");
    expect(contextTitle({ run_id: "r", repository: "plugins" })).toBe("Luna Factory · plugins");
    expect(contextTitle({})).toBeNull();
  });
  it("sends a titled text block beside structured context", async () => {
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.2.1" }, {}, { autoResize: false });
    const host = new AppBridge(null, { name: "Synthetic host", version: "1" }, { updateModelContext: { structuredContent: {}, text: {} }, experimental: { "openai/modelContext": {} } });
    const updates: unknown[] = [];
    host.onupdatemodelcontext = async params => { updates.push(params); return { _meta: { "openai/modelContext": { updateId: "u1" } } }; };
    await host.connect(hostTransport);
    const bridge = new HostBridge(app, () => undefined);
    await bridge.connect(appTransport);
    await bridge.context({ run_id: "run-1", objective: "Ship the map", agent_label: "Luna" });
    expect(updates.at(-1)).toMatchObject({ structuredContent: { run_id: "run-1" }, content: [{ type: "text", text: "Luna · Ship the map", _meta: { "openai/title": "Luna · Ship the map" } }] });
  });
});

describe("since you last looked", () => {
  it("restores baselines from host widget state and persists new ones", () => {
    const saved = vi.fn();
    const tracker = new SinceTracker(() => 2_000, { "run-123": { at: 1_000, updated: 900 } }, saved);
    const run = fixtureRun({ updated_at: 1_500, receipts: [{ kind: "execution", summary: "Landed while you were away", subject: "s", created_at: 1_400 }] });
    tracker.observe([run]);
    expect(saved).not.toHaveBeenCalled();
    expect(tracker.changes(run)?.lines).toEqual([expect.stringContaining("Landed while you were away")]);
    tracker.observe([fixtureRun({ id: "new-run" })]);
    expect(saved).toHaveBeenCalledWith(expect.objectContaining({ "run-123": { at: 1_000, updated: 900 }, "new-run": expect.any(Object) }));
    tracker.markSeen([run]);
    expect(tracker.changes(run)).toBeNull();
  });
});
