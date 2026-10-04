import { describe, expect, it, vi } from "vitest";
import { App } from "@modelcontextprotocol/ext-apps";
import { AppBridge } from "@modelcontextprotocol/ext-apps/app-bridge";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { OpenAIExtensions } from "@openai/mcp-extensions/app";
import { boundedContext, parseRunLink, parseToolResult } from "../src/domain";
import { fixtureRun, fixtureWorkbench } from "./fixtures";

describe("official MCP Apps transport", () => {
  it("negotiates, receives the initial result, calls real tool names, and publishes bounded context", async () => {
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.1.0" }, {}, { autoResize: false });
    const host = new AppBridge(null, { name: "Fixture host", version: "0.1.0" }, { serverTools: {}, updateModelContext: { structuredContent: {} }, experimental: { "openai/modelContext": {} } });
    const extensions = new OpenAIExtensions(app);
    const calls: string[] = [];
    let initial: unknown;
    let context: unknown;
    host.oncalltool = async request => { calls.push(request.name); return { content: [], structuredContent: fixtureRun() }; };
    host.onupdatemodelcontext = async request => { context = request.structuredContent; return { _meta: { "openai/modelContext": { updateId: "test-update" } } }; };
    app.ontoolresult = result => { initial = result; };
    try {
      await host.connect(hostTransport);
      await app.connect(appTransport);
      await host.sendToolInput({ arguments: {} });
      await host.sendToolResult({ content: [], structuredContent: fixtureWorkbench });
      expect(parseToolResult(initial).kind).toBe("workbench");
      expect(calls).toEqual([]);
      const selected = await app.callServerTool({ name: "get_factory_run", arguments: { run_id: "run-123" } });
      expect(parseToolResult(selected).kind).toBe("run");
      const update = await extensions.modelContext?.update({ structuredContent: boundedContext(fixtureRun()) });
      expect(update?.updateId).toBe("test-update");
      expect(context).toEqual(boundedContext(fixtureRun()));
      host.setHostContext({ "openai/deepLink": { url: "/runs/run-123" } });
      await vi.waitFor(() => expect(parseRunLink(extensions.deepLink.getCurrent()?.url ?? "")).toBe("run-123"));
      expect(calls).toEqual(["get_factory_run"]);
    } finally { await app.close(); await host.close(); }
  });
});
