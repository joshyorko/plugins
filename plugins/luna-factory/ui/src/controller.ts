import { allowedFinishes, boundedContext, buildFollowUpPrompt, finishSchema, parseToolResult, repositoryDiscoverySchema, repositoryRegistrationSchema, runId, settingsSchema, structuredResult, type Capabilities, type FollowUpKind, type RepositoryDiscovery, type RunView, type Settings, type ToolData } from "./domain";
import { backendCatalogSchema, graphChangeSchema, graphEnvelopeSchema, type BackendCatalog, type FactoryGraph, type GraphChange, type GraphEnvelope } from "./domain";
import { campaignListSchema, type CampaignView } from "./domain";
import { agentTimelineSchema, type AgentTimelineView } from "./domain";
import { deliverySchema, type Delivery } from "./domain";
import { githubConnectSchema, githubRepositoryListSchema, localCreateSchema, localRepositoryName, repositoryReasonCopy, type GithubRepositoryList } from "./domain";
import { roster } from "./agents";

export interface Bridge {
  call(tool: string, args: Record<string, unknown>): Promise<unknown>;
  context(context: Record<string, unknown>): Promise<void>;
  selectContext?(): void;
}
export type MutationTool = "start_factory" | "steer_factory_run" | "cancel_factory_run" | "resume_factory_run" | "reconcile_factory_run";
export interface ViewState {
  runs: RunView[]; capabilities: Capabilities | null; settings: Settings;
  connectionStatus: "connecting" | "connected" | "disconnected";
  selectedId: string | null; initialized: boolean; connected: boolean; refreshing: boolean;
  pending: { tool: string; runId: string | null } | null;
  error: string | null; notice: string | null; contextError: string | null;
  discovery: RepositoryDiscovery | null; discovering: boolean;
  graph: FactoryGraph | null; graphLoading: boolean; graphStale: boolean; selectedNodeId: string | null;
  proposal: GraphEnvelope["proposal"]; backends: BackendCatalog | null;
  /** View-only selection shared by Map, Lanes and the inspector. Never mutates the plan. */
  viewMode: "map" | "lanes"; selectedAgent: string | null;
  /** Last valid `list_factory_campaigns` read; null when the server has not provided one. */
  campaigns: CampaignView[] | null; campaignsStale: boolean;
  /** Records the last valid read skipped as unreadable. Counted only; never rendered as campaigns. */
  campaignsSkipped: number;
  /** "Observed in Codex" per agent thread of `timelineRunId`. Display only: never proof, attention or plan state. */
  timelines: Record<string, AgentTimelineView>; timelineRunId: string | null;
  /** GitHub-reported telemetry (`read_factory_delivery`). Display only: never proof, attention or an action. */
  delivery: Delivery | null;
  /** GitHub repos the App installation can see, for the plan form's picker. Display only. */
  githubRepos: GithubRepositoryList | null; githubLoading: boolean;
  /** The plan form's chosen approved alias. View-only; the server rechecks it on submit. */
  pickedRepository: string | null;
  /** The plan form's "+ New repo…" choice: the repo is created only when Plan it is pressed. View-only. */
  newRepository: boolean;
}
/** Each agent's observed timeline is read at most once per interval, a few agents at a time. */
export const TIMELINE_INTERVAL_MS = 25_000;
export const MAX_TIMELINE_AGENTS = 8;
/** Matches the server's per-node minimum interval; the app never polls GitHub faster. */
export const DELIVERY_INTERVAL_MS = 60_000;
export class WorkbenchController {
  readonly state: ViewState = { runs: [], capabilities: null, settings: {}, connectionStatus: "connecting", selectedId: null, initialized: false, connected: false, refreshing: false, pending: null, error: null, notice: null, contextError: null, discovery: null, discovering: false, graph: null, graphLoading: false, graphStale: false, selectedNodeId: null, proposal: null, backends: null, viewMode: "map", selectedAgent: null, campaigns: null, campaignsStale: false, campaignsSkipped: 0, timelines: {}, timelineRunId: null, delivery: null, githubRepos: null, githubLoading: false, pickedRepository: null, newRepository: false };
  private initialSeen = false;
  private readVersion = 0;
  private contextVersion = 0;
  private contextQueue = Promise.resolve();
  private lastContext = "";
  private graphVersion = 0;
  private campaignVersion = 0;
  private selectionEpoch = 0;
  private contextRestoreEpoch: number | null = null;
  private changeRetry: { fingerprint: string; key: string } | null = null;
  /** Applied changes whose "Plan updated" note the user dismissed. Display state only. */
  private readonly dismissedChanges = new Set<string>();
  private readonly detailsLoaded = new Set<string>();
  private timelineVersion = 0;
  private timelineLoading = false;
  private readonly timelineReadAt = new Map<string, number>();
  private deliveryReadAt: number | null = null;
  private githubVersion = 0;
  private readonly repositoryKeys = new Map<string, string>();
  /** New-repo names created for a plan that has not been saved yet, so a plan retry skips the create. */
  private readonly createdRepositories = new Map<string, string>();
  constructor(private readonly bridge: Bridge, private readonly changed: () => void) {}
  get selected(): RunView | undefined { return this.state.runs.find(run => run.id === this.state.selectedId); }
  setConnected(connected: boolean): void { this.state.connected = connected; this.state.connectionStatus = connected ? "connected" : "connecting"; this.lastContext = ""; this.syncContext(); this.changed(); }
  setDisconnected(message: string): void {
    ++this.readVersion; ++this.graphVersion; ++this.campaignVersion; ++this.timelineVersion; this.timelineLoading = false;
    this.state.connected = false; this.state.connectionStatus = "disconnected"; this.state.error = message;
    this.state.campaignsStale = this.state.campaigns !== null;
    this.state.refreshing = false; this.state.graphLoading = false; this.state.graphStale = this.state.graph !== null;
    this.detailsLoaded.clear(); this.lastContext = ""; this.syncContext(); this.changed();
  }
  reportError(message: string): void { this.state.error = message; this.changed(); }
  reportNotice(message: string): void { this.state.error = null; this.state.notice = message; this.changed(); }
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
    if (previous && (previous.control
      ? !run.control || run.control.revision < previous.control.revision
      : (run.generation ?? 0) < (previous.generation ?? 0) || ((run.generation ?? 0) === (previous.generation ?? 0) && (run.updated_at ?? 0) < (previous.updated_at ?? 0)))) return;
    this.state.runs = previous ? this.state.runs.map(item => item.id === run.id ? run : item) : [run, ...this.state.runs];
    if (this.state.graph?.run_id === run.id && run.control && run.control.revision > this.state.graph.revision) this.state.graphStale = true;
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
  async select(id: string | null, explicit = true): Promise<void> {
    if (id !== null && !runId.safeParse(id).success) { this.reportError("This run link is invalid"); return; }
    // All navigation supersedes pending restoration/creation; only a user gesture reattaches context.
    ++this.selectionEpoch;
    if (this.contextRestoreEpoch !== this.selectionEpoch) this.contextRestoreEpoch = null;
    if (explicit) { this.bridge.selectContext?.(); this.lastContext = ""; }
    if (id !== this.state.selectedId) {
      ++this.graphVersion;
      this.state.graph = null; this.state.proposal = null; this.state.selectedNodeId = null; this.state.graphLoading = false;
      this.state.graphStale = false; this.state.backends = null; this.changeRetry = null; this.state.selectedAgent = null;
      ++this.timelineVersion; this.timelineLoading = false; this.state.timelines = {}; this.state.timelineRunId = null;
      this.resetDelivery();
    }
    this.state.selectedId = id;
    this.state.error = null;
    this.state.notice = null;
    const version = ++this.readVersion;
    this.state.refreshing = false;
    this.changed();
    this.syncContext();
    if (id === null || this.detailsLoaded.has(id) || this.state.pending) { void this.loadTimelines(); return; }
    this.state.refreshing = true;
    this.changed();
    try {
      const data = parseToolResult(await this.bridge.call("get_factory_run", { run_id: id }));
      if (version !== this.readVersion) return;
      if (data.kind !== "run" || data.value.id !== id) throw new Error("The server returned a different run. Selection was preserved.");
      this.apply(data);
      this.syncContext();
    } catch (error) { if (version === this.readVersion) this.state.error = errorMessage(error); }
    finally { if (version === this.readVersion) { this.state.refreshing = false; this.syncContext(); this.changed(); void this.loadTimelines(); } }
  }
  async refresh(): Promise<void> {
    if (this.state.pending) return;
    const version = ++this.readVersion;
    this.state.refreshing = true;
    this.state.error = null;
    this.syncContext();
    this.changed();
    try {
      const args = this.state.selectedId ? { run_id: this.state.selectedId } : {};
      const result = await this.bridge.call("refresh_factory", args);
      // The grouping has its own fence, so a superseded run read still refreshes it.
      const grouping = this.loadCampaigns();
      const data = parseToolResult(result);
      if (version !== this.readVersion) { await grouping; return; }
      this.apply(data);
      this.syncContext();
      await grouping;
    } catch (error) { if (version === this.readVersion) this.state.error = errorMessage(error); }
    finally { if (version === this.readVersion) { this.state.refreshing = false; this.syncContext(); this.changed(); } }
  }
  /**
   * Read-only campaign grouping. A server without the tool, or a failed or inconsistent read,
   * never blocks the run list: the last valid grouping is kept and marked stale, otherwise
   * runs keep their ungrouped tiers. Campaign data never feeds attention or proof.
   */
  async loadCampaigns(): Promise<void> {
    if (!this.state.connected) return;
    const version = ++this.campaignVersion;
    try {
      const parsed = campaignListSchema.parse(structuredResult(await this.bridge.call("list_factory_campaigns", { limit: 100 })));
      if (version !== this.campaignVersion) return;
      this.state.campaigns = parsed.campaigns; this.state.campaignsSkipped = parsed.skipped.length; this.state.campaignsStale = false;
    } catch {
      if (version === this.campaignVersion) this.state.campaignsStale = this.state.campaigns !== null;
    } finally { if (version === this.campaignVersion) this.changed(); }
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
    if (!this.state.connected || this.state.pending || tool === "start_factory" && this.state.capabilities?.execution?.eligible !== true) return false;
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
      if (tool === "start_factory") {
        this.state.selectedId = data.value.id;
        ++this.graphVersion;
        this.state.graph = null; this.state.proposal = null; this.state.selectedNodeId = null; this.state.graphLoading = false;
        this.resetDelivery();
      }
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
      const result = structuredResult(await this.bridge.call("update_factory_settings", { set: settings }));
      const values = result !== null && typeof result === "object" && "values" in result ? result.values : undefined;
      this.state.settings = settingsSchema.parse(values);
      this.state.notice = "Defaults saved. Trusted server limits still apply.";
      return true;
    } catch (error) { this.state.error = errorMessage(error); return false; }
    finally { this.state.pending = null; this.changed(); }
  }
  async createGraph(args: Record<string, unknown>): Promise<boolean> {
    if (!this.state.connected || this.state.pending) return false;
    const selectionEpoch = ++this.selectionEpoch;
    this.contextRestoreEpoch = null;
    this.bridge.selectContext?.();
    this.state.pending = { tool: "create_factory_graph", runId: null };
    ++this.readVersion; ++this.graphVersion;
    this.state.graphLoading = false;
    this.state.error = null; this.changed();
    try {
      const envelope = graphEnvelopeSchema.parse(structuredResult(await this.bridge.call("create_factory_graph", args)));
      if (!envelope.graph.planning_only || envelope.graph.claim.held || envelope.graph.attempts.length) throw new Error("The server did not return a planning-only graph");
      if (selectionEpoch !== this.selectionEpoch) { this.state.notice = "Plan saved. Refresh the run list to inspect it."; return true; }
      this.state.selectedId = envelope.graph.run_id;
      this.state.graph = null;
      this.resetDelivery();
      this.acceptGraph(envelope);
      this.state.initialized = true;
      this.state.notice = "Plan saved. Inspect its tasks and review changes before applying them. No execution was started.";
      this.lastContext = ""; this.syncContext();
      return true;
    } catch (error) { this.state.error = `${errorMessage(error)} Retry unchanged fields to reuse the request key.`; return false; }
    finally { this.state.pending = null; this.changed(); }
  }
  async loadGraph(): Promise<void> {
    const id = this.state.selectedId;
    if (!this.state.connected || !id || this.state.pending) return;
    const version = ++this.graphVersion;
    this.state.graphLoading = true; this.state.error = null; this.syncContext(); this.changed();
    const [graph, backends] = await Promise.allSettled([
      this.bridge.call("get_factory_graph", { run_id: id }), this.bridge.call("get_factory_backends", {}),
    ]);
    if (version !== this.graphVersion || id !== this.state.selectedId) return;
    try {
      if (graph.status === "rejected") throw graph.reason;
      this.acceptGraph(graphEnvelopeSchema.parse(structuredResult(graph.value)));
      if (this.selected?.control?.revision !== this.state.graph?.revision) await this.readGraphRun(id);
      if (version !== this.graphVersion || id !== this.state.selectedId) return;
      if (backends.status === "fulfilled") this.state.backends = backendCatalogSchema.parse(structuredResult(backends.value));
      else { this.state.backends = null; this.state.error = "Execution targets could not be read. Target preferences are unavailable."; }
    } catch (error) { if (version === this.graphVersion && id === this.state.selectedId) { this.state.graphStale = true; this.state.error = `${errorMessage(error)} Refresh the graph before changing it.`; } }
    finally { if (version === this.graphVersion && id === this.state.selectedId) { this.state.graphLoading = false; this.changed(); this.syncContext(); } }
    if (version === this.graphVersion && id === this.state.selectedId) await this.loadDelivery(true);
  }
  private resetDelivery(): void { this.state.delivery = null; this.deliveryReadAt = null; }
  /**
   * Read GitHub-reported delivery telemetry for the loaded plan. Only when the operator configured
   * the GitHub App and the plan has GitHub sources; at most once per minute. A failure clears the
   * display quietly: it is never an error banner, attention, proof or a reason to retry anything.
   */
  async loadDelivery(force = false): Promise<void> {
    const id = this.state.selectedId;
    const graph = this.state.graph;
    if (!this.state.connected || !id || !graph || graph.run_id !== id || this.state.graphStale || this.state.capabilities?.github?.configured !== true) return;
    if (!graph.nodes.some(node => node.source?.provider === "github")) return;
    if (!force && this.deliveryReadAt !== null && Date.now() - this.deliveryReadAt < DELIVERY_INTERVAL_MS) return;
    const version = this.graphVersion;
    this.deliveryReadAt = Date.now();
    let delivery: Delivery | null = null;
    try {
      const parsed = deliverySchema.parse(structuredResult(await this.bridge.call("read_factory_delivery", { run_id: id })));
      delivery = parsed.run_id === id ? parsed : null;
    } catch { delivery = null; }
    if (version !== this.graphVersion || id !== this.state.selectedId) return;
    this.state.delivery = delivery;
    this.changed();
  }
  async restoreContext(id: string, nodeId?: string, expectedRevision?: number): Promise<void> {
    const epoch = this.selectionEpoch + 1;
    ++this.contextVersion;
    this.contextRestoreEpoch = epoch;
    this.state.selectedNodeId = null;
    try {
      await this.select(id, false);
      if (nodeId && epoch === this.selectionEpoch && this.state.selectedId === id) {
        await this.loadGraph();
        if (epoch === this.selectionEpoch && this.state.selectedId === id) {
          const graph = this.state.graph;
          if (expectedRevision !== undefined && graph?.revision !== expectedRevision) { this.state.selectedNodeId = null; this.reportError("This task link is stale. Refresh the run and reopen the task at its current revision."); }
          else if (!graph?.nodes.some(node => node.id === nodeId)) { this.state.selectedNodeId = null; this.reportError("This task is no longer present in the selected run."); }
          else this.selectNode(nodeId, false);
        }
      }
    } finally {
      if (this.contextRestoreEpoch === epoch) { this.contextRestoreEpoch = null; this.syncContext(); }
    }
  }
  selectNode(id: string, explicit = true): void {
    if (!this.state.graph?.nodes.some(node => node.id === id)) return;
    if (explicit) { ++this.selectionEpoch; this.contextRestoreEpoch = null; this.bridge.selectContext?.(); this.lastContext = ""; }
    // A different task ends an agent selection so the shared context never mixes two subjects.
    if (this.state.selectedAgent && roster(this.selected, this.state.graph).find(agent => agent.thread === this.state.selectedAgent)?.taskId !== id) this.state.selectedAgent = null;
    this.state.selectedNodeId = id; this.changed(); this.syncContext();
  }
  setViewMode(mode: "map" | "lanes"): void {
    if (this.state.viewMode === mode) return;
    this.state.viewMode = mode; this.changed();
    void this.loadTimelines();
  }
  /**
   * App-only reads of what Codex observed for each rostered agent, while Lanes is visible.
   * Bounded: one read at a time, at most MAX_TIMELINE_AGENTS agents, each at most once per
   * TIMELINE_INTERVAL_MS. Results are kept apart from runs and the graph, so they can never
   * change attention, proof, the Map or the ChatGPT context.
   */
  async loadTimelines(): Promise<void> {
    const run = this.selected;
    if (!this.state.connected || !this.lanesVisible() || !run || this.timelineLoading || !this.state.capabilities?.agent_timeline?.enabled) return;
    if (this.state.timelineRunId !== run.id) { this.state.timelines = {}; this.state.timelineRunId = run.id; }
    const now = Date.now();
    const key = (thread: string) => `${run.id}\u0000${thread}`;
    const due = roster(run, this.state.graph).slice(0, MAX_TIMELINE_AGENTS).filter(agent => now - (this.timelineReadAt.get(key(agent.thread)) ?? Number.NEGATIVE_INFINITY) >= TIMELINE_INTERVAL_MS);
    if (!due.length) return;
    const version = ++this.timelineVersion;
    this.timelineLoading = true;
    try {
      for (const agent of due) {
        // Stop as soon as Lanes closes or the page is hidden; unread agents stay due.
        if (!this.lanesVisible()) return;
        this.timelineReadAt.set(key(agent.thread), now);
        let view: AgentTimelineView = { status: "error" };
        try {
          const parsed = agentTimelineSchema.safeParse(structuredResult(await this.bridge.call("read_factory_agent_timeline", { run_id: run.id, thread_id: agent.thread })));
          if (parsed.success && parsed.data.run_id === run.id && parsed.data.thread_id === agent.thread) view = parsed.data;
        } catch { /* Shown as an unavailable observation; the run view is unaffected. */ }
        if (version !== this.timelineVersion || this.state.selectedId !== run.id) return;
        this.state.timelines = { ...this.state.timelines, [agent.thread]: view };
        this.changed();
      }
    } finally { if (version === this.timelineVersion) this.timelineLoading = false; }
  }
  private lanesVisible(): boolean {
    return this.state.viewMode === "lanes" && !(typeof document !== "undefined" && document.hidden);
  }
  /** Selecting an agent also selects its persisted task, so one context reaches ChatGPT. */
  selectAgent(thread: string | null, explicit = true): void {
    if (thread !== null && !roster(this.selected, this.state.graph).some(agent => agent.thread === thread)) return;
    // Only a user gesture reattaches context the user removed; model-driven focus respects that removal.
    if (explicit) { ++this.selectionEpoch; this.contextRestoreEpoch = null; this.bridge.selectContext?.(); this.lastContext = ""; }
    this.state.selectedAgent = thread;
    const task = roster(this.selected, this.state.graph).find(agent => agent.thread === thread)?.taskId;
    if (task && this.state.graph?.nodes.some(node => node.id === task)) this.state.selectedNodeId = task;
    this.changed(); this.syncContext();
    void this.loadTimelines();
  }
  private acceptGraph(envelope: GraphEnvelope): void {
    const graph = envelope.graph;
    if (graph.run_id !== this.state.selectedId) throw new Error("The server returned a graph for another run");
    const minimum = Math.max(this.state.graph?.revision ?? 0, this.selected?.control?.revision ?? 0);
    if (graph.revision < minimum) throw new Error("A stale graph revision was returned; your newer view is preserved.");
    this.state.graph = graph; this.state.graphStale = false;
    const next = envelope.proposal ?? [...graph.changes].reverse().find(change => change.status === "proposed" && change.base_revision + 1 === graph.revision) ?? null;
    // A dismissed "Plan updated" note stays dismissed when later reads return the same applied change.
    this.state.proposal = next?.status === "applied" && this.dismissedChanges.has(next.id) ? null : next;
    if (!graph.nodes.some(node => node.id === this.state.selectedNodeId)) this.state.selectedNodeId = graph.nodes[0]?.id ?? null;
  }
  async proposeChange(input: GraphChange): Promise<boolean> {
    const graph = this.state.graph;
    if (!this.state.connected || !graph || this.state.pending || this.state.graphLoading || this.state.graphStale) return false;
    if (!graph.planning_only && (graph.claim.held || graph.claim.status !== "released")) return false;
    // The server has no "clear target" change, and an empty ID would fail its node-ID bound.
    if (input.kind === "set_target" && !input.target_id) {
      this.reportNotice("Choose a planning target first. A saved planning note can't be cleared from here."); return false;
    }
    const change = graphChangeSchema.parse(input);
    if (change.kind !== "import_candidates" && !graph.nodes.some(node => node.id === change.node_id)) return false;
    const recovered = this.state.proposal;
    if (recovered?.status === "proposed" && recovered.base_revision + 1 === graph.revision && JSON.stringify(recovered.change) === JSON.stringify(change)) {
      this.reportNotice("This proposal is already saved. Review and confirm it before applying."); return true;
    }
    if (unchangedGraphChange(graph, change)) {
      this.reportNotice("No plan change: the selected values are already current."); return false;
    }
    if (change.kind === "set_target" && !this.state.backends?.targets.some(target => target.id === change.target_id && target.operator_enabled && target.planning_eligible && target.qualification !== "unsupported")) {
      this.reportError("This execution target cannot be selected for planning."); return false;
    }
    const fingerprint = JSON.stringify({ run_id: graph.run_id, revision: graph.revision, change });
    if (this.changeRetry?.fingerprint !== fingerprint) this.changeRetry = { fingerprint, key: crypto.randomUUID() };
    return this.graphMutation("propose_factory_change", { run_id: graph.run_id, expected_revision: graph.revision, idempotency_key: this.changeRetry.key, change });
  }
  /** Hides the "Plan updated" note. Display only: the applied change stays in the server's history. */
  dismissAppliedChange(): void {
    if (this.state.proposal?.status !== "applied") return;
    this.dismissedChanges.add(this.state.proposal.id);
    this.state.proposal = null; this.changed();
  }
  async applyChange(confirmed = false): Promise<boolean> {
    const { graph, proposal } = this.state;
    if (!confirmed || !this.state.connected || !graph || !proposal || proposal.status !== "proposed" || proposal.base_revision + 1 !== graph.revision || this.state.graphStale || this.state.pending || this.state.graphLoading) return false;
    if (!graph.planning_only && (graph.claim.held || graph.claim.status !== "released")) return false;
    if (unchangedGraphChange(graph, proposal.change)) { this.reportNotice("This saved proposal is unchanged. Its history is retained; nothing was applied."); return false; }
    return this.graphMutation("apply_factory_change", { run_id: graph.run_id, change_id: proposal.id, expected_revision: graph.revision });
  }
  private async graphMutation(tool: "propose_factory_change" | "apply_factory_change", args: Record<string, unknown>): Promise<boolean> {
    const version = ++this.graphVersion; ++this.readVersion;
    this.state.pending = { tool, runId: String(args.run_id) }; this.state.error = null; this.changed();
    try {
      const result = graphEnvelopeSchema.parse(structuredResult(await this.bridge.call(tool, args)));
      if (!result.proposal || (tool === "apply_factory_change" ? result.proposal.id !== args.change_id || result.proposal.status !== "applied" : result.proposal.base_revision !== args.expected_revision || JSON.stringify(result.proposal.change) !== JSON.stringify(args.change))) throw new Error("The server returned a different graph change");
      if (!this.state.connected || version !== this.graphVersion || this.state.selectedId !== args.run_id) return false;
      this.acceptGraph(result);
      await this.readGraphRun(String(args.run_id));
      if (!this.state.connected || version !== this.graphVersion || this.state.selectedId !== args.run_id) return false;
      this.syncContext();
      // The applied change shows as its own quiet "Plan updated" note, so it needs no second notice.
      this.state.notice = tool === "propose_factory_change" ? "Change proposed. Review it below, then apply it." : null;
      this.changeRetry = null;
      return true;
    } catch (error) { if (this.state.selectedId === args.run_id) { this.state.graphStale = true; this.state.error = `${errorMessage(error)} Refresh before retrying; the change may have reached the server.`; } return false; }
    finally { this.state.pending = null; this.changed(); }
  }
  private async readGraphRun(id: string): Promise<void> {
    const result = await this.bridge.call("get_factory_run", { run_id: id });
    if (this.state.selectedId !== id) return;
    const data = parseToolResult(result);
    if (data.kind !== "run" || data.value.id !== id || data.value.control?.revision !== this.state.graph?.revision) throw new Error("Run and graph revisions differ. Refresh both before changing or sharing the plan.");
    this.apply(data);
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
  async requestRepositoryWithHostForm(): Promise<boolean> {
    if (this.state.pending || this.state.discovering) return false;
    this.state.pending = { tool: "request_factory_repository", runId: null };
    this.state.error = null;
    this.state.notice = null;
    this.changed();
    try {
      const parsed = repositoryRegistrationSchema.safeParse(structuredResult(await this.bridge.call("request_factory_repository", {})));
      if (!parsed.success) throw new Error("The host form result is invalid. Use the accessible repository form below.");
      const request = parsed.data;
      if (this.state.discovery) this.state.discovery.requests = [...this.state.discovery.requests.filter(item => item.id !== request.id), request];
      this.state.notice = request.status === "approved" ? "Repository already approved. Refresh repositories to use its alias." : "Access requested. A local operator must approve it before a run can start.";
      return true;
    } catch (error) {
      this.state.error = `${errorMessage(error)} Use the accessible repository form below if this host cannot display a native form.`;
      return false;
    } finally { this.state.pending = null; this.changed(); }
  }
  /**
   * Read-only list for the plan form. Called only when the server offers GitHub connect; a server
   * without the tool, or a failed read, leaves just the approved repositories.
   */
  async loadGithubRepositories(cursor?: string): Promise<void> {
    if (!this.state.connected || this.state.capabilities?.workspaces?.connect_github !== true || this.state.githubLoading) return;
    const version = ++this.githubVersion;
    this.state.githubLoading = true; this.changed();
    try {
      const parsed = githubRepositoryListSchema.parse(structuredResult(await this.bridge.call("list_factory_github_repositories", cursor ? { cursor } : {})));
      if (version !== this.githubVersion) return;
      const previous = cursor ? this.state.githubRepos?.repositories ?? [] : [];
      const seen = new Set(parsed.repositories.map(repo => repo.full_name));
      this.state.githubRepos = { ...parsed, repositories: [...previous.filter(repo => !seen.has(repo.full_name)), ...parsed.repositories] };
    } catch { if (version === this.githubVersion && !cursor) this.state.githubRepos = null; }
    finally { if (version === this.githubVersion) { this.state.githubLoading = false; this.changed(); } }
  }
  /** View-only plan form choice: an approved alias, or `null` for "+ New repo…". Nothing is read or sent. */
  pickRepository(alias: string | null): void {
    if (alias !== null && !this.state.capabilities?.repositories.some(repo => repo.alias === alias)) return;
    if (alias === null && this.state.capabilities?.workspaces?.create_local !== true) return;
    if (alias !== null) this.state.pickedRepository = alias;
    this.state.newRepository = alias === null;
    this.changed();
  }
  /** Clone a GitHub repo into Luna's workspace as an approved alias. No push, branch, PR or run. */
  async connectGithubRepository(fullName: string): Promise<boolean> {
    const repo = this.state.githubRepos?.repositories.find(item => item.full_name === fullName);
    if (this.state.capabilities?.workspaces?.connect_github !== true || !repo || !repo.connectable || repo.connected_alias) {
      this.reportError("This repo can't be connected right now. Refresh and try again."); return false;
    }
    const added = await this.addRepository("connect_factory_github_repository", `github:${fullName}`, { repository: fullName }, result => {
      const parsed = githubConnectSchema.parse(result);
      if (parsed.full_name !== fullName) throw new Error("The server connected a different repo. Refresh before retrying.");
      return { alias: parsed.alias, notice: parsed.status === "already_connected" ? `${fullName} was already connected.` : `Connected ${fullName}.` };
    });
    if (typeof added === "string") this.state.newRepository = false;
    return added !== false;
  }
  /** Start a new empty local repository as an approved alias. No GitHub call and nothing runs. */
  async createLocalRepository(input: string): Promise<boolean> {
    return await this.createLocal(input) !== false;
  }
  /**
   * "+ New repo…" then Plan it: create the repo, then submit the plan with its alias. A name that
   * was already created and approved in this session is not created again, so retrying a failed
   * plan sends the same plan request (and idempotency key) without a second create call.
   */
  async createRepositoryAndSubmit(input: string, submit: (alias: string) => Promise<boolean>): Promise<boolean> {
    const name = input.trim();
    let alias = this.createdRepositories.get(name);
    if (!alias || !this.state.capabilities?.repositories.some(repo => repo.alias === alias)) {
      const added = await this.createLocal(name);
      if (added === false) return false;
      if (added === null) { this.reportError(`Luna created ${name} but hasn't listed it as ready yet. Refresh, then press Plan it again.`); return false; }
      alias = added;
      this.createdRepositories.set(name, alias);
    }
    const success = await submit(alias);
    if (success) { this.createdRepositories.delete(name); this.state.newRepository = false; this.state.pickedRepository = alias; }
    return success;
  }
  private createLocal(input: string): Promise<string | null | false> {
    const name = input.trim();
    if (this.state.capabilities?.workspaces?.create_local !== true) { this.reportError(repositoryReasonCopy("local_create_not_enabled")); return Promise.resolve(false); }
    if (!localRepositoryName.test(name)) { this.reportError("Use lowercase letters, digits, dots, dashes or underscores, starting with a letter or digit."); return Promise.resolve(false); }
    return this.addRepository("create_factory_local_repository", `local:${name}`, { name }, result => {
      const parsed = localCreateSchema.parse(result);
      return { alias: parsed.alias, notice: parsed.status === "already_exists" ? `${parsed.alias} already exists.` : `Created ${parsed.alias}.` };
    });
  }
  /** Resolves to the approved alias, `null` when added but not listed as approved yet, or `false` on failure. */
  private async addRepository(tool: "connect_factory_github_repository" | "create_factory_local_repository", keyFor: string, args: Record<string, unknown>, read: (result: unknown) => { alias: string; notice: string }): Promise<string | null | false> {
    if (!this.state.connected || this.state.pending) return false;
    // One key per repo until it succeeds, so a retry after a lost reply is the same request.
    const key = this.repositoryKeys.get(keyFor) ?? crypto.randomUUID();
    this.repositoryKeys.set(keyFor, key);
    this.state.pending = { tool, runId: null }; this.state.error = null; this.state.notice = null; this.changed();
    let added: { alias: string; notice: string };
    try {
      const result = structuredResult(await this.bridge.call(tool, { ...args, idempotency_key: key }));
      const reason = result !== null && typeof result === "object" && "reason" in result && typeof result.reason === "string" && !("alias" in result) ? result.reason : null;
      if (reason) throw new Error(repositoryReasonCopy(reason));
      added = read(result);
      this.repositoryKeys.delete(keyFor);
    } catch (error) {
      const message = errorMessage(error);
      this.state.error = /^[a-z_]+$/.test(message) ? repositoryReasonCopy(message) : error instanceof Error && error.name === "ZodError" ? "The server's reply didn't look right. Refresh before retrying." : message;
      this.state.pending = null; this.changed();
      return false;
    }
    this.state.pending = null;
    // The new alias is usable only once the server lists it as approved.
    await this.refresh();
    const ready = this.state.capabilities?.repositories.some(repo => repo.alias === added.alias) === true;
    if (ready) this.state.pickedRepository = added.alias;
    this.state.notice = ready ? `${added.notice} It's ready to plan.` : `${added.notice} Refresh if it doesn't show up yet.`;
    this.changed();
    if (tool === "connect_factory_github_repository") void this.loadGithubRepositories();
    return ready ? added.alias : null;
  }
  private syncContext(): void {
    if (this.contextRestoreEpoch !== null) return;
    const current = this.state.connected && !this.state.refreshing && !this.state.graphLoading && !this.state.graphStale;
    const graph = current && this.state.graph?.run_id === this.state.selectedId ? this.state.graph : null;
    const run = current && this.selected && this.detailsLoaded.has(this.selected.id) && (!graph || this.selected.control?.revision === graph.revision) ? this.selected : undefined;
    const node = graph?.nodes.find(item => item.id === this.state.selectedNodeId);
    const agent = run ? roster(run, graph).find(item => item.thread === this.state.selectedAgent) : undefined;
    const context = { ...(run ? boundedContext(run) : graph ? { run_id: graph.run_id, revision: graph.revision, repository: graph.repository.alias, state: graph.planning_only ? "PLANNING" : "UNVERIFIED", current_subject: graph.repository.subject.slice(0, 180) } : {}),
      ...(node && graph ? { node_id: node.id, node_title: node.title.slice(0, 300), node_state: node.state, graph_revision: graph.revision } : {}),
      ...(agent ? { agent_label: agent.label, agent_role: agent.role, agent_liveness: agent.liveness, agent_task_id: agent.taskId } : {}),
    };
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
  currentFollowUpPrompt(kind: FollowUpKind, taskId?: string): string {
    const run = this.selected;
    if (!this.state.connected || !run?.control || !run.presentation || !this.detailsLoaded.has(run.id) || this.state.refreshing || this.state.graphLoading || this.state.graphStale || this.state.pending) throw new Error("Refresh the current run and graph before sending ChatGPT context.");
    const graph = this.state.graph;
    const task = taskId ? graph?.nodes.find(node => node.id === taskId && node.id === this.state.selectedNodeId) : undefined;
    if (taskId && (!task || graph?.run_id !== run.id || graph.revision !== run.control?.revision)) throw new Error("The selected task is stale. Refresh the run and graph before sending context.");
    return buildFollowUpPrompt(run, kind, task, task ? graph?.revision : undefined);
  }
}
function errorMessage(error: unknown): string { return error instanceof Error ? error.message.slice(0, 800) : "The request could not be completed"; }
function unchangedGraphChange(graph: FactoryGraph, change: GraphChange): boolean {
  if (change.kind === "import_candidates") return false;
  const node = graph.nodes.find(item => item.id === change.node_id);
  if (!node) return false;
  return change.kind === "set_dependencies"
    ? change.dependencies.length === node.dependencies.length && new Set(change.dependencies).size === change.dependencies.length && change.dependencies.every(id => node.dependencies.includes(id))
    : change.target_id === node.target_preference;
}
function validOwnerMessage(value: unknown): value is string { return typeof value === "string" && value.trim().length > 0 && value.length <= 4000; }
