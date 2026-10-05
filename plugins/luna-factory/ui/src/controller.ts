import { boundedContext, parseToolResult, runId, settingsSchema, structuredResult, type Capabilities, type RunView, type Settings, type ToolData } from "./domain";

export interface Bridge {
  call(tool: string, args: Record<string, unknown>): Promise<unknown>;
  context(context: Record<string, unknown>): Promise<void>;
}
export type MutationTool = "start_factory" | "steer_factory_run" | "cancel_factory_run" | "resume_factory_run";
export interface ViewState {
  runs: RunView[]; capabilities: Capabilities | null; settings: Settings;
  selectedId: string | null; initialized: boolean; connected: boolean; refreshing: boolean;
  pending: { tool: string; runId: string | null } | null;
  error: string | null; notice: string | null; contextError: string | null;
}
export class WorkbenchController {
  readonly state: ViewState = { runs: [], capabilities: null, settings: {}, selectedId: null, initialized: false, connected: false, refreshing: false, pending: null, error: null, notice: null, contextError: null };
  private initialSeen = false;
  private readVersion = 0;
  private contextVersion = 0;
  private contextQueue = Promise.resolve();
  private lastContext = "";
  private readonly detailsLoaded = new Set<string>();
  constructor(private readonly bridge: Bridge, private readonly changed: () => void) {}
  get selected(): RunView | undefined { return this.state.runs.find(run => run.id === this.state.selectedId); }
  setConnected(connected: boolean): void { this.state.connected = connected; this.changed(); }
  reportError(message: string): void { this.state.error = message; this.changed(); }
  receiveInitial(result: unknown): void {
    if (this.initialSeen) return;
    try {
      const data = parseToolResult(result);
      this.initialSeen = true;
      this.apply(data, true);
      this.state.error = null;
    } catch (error) { this.state.error = errorMessage(error); }
    this.changed();
    this.syncContext();
  }
  private upsert(run: RunView): void {
    const previous = this.state.runs.find(item => item.id === run.id);
    if (previous && ((run.generation ?? 0) < (previous.generation ?? 0) || ((run.generation ?? 0) === (previous.generation ?? 0) && (run.updated_at ?? 0) < (previous.updated_at ?? 0)))) return;
    this.state.runs = previous ? this.state.runs.map(item => item.id === run.id ? run : item) : [run, ...this.state.runs];
  }
  private apply(data: ToolData, initial = false): void {
    if (data.kind === "run") {
      this.upsert(data.value);
      this.detailsLoaded.add(data.value.id);
      if (initial && this.state.selectedId === null) this.state.selectedId = data.value.id;
    } else {
      // Merge by durable ID so selected history is not lost when it falls off a recent-runs page.
      const retained = new Set(data.value.runs.map(run => run.id));
      if (this.state.selectedId) retained.add(this.state.selectedId);
      this.state.runs = this.state.runs.filter(run => retained.has(run.id));
      for (const run of data.value.runs) { this.upsert(run); this.detailsLoaded.delete(run.id); }
      if (data.value.selected_run) { this.upsert(data.value.selected_run); this.detailsLoaded.add(data.value.selected_run.id); }
      this.state.capabilities = data.value.capabilities;
      this.state.settings = data.value.settings;
      if (initial && this.state.selectedId === null && data.value.selected_run) this.state.selectedId = data.value.selected_run.id;
    }
    this.state.initialized = true;
  }
  async select(id: string | null): Promise<void> {
    if (id !== null && !runId.safeParse(id).success) { this.reportError("This run link is invalid"); return; }
    this.state.selectedId = id;
    this.state.error = null;
    const version = ++this.readVersion;
    this.state.refreshing = false;
    this.changed();
    this.syncContext();
    if (id === null || this.detailsLoaded.has(id) || this.state.pending) return;
    this.state.refreshing = true;
    this.changed();
    try {
      const data = parseToolResult(await this.bridge.call("get_factory_run", { run_id: id }));
      if (version !== this.readVersion) return;
      if (data.kind !== "run" || data.value.id !== id) throw new Error("The server returned a different run. Selection was preserved.");
      this.apply(data);
      this.syncContext();
    } catch (error) { if (version === this.readVersion) this.state.error = errorMessage(error); }
    finally { if (version === this.readVersion) { this.state.refreshing = false; this.changed(); } }
  }
  async refresh(): Promise<void> {
    if (this.state.pending) return;
    const version = ++this.readVersion;
    this.state.refreshing = true;
    this.state.error = null;
    this.changed();
    try {
      const args = this.state.selectedId ? { run_id: this.state.selectedId } : {};
      const data = parseToolResult(await this.bridge.call("refresh_factory", args));
      if (version !== this.readVersion) return;
      this.apply(data);
      this.syncContext();
    } catch (error) { if (version === this.readVersion) this.state.error = errorMessage(error); }
    finally { if (version === this.readVersion) { this.state.refreshing = false; this.changed(); } }
  }
  async sendOwnerInput(input: string): Promise<boolean> {
    const run = this.selected;
    const message = input.trim();
    if (!run) throw new Error("Select a run before answering the owner");
    if (!message || message.length > 4000) throw new Error("Enter an answer or correction of at most 4000 characters");
    if (run.pending_decision) {
      return this.mutate("resume_factory_run", { run_id: run.id, message, expected_decision_id: run.pending_decision.id });
    }
    if (run.state === "NEEDS_INPUT") throw new Error("This approval or input must be handled in native Codex. The workbench cannot approve it.");
    if (!["RUNNING", "VERIFYING"].includes(run.state) || !run.turn_id) throw new Error("No active owner turn. Refresh this run before steering.");
    return this.mutate("steer_factory_run", { run_id: run.id, expected_turn_id: run.turn_id, message });
  }
  async mutate(tool: MutationTool, args: Record<string, unknown>): Promise<boolean> {
    if (this.state.pending) return false;
    ++this.readVersion;
    this.state.refreshing = false;
    this.state.pending = { tool, runId: typeof args.run_id === "string" ? args.run_id : null };
    this.state.error = null;
    this.state.notice = null;
    this.changed();
    try {
      const data = parseToolResult(await this.bridge.call(tool, args));
      if (data.kind !== "run" || (typeof args.run_id === "string" && data.value.id !== args.run_id)) throw new Error("The server returned a different run. Refresh before retrying.");
      this.apply(data);
      if (tool === "start_factory") this.state.selectedId = data.value.id;
      if (tool === "resume_factory_run" && ["BLOCKED", "NEEDS_INPUT", "INTERRUPTED", "FAILED", "QUIESCENT"].includes(data.value.state)) {
        this.state.error = `Run did not resume. ${data.value.blocker || data.value.remaining_gap || "Check the current state before trying again."}`;
        this.syncContext();
        return false;
      }
      this.state.notice = tool === "cancel_factory_run" ? "Stop outcome received. The state and ownership below are the server's verified result." : tool === "resume_factory_run" ? "Resume outcome received. This run keeps its history and remaining limits." : tool === "steer_factory_run" ? "Correction delivered to the current owner turn." : "Run admitted. Follow its progress here.";
      this.syncContext();
      return true;
    } catch (error) {
      this.state.error = `${errorMessage(error)} Refresh before retrying; the action may have reached the server.`;
      return false;
    } finally { this.state.pending = null; this.changed(); }
  }
  async saveSettings(settings: Settings): Promise<boolean> {
    if (this.state.pending) return false;
    ++this.readVersion;
    this.state.refreshing = false;
    this.state.pending = { tool: "update_factory_settings", runId: null };
    this.state.error = null;
    this.changed();
    try {
      const result = structuredResult(await this.bridge.call("update_factory_settings", settings));
      // Settings readers return { schema, values, layout }; writers may return the values.
      const values = result !== null && typeof result === "object" && "values" in result ? result.values : result;
      this.state.settings = settingsSchema.parse(values);
      this.state.notice = "Defaults saved. Trusted server limits still apply.";
      return true;
    } catch (error) { this.state.error = errorMessage(error); return false; }
    finally { this.state.pending = null; this.changed(); }
  }
  private syncContext(): void {
    const context = this.selected ? boundedContext(this.selected) : {};
    const serialized = JSON.stringify(context);
    if (serialized === this.lastContext) return;
    this.lastContext = serialized;
    const version = ++this.contextVersion;
    // Serialize updates so a slow old context never wins after a new selection.
    this.contextQueue = this.contextQueue.then(async () => {
      if (version !== this.contextVersion) return;
      try { await this.bridge.context(context); this.state.contextError = null; }
      catch { this.lastContext = ""; this.state.contextError = "Chat context could not update. The run view still works."; }
      this.changed();
    });
  }
}
function errorMessage(error: unknown): string { return error instanceof Error ? error.message.slice(0, 800) : "The request could not be completed"; }
