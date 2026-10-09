import type { App } from "@modelcontextprotocol/ext-apps";
import { OpenAIExtensions, type OpenAIModelContextHostState } from "@openai/mcp-extensions/app";
import { nodeId, parseRunLink, runId } from "./domain";
import type { Bridge, WorkbenchController } from "./controller";

type ContextWrite =
  | { kind: "clear"; selectionEpoch: number; notificationSeen: boolean }
  | { kind: "publish"; payload: string };

/** Host routing restores a view; it does not override a removed attachment. */
export async function applyDeepLink(controller: Pick<WorkbenchController, "select" | "reportError">, url: string): Promise<void> {
  const id = url === "/" ? null : parseRunLink(url);
  if (url !== "/" && id === null) { controller.reportError("This exact-run link is invalid"); return; }
  await controller.select(id, false);
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

  constructor(private readonly app: App, private readonly restoreSelection: (id: string, nodeId?: string) => void) {
    this.extensions = new OpenAIExtensions(app);
    app.addEventListener("hostcontextchanged", patch => {
      if (Object.hasOwn(patch, "openai/modelContext")) this.receiveContext();
    });
  }
  async connect(transport?: Parameters<App["connect"]>[0]): Promise<void> {
    this.connection = (async () => {
      await this.app.connect(transport);
      this.receiveContext();
    })();
    await this.connection;
  }
  async call(tool: string, args: Record<string, unknown>): Promise<unknown> {
    if (!this.connection) throw new Error("The host is not connected");
    await this.connection;
    if (!this.app.getHostCapabilities()?.serverTools) throw new Error("This host does not support app tool calls");
    return this.app.callServerTool({ name: tool, arguments: args });
  }
  /** Only explicit navigation/reattachment overrides a user's context removal. */
  selectContext(): void { this.contextCleared = false; ++this.epoch; ++this.selectionEpoch; }
  async context(context: Record<string, unknown>): Promise<void> {
    if (this.contextCleared) return;
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
    const params = support.structuredContent ? { structuredContent: context } : { content: empty ? [] : [{ type: "text" as const, text: JSON.stringify(context) }] };
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
    if (node.success) this.restoreSelection(id.data, node.data);
    else this.restoreSelection(id.data);
  }
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
