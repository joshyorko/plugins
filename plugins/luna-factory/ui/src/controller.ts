import { allowedFinishes, boundedContext, buildFollowUpPrompt, finishSchema, parseToolResult, repositoryDiscoverySchema, repositoryRegistrationSchema, runId, settingsSchema, structuredResult, type Capabilities, type FollowUpKind, type RepositoryDiscovery, type RunView, type Settings, type ToolData } from "./domain";
import { backendCatalogSchema, graphChangeSchema, graphEnvelopeSchema, type BackendCatalog, type FactoryGraph, type GraphChange, type GraphEnvelope } from "./domain";
import { campaignListSchema, type CampaignView } from "./domain";
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
}
export class WorkbenchController {
  readonly state: ViewState = { runs: [], capabilities: null, settings: {}, connectionStatus: "connecting", selectedId: null, initialized: false, connected: false, refreshing: false, pending: null, error: null, notice: null, contextError: null, discovery: null, discovering: false, graph: null, graphLoading: false, graphStale: false, selectedNodeId: null, proposal: null, backends: null, viewMode: "map", selectedAgent: null, campaigns: null, campaignsStale: false };
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
  private readonly detailsLoaded = new Set<string>();
  constructor(private readonly bridge: Bridge, private readonly changed: () => void) {}
  get selected(): RunView | undefined { return this.state.runs.find(run => run.id === this.state.selectedId); }
  setConnected(connected: boolean): void { this.state.connected = connected; this.state.connectionStatus = connected ? "connected" : "connecting"; this.lastContext = ""; this.syncContext(); this.changed(); }
  setDisconnected(message: string): void {
    ++this.readVersion; ++this.graphVersion; ++this.campaignVersion;
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
    }
    this.state.selectedId = id;
    this.state.error = null;
    this.state.notice = null;
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
    finally { if (version === this.readVersion) { this.state.refreshing = false; this.syncContext(); this.changed(); } }
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
      this.state.campaigns = parsed.campaigns; this.state.campaignsStale = false;
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
  }
  /** Selecting an agent also selects its persisted task, so one context reaches ChatGPT. */
  selectAgent(thread: string | null): void {
    if (thread !== null && !roster(this.selected, this.state.graph).some(agent => agent.thread === thread)) return;
    ++this.selectionEpoch; this.contextRestoreEpoch = null; this.bridge.selectContext?.(); this.lastContext = "";
    this.state.selectedAgent = thread;
    const task = roster(this.selected, this.state.graph).find(agent => agent.thread === thread)?.taskId;
    if (task && this.state.graph?.nodes.some(node => node.id === task)) this.state.selectedNodeId = task;
    this.changed(); this.syncContext();
  }
  private acceptGraph(envelope: GraphEnvelope): void {
    const graph = envelope.graph;
    if (graph.run_id !== this.state.selectedId) throw new Error("The server returned a graph for another run");
    const minimum = Math.max(this.state.graph?.revision ?? 0, this.selected?.control?.revision ?? 0);
    if (graph.revision < minimum) throw new Error("A stale graph revision was returned; your newer view is preserved.");
    this.state.graph = graph; this.state.graphStale = false;
    this.state.proposal = envelope.proposal ?? [...graph.changes].reverse().find(change => change.status === "proposed" && change.base_revision + 1 === graph.revision) ?? null;
    if (!graph.nodes.some(node => node.id === this.state.selectedNodeId)) this.state.selectedNodeId = graph.nodes[0]?.id ?? null;
  }
  async proposeChange(input: GraphChange): Promise<boolean> {
    const graph = this.state.graph;
    if (!this.state.connected || !graph || this.state.pending || this.state.graphLoading || this.state.graphStale) return false;
    if (!graph.planning_only && (graph.claim.held || graph.claim.status !== "released")) return false;
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
      this.state.notice = tool === "propose_factory_change" ? "Change proposed. Review its exact scope below, then apply it." : "Graph change applied. This records the plan; execution was not started.";
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
