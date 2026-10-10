import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "@modelcontextprotocol/ext-apps";
import { AppBridge } from "@modelcontextprotocol/ext-apps/app-bridge";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { HostBridge } from "../src/bridge";
import { buildFollowUpPrompt } from "../src/domain";
import { fixtureRun } from "./fixtures";

afterEach(() => vi.restoreAllMocks());

describe("ChatGPT follow-up messages", () => {
  it("builds bounded run and task prompts without repository paths or owner credentials", () => {
    const run = fixtureRun({ objective: "Resolve one bounded review blocker", blocker: "Choose the scope" });
    const summary = buildFollowUpPrompt(run, "summary");
    const task = buildFollowUpPrompt(run, "choose", { id: "task-17", title: "Review the parser" }, 9);

    expect(summary).toContain(run.id);
    expect(summary).toContain(String(run.control?.revision));
    expect(summary).toContain("Summarize");
    expect(task).toContain("task-17");
    expect(task).toContain("Revision: 9");
    expect(task).not.toContain("/home/");
    expect(task).not.toContain(run.owner_thread ?? "impossible-owner-value");
    expect(task.length).toBeLessThanOrEqual(1800);
  });

  it("keeps the exact task identity when optional context is oversized", () => {
    const run = fixtureRun({ objective: "x".repeat(8000), blocker: "y".repeat(4000), delta: "z".repeat(4000) });
    const prompt = buildFollowUpPrompt(run, "choose", { id: "task-exact-42", title: "Review parser" }, 123);
    expect(prompt).toContain("Task ID: task-exact-42");
    expect(prompt).toContain("Revision: 123");
    expect(prompt.length).toBeLessThanOrEqual(1800);
  });

  it("sends only after an explicit call and targets the active thread", async () => {
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.2.1" }, {}, { autoResize: false });
    const sendMessage = vi.spyOn(app, "sendMessage");
    const host = new AppBridge(null, { name: "Synthetic basic host", version: "1" }, { message: { text: {} }, experimental: { "openai/message": {} } });
    const messages: Array<{ role: string; content: unknown[] }> = [];
    host.onmessage = async request => { messages.push({ role: request.role, content: request.content }); return {}; };
    await host.connect(hostTransport);
    const bridge = new HostBridge(app, () => undefined);
    await bridge.connect(appTransport);

    expect(messages).toEqual([]);
    expect(bridge.canSendFollowUp()).toBe(true);
    await bridge.sendFollowUp("Ask ChatGPT about run run-123, revision 4.");

    expect(messages).toHaveLength(1);
    const message = messages[0];
    if (!message) throw new Error("No ChatGPT message was sent");
    expect(message.role).toBe("user");
    expect(message.content).toEqual([{ type: "text", text: "Ask ChatGPT about run run-123, revision 4." }]);
    const sentParams = sendMessage.mock.calls[0]?.[0];
    if (!sentParams) throw new Error("The UI did not send an MCP Apps message");
    expect(sentParams).toEqual(expect.objectContaining({
      _meta: { "openai/message": { target: "active", send: true } },
    }));
  });

  it("degrades when the host does not advertise ui/message", async () => {
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.2.1" }, {}, { autoResize: false });
    const host = new AppBridge(null, { name: "No message host", version: "1" }, {});
    const onMessage = vi.fn(async () => ({}));
    host.onmessage = onMessage;
    await host.connect(hostTransport);
    const bridge = new HostBridge(app, () => undefined);
    await bridge.connect(appTransport);

    expect(bridge.canSendFollowUp()).toBe(false);
    await expect(bridge.sendFollowUp("Summarize run run-123.")).rejects.toThrow("does not support ChatGPT messages");
    expect(onMessage).not.toHaveBeenCalled();
  });

  it("disables messages and reports a real transport close", async () => {
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    const app = new App({ name: "Luna Factory test", version: "0.2.1" }, {}, { autoResize: false });
    const host = new AppBridge(null, { name: "Synthetic message host", version: "1" }, { message: { text: {} }, experimental: { "openai/message": {} } });
    await host.connect(hostTransport);
    const disconnected = vi.fn();
    const bridge = new HostBridge(app, () => undefined, disconnected);
    await bridge.connect(appTransport);
    expect(bridge.canSendFollowUp()).toBe(true);
    await app.close();
    expect(bridge.canSendFollowUp()).toBe(false);
    expect(disconnected).toHaveBeenCalled();
    await expect(bridge.sendFollowUp("not sent")).rejects.toThrow("not connected");
  });
});
