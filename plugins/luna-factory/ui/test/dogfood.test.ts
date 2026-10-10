// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { boundedContext, classifyRun } from "../src/domain";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureGraph, fixturePlanningRun, fixtureRun, fixtureWorkbench } from "./fixtures";

function planningGraph() {
  const run = fixturePlanningRun();
  const envelope = fixtureGraph(6);
  envelope.graph.run_id = run.id;
  envelope.graph.nodes = ["objective", "task-a", "task-b"].map(id => ({ ...envelope.graph.nodes[0]!, id, title: id, criterion_ids: ["A1"], dependencies: id === "task-b" ? ["task-a"] : [] }));
  envelope.graph.criteria = run.control!.criteria;
  return envelope;
}
async function setup() {
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : planningGraph() }));
  const context = vi.fn<Bridge["context"]>().mockResolvedValue();
  const controller = new WorkbenchController({ call, context }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: fixturePlanningRun() });
  await controller.loadGraph();
  call.mockClear();
  return { controller, call, context };
}
describe("authenticated dogfood regressions reproduced in disposable fixtures", () => {
  it("does not turn a planning refresh or unresolved evidence into a human decision", () => {
    const run = fixturePlanningRun();
    expect(classifyRun(run)).toBe("recent");
    expect(classifyRun(fixtureRun({ state: "BLOCKED", pending_decision: null }))).toBe("recent");
    const root = document.createElement("div");
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: run }); controller.setConnected(true);
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector(".decision-panel .eyebrow")?.textContent).toBe("Plan navigation");
    expect(root.querySelector(".delivery-strip")?.closest("details")).not.toBeNull();
    expect(root.textContent).not.toContain("0 min");
    expect(root.textContent).toContain("Execution has not started");
  });
  it("keeps a real server-authorized answer in Needs me", () => {
    const run = fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "decision", question: "Choose the approved scope" } });
    expect(classifyRun(run)).toBe("needs");
    run.presentation!.primary_action.allowed = false;
    expect(classifyRun(run)).toBe("recent");
    run.pending_decision = null;
    run.presentation!.primary_action = { kind: "inspect", label: "Open native Codex", reason: "native_approval_requires_native_ui", tool: "get_factory_run", allowed: true };
    expect(classifyRun(run)).toBe("needs");
  });
  it("does not send a no-op prerequisite proposal or any disconnected mutation", async () => {
    const { controller, call } = await setup();
    expect(await controller.proposeChange({ kind: "set_dependencies", node_id: "objective", dependencies: [] })).toBe(false);
    expect(call).not.toHaveBeenCalled();
    controller.setDisconnected("Host disconnected");
    expect(await controller.proposeChange({ kind: "set_dependencies", node_id: "task-b", dependencies: [] })).toBe(false);
    expect(call).not.toHaveBeenCalled();
  });
  it("exports the current run and graph revision, then clears stale context", async () => {
    const { controller, context } = await setup();
    controller.selectNode("task-b");
    await vi.waitFor(() => expect(context).toHaveBeenLastCalledWith(expect.objectContaining({ run_id: "dogfood-plan", revision: 6, graph_revision: 6, node_id: "task-b" })));
    expect(boundedContext(fixturePlanningRun()).state).toBe("PLANNING");
    controller.setDisconnected("Host disconnected");
    await vi.waitFor(() => expect(context).toHaveBeenLastCalledWith({}));
    expect(controller.state.graphStale).toBe(true);
  });
  it("requires explicit apply confirmation and keeps unqualified execution out of ordinary forms", async () => {
    const { controller, call } = await setup();
    controller.state.proposal = { id: "proposal", base_revision: 5, idempotency_key: "key", fingerprint: "hash", actor: "local_operator", subject: "subject", status: "proposed", applied_revision: null, change: { kind: "set_dependencies", node_id: "task-b", dependencies: [] } };
    expect(await controller.applyChange()).toBe(false);
    expect(call).not.toHaveBeenCalled();
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector("#confirm-graph-change")).not.toBeNull();
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(true);
    expect(root.querySelector('select[name="target_id"]')?.closest("details")).not.toBeNull();
    expect(root.querySelector('option[value="codex-cloud"]')).toBeNull();
    controller.state.capabilities = fixtureWorkbench.capabilities;
    renderWorkbench(root, controller.state, "start", false);
    expect(root.querySelector<HTMLButtonElement>('button[value="start"]')?.disabled).toBe(true);
  });
  it("suppresses duplicate clicks and keeps late disconnected results stale", async () => {
    const { controller, call } = await setup();
    let settle: (result: unknown) => void = () => undefined;
    call.mockImplementationOnce(() => new Promise(resolve => { settle = resolve; }));
    const change = { kind: "set_dependencies" as const, node_id: "task-b", dependencies: [] };
    const first = controller.proposeChange(change);
    expect(await controller.proposeChange(change)).toBe(false);
    expect(call).toHaveBeenCalledTimes(1);
    controller.setDisconnected("Disconnected during proposal");
    const result = planningGraph(); result.graph.revision = 7;
    result.proposal = { id: "saved", base_revision: 6, idempotency_key: "key", fingerprint: "hash", actor: "local_operator", subject: "subject", status: "proposed", applied_revision: null, change };
    settle({ structuredContent: result });
    expect(await first).toBe(false);
    expect(controller.state.graph?.revision).toBe(6);
    expect(controller.state.graphStale).toBe(true);
  });
  it("reuses a recovered proposal after an ambiguous response instead of writing another", async () => {
    const { controller, call } = await setup();
    const change = { kind: "set_dependencies" as const, node_id: "task-b", dependencies: [] };
    controller.state.proposal = { id: "saved", base_revision: 5, idempotency_key: "key", fingerprint: "hash", actor: "local_operator", subject: "subject", status: "proposed", applied_revision: null, change };
    expect(await controller.proposeChange(change)).toBe(true);
    expect(call).not.toHaveBeenCalled();
    expect(controller.state.notice).toContain("already saved");
  });
  it("builds messages from the current selected node and rejects stale or reconnected data", async () => {
    const { controller } = await setup();
    controller.selectNode("task-b");
    expect(controller.currentFollowUpPrompt("choose", "task-b")).toContain("Revision: 6\nTask ID: task-b");
    expect(controller.currentFollowUpPrompt("summary")).toContain("Run ID: dogfood-plan");
    controller.selectNode("task-a");
    expect(() => controller.currentFollowUpPrompt("choose", "task-b")).toThrow("stale");
    controller.setDisconnected("Disconnected"); controller.setConnected(true);
    expect(() => controller.currentFollowUpPrompt("summary")).toThrow("Refresh");
    await controller.select(null);
    expect(() => controller.currentFollowUpPrompt("summary")).toThrow("Refresh");
  });
  it("retains a historical no-op proposal without applying it", async () => {
    const { controller, call } = await setup();
    controller.state.proposal = { id: "legacy-noop", base_revision: 5, idempotency_key: "key", fingerprint: "hash", actor: "local_operator", subject: "subject", status: "proposed", applied_revision: null, change: { kind: "set_dependencies", node_id: "objective", dependencies: [] } };
    expect(await controller.applyChange(true)).toBe(false);
    expect(call).not.toHaveBeenCalled();
    expect(controller.state.proposal.id).toBe("legacy-noop");
    expect(controller.state.graph?.revision).toBe(6);
  });
});
