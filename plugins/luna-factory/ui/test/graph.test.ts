import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { graphEnvelopeSchema } from "../src/domain";
import { fixtureBackends, fixtureGraph, fixtureRun } from "./fixtures";
const catalog = { schema_version: 1, discovery: "configuration_only", policy: "subscription_only", targets: [] };
describe("planning graph controls", () => {
  it("creates a planning graph through the public tool without dispatching", async () => {
    const call = vi.fn<Bridge["call"]>().mockResolvedValue({ structuredContent: fixtureGraph() });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    expect(await controller.createGraph({ idempotency_key: "create-1" })).toBe(true);
    expect(call).toHaveBeenCalledExactlyOnceWith("create_factory_graph", { idempotency_key: "create-1" });
    expect(controller.state.graph?.planning_only).toBe(true);
    expect(controller.state.selectedId).toBe("run-123");
  });
  it("reviews a proposed change before applying at its returned revision", async () => {
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? catalog : fixtureGraph() }));
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureRun() });
    await controller.loadGraph();
    controller.selectNode("task-owner");
    const change = { kind: "set_dependencies" as const, node_id: "task-owner", dependencies: [] };
    const proposed = fixtureGraph(8);
    proposed.proposal = { id: "change-1", base_revision: 7, idempotency_key: "change-key", fingerprint: "hash", actor: "local_operator", subject: proposed.graph.repository.subject, change, status: "proposed", applied_revision: null };
    call.mockResolvedValueOnce({ structuredContent: proposed });
    expect(await controller.proposeChange(change)).toBe(true);
    expect(call).not.toHaveBeenCalledWith("apply_factory_change", expect.anything());
    const applied = fixtureGraph(9); applied.proposal = { ...proposed.proposal, status: "applied", applied_revision: 9 };
    call.mockResolvedValueOnce({ structuredContent: applied });
    expect(await controller.applyChange()).toBe(true);
    expect(call).toHaveBeenLastCalledWith("apply_factory_change", { run_id: "run-123", change_id: "change-1", expected_revision: 8 });
  });
  it("preserves a newer view but disables graph mutations after a stale read", async () => {
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? catalog : fixtureGraph(9) }));
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureRun() });
    await controller.loadGraph();
    call.mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? catalog : fixtureGraph(8) }));
    await controller.loadGraph();
    expect(controller.state.graph?.revision).toBe(9);
    expect(controller.state.graphStale).toBe(true);
    call.mockClear();
    expect(await controller.proposeChange({ kind: "set_dependencies", node_id: "task-owner", dependencies: [] })).toBe(false);
    expect(call).not.toHaveBeenCalled();
  });
  it("restores only a node verified by the graph read and publishes bounded node data", async () => {
    const graph = fixtureGraph();
    graph.graph.nodes.push({ ...graph.graph.nodes[0]!, id: "issue-61", title: "x".repeat(1000) });
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? catalog : tool === "get_factory_run" ? fixtureRun() : graph }));
    const context = vi.fn<Bridge["context"]>().mockResolvedValue();
    const controller = new WorkbenchController({ call, context }, () => undefined);
    await controller.restoreContext("run-123", "issue-61");
    expect(controller.state.selectedNodeId).toBe("issue-61");
    await vi.waitFor(() => expect(context).toHaveBeenLastCalledWith(expect.objectContaining({ node_id: "issue-61", node_title: "x".repeat(300), graph_revision: 7 })));
    await controller.restoreContext("run-123", "invented-node");
    expect(controller.state.selectedNodeId).toBe("issue-61");
  });
  it("keeps unsupported targets from becoming planning preferences", async () => {
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : fixtureGraph() }));
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureRun() }); await controller.loadGraph(); call.mockClear();
    expect(await controller.proposeChange({ kind: "set_target", node_id: "task-owner", target_id: "codex-cloud" })).toBe(false);
    expect(call).not.toHaveBeenCalled();
  });
  it("accepts public model-origin proposals at server bounds", () => {
    const envelope = fixtureGraph(8);
    envelope.proposal = { id: "proposal", idempotency_key: "k".repeat(200), fingerprint: "hash", actor: "local_operator", base_revision: 7, subject: "subject", status: "proposed", applied_revision: null, change: { kind: "import_candidates", nodes: [{ id: "n".repeat(200), title: "t".repeat(2000), criterion_ids: ["A1"], dependencies: [], source: { provider: "p".repeat(200), repository_id: "repo", item_id: "61", revision: "r" } }] } };
    expect(graphEnvelopeSchema.safeParse(envelope).success).toBe(true);
  });
  it("ignores a late graph read for a previously selected run", async () => {
    let resolve!: (value: unknown) => void;
    const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => tool === "get_factory_graph" ? new Promise(res => { resolve = res; }) : { structuredContent: catalog });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureRun() });
    const reading = controller.loadGraph();
    await controller.select(null);
    resolve({ structuredContent: fixtureGraph() }); await reading;
    expect(controller.state.graph).toBeNull();
    expect(controller.state.selectedId).toBeNull();
    expect(controller.state.graphLoading).toBe(false);
  });
  it("a delayed graph creation cannot override later user navigation", async () => {
    let resolve!: (value: unknown) => void;
    const call = vi.fn<Bridge["call"]>().mockImplementation(async () => new Promise(res => { resolve = res; }));
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    const creating = controller.createGraph({ idempotency_key: "k" });
    await controller.select(null);
    resolve({ structuredContent: fixtureGraph() });
    expect(await creating).toBe(true);
    expect(controller.state.selectedId).toBeNull();
    expect(controller.state.graph).toBeNull();
  });
});
