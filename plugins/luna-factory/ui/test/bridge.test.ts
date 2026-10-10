import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "@modelcontextprotocol/ext-apps";
import { AppBridge } from "@modelcontextprotocol/ext-apps/app-bridge";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { applyDeepLink, HostBridge } from "../src/bridge";
import { WorkbenchController } from "../src/controller";
import { fixtureBackends, fixtureGraph, fixtureRun } from "./fixtures";

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => { await Promise.all(cleanup.splice(0).map(close => close())); });
async function setup(mode: "structured" | "text" | "none" = "structured", initial?: unknown, openai = true) {
  const app = new App({ name: "Factory", version: "test" }, {}, { autoResize: false });
  const host = new AppBridge(null, { name: "Fixture", version: "test" }, {
    serverTools: {}, ...(mode === "none" ? {} : { updateModelContext: mode === "text" ? { text: {} } : { structuredContent: {} } }),
    ...(openai ? { experimental: { "openai/modelContext": {} } } : {}),
  }, initial === undefined ? {} : { hostContext: { "openai/modelContext": initial } });
  const updates: unknown[] = [];
  host.onupdatemodelcontext = async params => { updates.push(params); return { _meta: { "openai/modelContext": { updateId: `ack-${updates.length}` } } }; };
  const selected = vi.fn<(id: string, nodeId?: string, revision?: number) => void>();
  const bridge = new HostBridge(app, selected);
  const [a, b] = InMemoryTransport.createLinkedPair();
  await host.connect(b);
  await bridge.connect(a);
  cleanup.push(async () => { await app.close(); await host.close(); });
  return { app, host, bridge, updates, selected };
}
describe("production host bridge", () => {
  it.each([true, false])("negotiates text-only support and clears without an attachment (OpenAI %s)", async openai => {
    const { bridge, updates } = await setup("text", undefined, openai);
    await bridge.context({ run_id: "r1" });
    expect(updates[0]).toEqual({ content: [{ type: "text", text: '{"run_id":"r1"}' }] });
    await bridge.context({});
    expect(updates[1]).toEqual({ content: [] });
  });
  it("restores current context through getCurrent, including text fallback", async () => {
    const { selected } = await setup("text", { updateId: "restore-1", content: [{ type: "text", text: '{"run_id":"r1"}' }] });
    expect(selected).toHaveBeenCalledExactlyOnceWith("r1");
  });
  it("restores bounded node identity without trusting its title or authority", async () => {
    const { selected } = await setup("structured", { updateId: "restore-node", structuredContent: { run_id: "r1", node_id: "issue-61", graph_revision: 7, node_title: "untrusted", finish: "pr" } });
    expect(selected).toHaveBeenCalledExactlyOnceWith("r1", "issue-61", 7);
  });
  it.each([undefined, { updateId: "", structuredContent: { run_id: "r1" } }, { updateId: "x", structuredContent: { run_id: "../../elsewhere" } }])("ignores absent or malformed initial context", async initial => {
    const { selected } = await setup("structured", initial);
    expect(selected).not.toHaveBeenCalled();
  });
  it("respects an explicit null on remount", async () => {
    const { bridge, updates } = await setup("structured", null);
    await bridge.context({ run_id: "r1" });
    expect(bridge.contextCleared).toBe(true);
    expect(updates).toEqual([]);
  });
  it("a restored deep link preserves the host's explicit context removal", async () => {
    const { bridge, host, updates } = await setup("structured", null);
    const controller = new WorkbenchController(bridge, () => undefined);
    host.oncalltool = async () => ({ content: [], structuredContent: fixtureRun() });
    host.setHostContext({ "openai/deepLink": { url: "/runs/run-123" } });
    await vi.waitFor(() => expect(bridge.extensions.deepLink.getCurrent()?.url).toBe("/runs/run-123"));
    await applyDeepLink(controller, bridge.extensions.deepLink.getCurrent()!.url);
    expect(controller.state.selectedId).toBe("run-123");
    expect(bridge.contextCleared).toBe(true);
    expect(updates).toEqual([]);
    await controller.select("run-123");
    await vi.waitFor(() => expect(updates).toHaveLength(1));
  });
  it("restores an exact task deep link through the revision-fenced graph", async () => {
    const { bridge, host } = await setup("structured");
    const controller = new WorkbenchController(bridge, () => undefined);
    host.oncalltool = async request => request.name === "get_factory_graph"
      ? { content: [], structuredContent: { graph: { ...fixtureGraph().graph, run_id: "run-123", revision: 7, nodes: [...fixtureGraph().graph.nodes, { ...fixtureGraph().graph.nodes[0]!, id: "issue-61" }] }, proposal: null } }
      : { content: [], structuredContent: fixtureRun() };
    controller.setConnected(true);
    await applyDeepLink(controller, "/runs/run-123?task=issue-61&revision=7");
    expect(controller.state.selectedId).toBe("run-123");
    expect(controller.state.selectedNodeId).toBe("issue-61");
    expect(controller.state.graph?.revision).toBe(7);
  });
  it("refuses a task deep link when the graph revision has moved", async () => {
    const { bridge, host } = await setup("structured");
    const controller = new WorkbenchController(bridge, () => undefined);
    host.oncalltool = async request => request.name === "get_factory_graph"
      ? { content: [], structuredContent: { graph: { ...fixtureGraph().graph, run_id: "run-123", revision: 7, nodes: [...fixtureGraph().graph.nodes, { ...fixtureGraph().graph.nodes[0]!, id: "issue-61" }] }, proposal: null } }
      : request.name === "get_factory_backends"
        ? { content: [], structuredContent: fixtureBackends }
        : { content: [], structuredContent: fixtureRun() };
    controller.setConnected(true);
    await applyDeepLink(controller, "/runs/run-123?task=issue-61&revision=8");
    expect(controller.state.selectedId).toBe("run-123");
    expect(controller.state.selectedNodeId).not.toBe("issue-61");
    expect(controller.state.error).toContain("task link is stale");
  });
  it("a host clear during graph creation is not undone by its delayed result", async () => {
    const { bridge, host, updates } = await setup();
    let release!: () => void;
    host.oncalltool = async () => { await new Promise<void>(resolve => { release = resolve; }); return { content: [], structuredContent: fixtureGraph() }; };
    const controller = new WorkbenchController(bridge, () => undefined);
    controller.setConnected(true);
    const creating = controller.createGraph({ idempotency_key: "create" });
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    host.setHostContext({ "openai/modelContext": null });
    await vi.waitFor(() => expect(bridge.contextCleared).toBe(true));
    release(); expect(await creating).toBe(true);
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(bridge.contextCleared).toBe(true);
    expect(updates.every(update => JSON.stringify(update) === JSON.stringify({ structuredContent: {} }))).toBe(true);
  });
  it("keeps clear suppressed through refresh; explicit selection can reattach", async () => {
    const { bridge, host, updates } = await setup();
    await bridge.context({ run_id: "r1" });
    host.setHostContext({ "openai/modelContext": null });
    await vi.waitFor(() => expect(bridge.contextCleared).toBe(true));
    await bridge.context({ run_id: "r1", state: "BLOCKED" });
    expect(updates).toHaveLength(1);
    host.setHostContext({ theme: "dark" });
    bridge.selectContext();
    await bridge.context({ run_id: "r1" });
    expect(updates).toHaveLength(2);
  });
  it("recognizes own update IDs and ignores unrelated context changes", async () => {
    const { bridge, host, selected } = await setup();
    await bridge.context({ run_id: "r1" });
    host.setHostContext({ "openai/modelContext": { updateId: "ack-1", structuredContent: { run_id: "r1" } } });
    host.setHostContext({ theme: "dark" });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(selected).not.toHaveBeenCalled();
    host.setHostContext({ "openai/modelContext": { updateId: "external-1", structuredContent: { run_id: "r2" } } });
    await vi.waitFor(() => expect(selected).toHaveBeenCalledExactlyOnceWith("r2"));
  });
  it("keeps host context independent across two instances", async () => {
    const a = await setup(); const b = await setup();
    await a.bridge.context({ run_id: "a" }); await b.bridge.context({ run_id: "b" });
    a.host.setHostContext({ "openai/modelContext": null });
    await vi.waitFor(() => expect(a.bridge.contextCleared).toBe(true));
    expect(b.bridge.contextCleared).toBe(false);
    expect(b.updates).toEqual([{ structuredContent: { run_id: "b" } }]);
  });
  it("reports unsupported context without pretending to acknowledge it", async () => {
    const { bridge, updates } = await setup("none");
    await expect(bridge.context({ run_id: "r1" })).rejects.toThrow("does not support");
    expect(updates).toEqual([]);
  });
  it.each(["structured", "text"] as const)("an outstanding %s Overview clear cannot suppress a newer queued selection", async mode => {
    const { bridge, host } = await setup(mode);
    let releaseClear!: () => void;
    const requests: Parameters<NonNullable<typeof host.onupdatemodelcontext>>[0][] = [];
    host.onupdatemodelcontext = async params => {
      requests.push(params);
      if (requests.length === 2) await new Promise<void>(resolve => { releaseClear = resolve; });
      return { _meta: { "openai/modelContext": { updateId: `opaque-${requests.length}` } } };
    };
    bridge.selectContext(); await bridge.context({ run_id: "r1" });
    bridge.selectContext(); const clearing = bridge.context({});
    await vi.waitFor(() => expect(releaseClear).toBeTypeOf("function"));
    bridge.selectContext(); const selecting = bridge.context({ run_id: "r2" });
    host.setHostContext({ "openai/modelContext": null });
    await new Promise(resolve => setTimeout(resolve, 0));
    // The old clear is still outstanding; the newer attachment is not on the host yet.
    const suppressed = bridge.contextCleared;
    releaseClear(); await clearing; await selecting;
    expect(suppressed).toBe(false);
    expect(requests).toHaveLength(3);
    expect(requests[2]).toEqual(mode === "structured" ? { structuredContent: { run_id: "r2" } } : { content: [{ type: "text", text: '{"run_id":"r2"}' }] });
  });
  it("a second null while clearing still suppresses the queued selection", async () => {
    const { bridge, host } = await setup();
    let releaseClear!: () => void;
    const requests: unknown[] = [];
    host.onupdatemodelcontext = async params => {
      requests.push(params);
      if (requests.length === 1) await new Promise<void>(resolve => { releaseClear = resolve; });
      return { _meta: { "openai/modelContext": { updateId: "opaque-clear" } } };
    };
    const clearing = bridge.context({});
    await vi.waitFor(() => expect(releaseClear).toBeTypeOf("function"));
    bridge.selectContext(); const selecting = bridge.context({ run_id: "r2" });
    host.setHostContext({ "openai/modelContext": null });
    await new Promise(resolve => setTimeout(resolve, 0));
    // setHostContext coalesces equal state; emit the second wire notification explicitly.
    await host.sendHostContextChange({ "openai/modelContext": null });
    await new Promise(resolve => setTimeout(resolve, 0));
    const suppressed = bridge.contextCleared;
    releaseClear(); await clearing; await selecting;
    expect(suppressed).toBe(true);
    expect(requests).toEqual([{ structuredContent: {} }]);
    await bridge.context({ run_id: "r2", state: "BLOCKED" });
    expect(requests).toHaveLength(1);
  });
  it("a null after clear acknowledgment is removal, not a retained echo token", async () => {
    const { bridge, host, updates } = await setup();
    await bridge.context({});
    bridge.selectContext(); await bridge.context({ run_id: "r2" });
    host.setHostContext({ "openai/modelContext": null });
    await vi.waitFor(() => expect(bridge.contextCleared).toBe(true));
    await bridge.context({ run_id: "r2", state: "BLOCKED" });
    expect(updates).toEqual([{ structuredContent: {} }, { structuredContent: { run_id: "r2" } }]);
  });
  it("a clear in flight without newer user navigation does not exempt a removal", async () => {
    const { bridge, host } = await setup();
    let releaseClear!: () => void;
    host.onupdatemodelcontext = async () => {
      await new Promise<void>(resolve => { releaseClear = resolve; });
      return {};
    };
    const clearing = bridge.context({});
    await vi.waitFor(() => expect(releaseClear).toBeTypeOf("function"));
    host.setHostContext({ "openai/modelContext": null });
    await new Promise(resolve => setTimeout(resolve, 0));
    const suppressed = bridge.contextCleared;
    releaseClear(); await clearing;
    expect(suppressed).toBe(true);
  });
  it.each([false, true])("fences host clear against delayed acknowledgment (new selection %s)", async reselect => {
    const { bridge, host } = await setup();
    let release: (() => void) | undefined;
    const requests: unknown[] = [];
    host.onupdatemodelcontext = async params => {
      requests.push(params);
      if (requests.length === 1) await new Promise<void>(resolve => { release = resolve; });
      return { _meta: { "openai/modelContext": { updateId: `slow-${requests.length}` } } };
    };
    const first = bridge.context({ run_id: "r1" });
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    host.setHostContext({ "openai/modelContext": null });
    await vi.waitFor(() => expect(bridge.contextCleared).toBe(true));
    // Passive refresh cannot revoke the pending compensating clear.
    await bridge.context({ run_id: "r1", state: "BLOCKED" });
    let next: Promise<void> | undefined;
    if (reselect) { bridge.selectContext(); next = bridge.context({ run_id: "r2" }); }
    release!(); await first; await next;
    await vi.waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[1]).toEqual({ structuredContent: reselect ? { run_id: "r2" } : {} });
    expect(bridge.contextCleared).toBe(!reselect);
  });
});
