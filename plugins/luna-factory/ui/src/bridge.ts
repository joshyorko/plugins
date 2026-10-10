import type { App } from "@modelcontextprotocol/ext-apps";
import { OpenAIExtensions, type OpenAIModelContextHostState } from "@openai/mcp-extensions/app";
import { nodeId, parseFactoryLink, runId } from "./domain";
import type { Bridge, WorkbenchController } from "./controller";

type ContextWrite =
  | { kind: "clear"; selectionEpoch: number; notificationSeen: boolean }
  | { kind: "publish"; payload: string };
type HostContext = NonNullable<ReturnType<App["getHostContext"]>>;
type SurfaceContext = Pick<HostContext, "displayMode"> & {
  toolInfo?: { tool: Pick<NonNullable<HostContext["toolInfo"]>["tool"], "name"> };
};

export function surfaceFromHostContext(context: SurfaceContext | undefined): "inline" | "global" | "thread" {
  const entrypoint = context?.toolInfo?.tool.name;
  if (entrypoint === "open_factory_panel") return "thread";
  if (entrypoint === "open_factory") return "global";
  return context?.displayMode === "inline" ? "inline" : "global";
}

/** Host routing restores a view; it does not override a removed attachment. */
export async function applyDeepLink(controller: Pick<WorkbenchController, "select" | "restoreContext" | "reportError">, url: string): Promise<void> {
  if (url === "/") { await controller.select(null, false); return; }
  const link = parseFactoryLink(url);
  if (!link) { controller.reportError("This exact run or task link is invalid"); return; }
  if (link.taskId && link.revision !== undefined) await controller.restoreContext(link.runId, link.taskId, link.revision);
  else await controller.select(link.runId, false);
}

/** One bridge per mounted App; context and selection never cross instances. */
export class HostBridge implements Bridge {
  readonly extensions: OpenAIExtensions;
  contextCleared = false;
  private connection: Promise<void> | null = null;
  private queue = Promise.resolve();
  private epoch = 0;
  private selectionEpoch = 0;
  private inFlight: ContextWrite | null = null;
  private lastHostId: string | null = null;
  private readonly ownIds = new Set<string>();
  private connected = false;

