import { allowedFinishes, boundedContext, finishSchema, parseToolResult, repositoryDiscoverySchema, repositoryRegistrationSchema, runId, settingsSchema, structuredResult, type Capabilities, type RepositoryDiscovery, type RunView, type Settings, type ToolData } from "./domain";

export interface Bridge {
  call(tool: string, args: Record<string, unknown>): Promise<unknown>;
  context(context: Record<string, unknown>): Promise<void>;
}
export type MutationTool = "start_factory" | "steer_factory_run" | "cancel_factory_run" | "resume_factory_run" | "reconcile_factory_run";
export interface ViewState {
  runs: RunView[]; capabilities: Capabilities | null; settings: Settings;
  selectedId: string | null; initialized: boolean; connected: boolean; refreshing: boolean;
  pending: { tool: string; runId: string | null } | null;
  error: string | null; notice: string | null; contextError: string | null;
  discovery: RepositoryDiscovery | null; discovering: boolean;
}
export class WorkbenchController {
  readonly state: ViewState = { runs: [], capabilities: null, settings: {}, selectedId: null, initialized: false, connected: false, refreshing: false, pending: null, error: null, notice: null, contextError: null, discovery: null, discovering: false };
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
    const primary = run.presentation?.primary_action;
    if (!primary) throw new Error("The server has no available owner action. Refresh this run before trying again.");
    if (primary.allowed && primary.kind === "answer" && primary.tool === "resume_factory_run" && run.pending_decision) {
      return this.mutate("resume_factory_run", { run_id: run.id, message, expected_decision_id: run.pending_decision.id });
    }
    const availableSteer = run.presentation?.actions.find(action => action.kind === "steer" && action.tool === "steer_factory_run" && action.allowed);
    const steer = primary.kind === "steer" && primary.tool === "steer_factory_run" ? primary : ["wait", "refresh"].includes(primary.kind) ? availableSteer : undefined;
    if (steer?.allowed && steer.tool === "steer_factory_run" && run.presentation?.owner.turn_id) {
      return this.mutate("steer_factory_run", { run_id: run.id, expected_turn_id: run.presentation.owner.turn_id, message });
    }
    if (!primary.allowed) {
      if (run.state === "NEEDS_INPUT" && !run.pending_decision) throw new Error("This approval must be handled in native Codex. The workbench cannot approve it.");
      throw new Error("The server has no available owner action. Refresh this run before trying again.");
    }
    throw new Error("The server presentation does not authorize an owner answer or correction. Refresh before trying again.");
  }
  async mutate(tool: MutationTool, args: Record<string, unknown>): Promise<boolean> {
    if (this.state.pending) return false;
    const targetId = typeof args.run_id === "string" ? args.run_id : null;
    const targeted = tool !== "start_factory";
    const run = targeted && targetId !== null ? this.state.runs.find(item => item.id === targetId) : undefined;
    const presentation = run?.presentation;
    if (targeted && (!run || !presentation || !this.actionAllows(run, tool, args))) {
      this.state.error = "The server no longer authorizes this action. Refresh the run before trying again.";
      this.changed();
      return false;
    }
    const requestArgs = targeted && presentation ? { ...args, expected_revision: presentation.revision } : args;
    ++this.readVersion;
    this.state.refreshing = false;
    this.state.pending = { tool, runId: typeof args.run_id === "string" ? args.run_id : null };
    this.state.error = null;
    this.state.notice = null;
    this.changed();
    try {
      const data = parseToolResult(await this.bridge.call(tool, requestArgs));
      if (data.kind !== "run" || (targetId !== null && data.value.id !== targetId)) throw new Error("The server returned a different run. Refresh before retrying.");
      this.apply(data);
      if (tool === "start_factory") this.state.selectedId = data.value.id;
      if (tool === "resume_factory_run" && ["BLOCKED", "NEEDS_INPUT", "INTERRUPTED", "FAILED", "QUIESCENT"].includes(data.value.state)) {
        this.state.error = `Run did not resume. ${data.value.blocker || data.value.remaining_gap || "Check the current state before trying again."}`;
        this.syncContext();
        return false;
      }
      this.state.notice = tool === "cancel_factory_run" ? "Stop outcome received. The state and ownership below are the server's observed result." : tool === "resume_factory_run" ? "Resume outcome received. This run keeps its history and remaining limits." : tool === "steer_factory_run" ? "Correction delivered to the current owner turn." : tool === "reconcile_factory_run" ? "Existing work was reconciled. Current proof and ownership are shown below." : "Run admitted. Follow its progress here.";
      this.syncContext();
      return true;
    } catch (error) {
      if (targetId !== null) {
        const current = this.state.runs.find(item => item.id === targetId);
        if (current) current.presentation = undefined;
      }
      this.state.error = `${errorMessage(error)} Refresh to read the current state; the action may have reached the server.`;
      return false;
    } finally { this.state.pending = null; this.changed(); }
  }
  private actionAllows(run: RunView, tool: MutationTool, args: Record<string, unknown>): boolean {
    const runPresentation = run.presentation;
    if (!run.control || !runPresentation || run.control.revision !== runPresentation.revision) return false;
    const expectedKinds = tool === "resume_factory_run" ? ["resume", "answer"] : tool === "steer_factory_run" ? ["steer"] : tool === "cancel_factory_run" ? ["cancel"] : tool === "reconcile_factory_run" ? ["reconcile"] : [];
    const action = runPresentation.actions.find(item => item.tool === tool && expectedKinds.includes(item.kind));
    if (!action?.allowed) return false;
    const isPrimary = runPresentation.primary_action.kind === action.kind && runPresentation.primary_action.tool === action.tool && runPresentation.primary_action.allowed;
    const secondarySteer = action.kind === "steer" && ["wait", "refresh"].includes(runPresentation.primary_action.kind);
    if (!isPrimary && action.kind !== "cancel" && !secondarySteer) return false;
    if (tool === "resume_factory_run" && action.kind === "answer") return typeof args.expected_decision_id === "string" && args.expected_decision_id === run.pending_decision?.id && validOwnerMessage(args.message);
    if (tool === "resume_factory_run" && action.kind === "resume") return args.expected_decision_id === undefined && args.message === undefined;
    if (tool === "steer_factory_run") return args.expected_turn_id === runPresentation.owner.turn_id && args.expected_turn_id !== null && validOwnerMessage(args.message);
    return true;
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
  async discoverRepositories(): Promise<void> {
    if (this.state.discovering || this.state.pending) return;
    this.state.discovering = true;
    this.state.error = null;
    this.changed();
    try {
      const parsed = repositoryDiscoverySchema.safeParse(structuredResult(await this.bridge.call("discover_factory_repositories", {})));
      if (!parsed.success) throw new Error("The repository catalog is invalid. Refresh before retrying.");
      this.state.discovery = parsed.data;
      await this.refresh();
    } catch (error) { this.state.error = errorMessage(error); }
    finally { this.state.discovering = false; this.changed(); }
  }
  async requestRepository(fields: Record<string, string>): Promise<boolean> {
    if (this.state.pending || this.state.discovering) return false;
    const candidate = this.state.discovery?.candidates.find(item => item.id === fields.candidate_id);
    const finish = finishSchema.parse(fields.max_finish);
    if (!candidate || !allowedFinishes(candidate.max_finish).includes(finish)) throw new Error("Choose a discovered repository and permitted finish authority");
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(fields.alias ?? "")) throw new Error("Use 1 to 64 letters, digits, hyphens or underscores for the alias");
    this.state.pending = { tool: "request_factory_repository", runId: null };
    this.state.error = null;
    this.state.notice = null;
    this.changed();
    try {
      const parsed = repositoryRegistrationSchema.safeParse(structuredResult(await this.bridge.call("request_factory_repository", { candidate_id: candidate.id, alias: fields.alias, max_finish: finish })));
      if (!parsed.success) throw new Error("The repository request result is invalid. Refresh before retrying.");
      const request = parsed.data;
      if (request.alias !== fields.alias || request.root_alias !== candidate.root_alias || request.name !== candidate.name || request.max_finish !== finish) throw new Error("The server returned a different repository request. Refresh before retrying.");
      if (this.state.discovery) this.state.discovery.requests = [...this.state.discovery.requests.filter(item => item.id !== request.id), request];
      this.state.notice = request.status === "approved" ? "Repository already approved. Refresh repositories to use its alias." : "Access requested. A local operator must approve it before a run can start.";
      return true;
    } catch (error) { this.state.error = `${errorMessage(error)} Refresh before retrying; the request may have reached the server.`; return false; }
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
function validOwnerMessage(value: unknown): value is string { return typeof value === "string" && value.trim().length > 0 && value.length <= 4000; }