  constructor(private readonly app: App, private readonly restoreSelection: (id: string, nodeId?: string, revision?: number) => void, private readonly onDisconnected?: (message: string) => void) {
    this.extensions = new OpenAIExtensions(app);
    app.onclose = () => {
      this.connected = false;
      this.connection = null;
      this.onDisconnected?.("The MCP Apps host disconnected. Reopen Luna Factory to refresh current state.");
    };
    app.onerror = () => {
      this.connected = false;
      this.connection = null;
      this.onDisconnected?.("The MCP Apps connection failed. Reopen Luna Factory to refresh current state.");
    };
    app.addEventListener("hostcontextchanged", patch => {
      if (Object.hasOwn(patch, "openai/modelContext")) this.receiveContext();
    });
  }
  async connect(transport?: Parameters<App["connect"]>[0]): Promise<void> {
    this.connection = (async () => {
      await this.app.connect(transport);
      this.connected = true;
      this.receiveContext();
    })();
    await this.connection;
  }
  async call(tool: string, args: Record<string, unknown>): Promise<unknown> {
    if (!this.connection || !this.connected) throw new Error("The host is not connected");
    await this.connection;
    if (!this.app.getHostCapabilities()?.serverTools) throw new Error("This host does not support app tool calls");
    return this.app.callServerTool({ name: tool, arguments: args });
  }
  canSendFollowUp(): boolean {
    return Boolean(this.connection && this.connected && this.extensions.message && this.app.getHostCapabilities()?.message?.text);
  }
  async sendFollowUp(prompt: string): Promise<void> {
    if (!this.connection || !this.connected) throw new Error("The host is not connected");
    await this.connection;
    const messages = this.extensions.message;
    if (!messages || !this.app.getHostCapabilities()?.message?.text) {
      throw new Error("This host does not support ChatGPT messages");
    }
    const result = await messages.send({
      role: "user",
      content: [{ type: "text", text: prompt }],
      _meta: { "openai/message": { target: "active", send: true } },
    });
    if (result.isError) throw new Error("ChatGPT could not receive this message");
  }
  /** Only explicit navigation/reattachment overrides a user's context removal. */
  selectContext(): void { this.contextCleared = false; ++this.epoch; ++this.selectionEpoch; }
  async context(context: Record<string, unknown>): Promise<void> {
    if (this.contextCleared || !this.connected) return;
    const epoch = ++this.epoch;
    if (!this.connection) return;
    await this.connection;
    if (this.contextCleared || epoch !== this.epoch) return;
    const task = this.queue.then(async () => {
      if (this.contextCleared || epoch !== this.epoch) return;
      await this.publish(context);
    });
    this.queue = task.catch(() => undefined);
    await task;
  }
  private async publish(context: Record<string, unknown>): Promise<void> {
    const support = this.app.getHostCapabilities()?.updateModelContext;
    if (!support?.structuredContent && !support?.text) throw new Error("This host does not support model context updates");
    const empty = Object.keys(context).length === 0;
    const title = empty ? null : contextTitle(context);
    // Titles label the host's context chip; content-block _meta is excluded from model input.
    const titled = title ? { _meta: { "openai/title": title } } : {};
    const params = support.structuredContent
      ? { structuredContent: context, ...(title && support.text ? { content: [{ type: "text" as const, text: title, ...titled }] } : {}) }
      : { content: empty ? [] : [{ type: "text" as const, text: JSON.stringify(context), ...titled }] };
    this.inFlight = empty
      ? { kind: "clear", selectionEpoch: this.selectionEpoch, notificationSeen: false }
      : { kind: "publish", payload: JSON.stringify(context) };
    try {
      const helper = this.extensions.modelContext;
      if (helper) {
        const ack = await helper.update(params);
        if (ack) {
          this.ownIds.add(ack.updateId);
          // A bounded echo cache, scoped to this mount only.
          if (this.ownIds.size > 64) this.ownIds.delete(this.ownIds.values().next().value!);
        }
      } else await this.app.updateModelContext(params);
    } finally { this.inFlight = null; }
  }
  private receiveContext(): void {
    const current = this.extensions.modelContext?.getCurrent();
    if (current === undefined) return;
    if (current === null) {
      const pending = this.inFlight;
      if (pending?.kind === "clear" && !pending.notificationSeen) {
        pending.notificationSeen = true;
        // Only one null can be attributed to this still-outstanding clear.
        // Serialization guarantees the newer explicit selection is not attached yet.
        if (!this.contextCleared && this.selectionEpoch > pending.selectionEpoch) return;
      }
      // Bare null carries no updateId. After acknowledgment, or after the first
      // notification, there is no safe echo correlation: honor it as removal.
      this.contextCleared = true;
      const clearEpoch = ++this.epoch;
      // An already-sent update can reach the host after its clear notification.
      // Settle that request, then clear this instance again; never reattach it.
      if (pending?.kind === "publish") {
        this.queue = this.queue.then(async () => {
          if (this.contextCleared && this.epoch === clearEpoch) await this.publish({});
        }).catch(() => undefined);
      }
      return;
    }
    if (this.ownIds.has(current.updateId) || current.updateId === this.lastHostId) return;
    this.lastHostId = current.updateId;
    const payload = contextData(current);
    if (this.inFlight?.kind === "publish" && JSON.stringify(payload) === this.inFlight.payload) return;
    const id = runId.safeParse(payload?.run_id);
    if (!id.success) return;
    ++this.epoch;
    this.contextCleared = false;
    const node = nodeId.safeParse(payload?.node_id);
    const revision = payload?.graph_revision;
    if (node.success && typeof revision === "number" && Number.isSafeInteger(revision) && revision >= 0) this.restoreSelection(id.data, node.data, revision);
    else this.restoreSelection(id.data);
  }
}
/** A short human label for the attached selection, built only from bounded context fields. */
export function contextTitle(context: Record<string, unknown>): string | null {
  const text = (key: string) => typeof context[key] === "string" && context[key] ? String(context[key]) : null;
  const clip = (value: string, length: number) => value.length > length ? `${value.slice(0, length - 1)}…` : value;
  const subject = text("node_title") ?? text("objective");
  const parts = [text("agent_label"), subject ? clip(subject, 48) : null].filter((part): part is string => part !== null);
  if (!parts.length) return text("repository") ? `Luna Factory · ${text("repository")}` : null;
  return clip(`${parts.join(" · ")}`, 72);
}
function contextData(state: Exclude<OpenAIModelContextHostState, null>): Record<string, unknown> | undefined {
  if (state.structuredContent) return state.structuredContent;
  for (const block of state.content ?? []) {
    if (block.type !== "text" || block.text.length > 6000) continue;
    try {
      const value: unknown = JSON.parse(block.text);
      if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
    } catch { /* Unrelated host text is not app selection state. */ }
  }
  return undefined;
}
