import { App, applyDocumentTheme, applyHostStyleVariables } from "@modelcontextprotocol/ext-apps";
import { applyDeepLink, HostBridge, surfaceFromHostContext } from "./bridge";
import { WorkbenchController, type Bridge } from "./controller";
import { allowedFinishes, finishLabels, settingsSchema, startRequest, UI_VERSION, type FollowUpKind } from "./domain";
import { renderWorkbench, type Editor, type WorkbenchOptions } from "./view";
import { SinceTracker } from "./narrative";
import { submitNewRun } from "./submission";
import "./style.css";

const root = document.getElementById("app");
if (!root) throw new Error("Missing application root");
const mount = root;
let editor: Editor = null;
let pendingStart: { fingerprint: string; key: string } | null = null;
let messagePending = false;
const fixturePreview = import.meta.env.DEV && new URLSearchParams(location.search).get("preview") === "fixture";
const previewParams = new URLSearchParams(location.search);
const app = new App({ name: "Luna Factory", version: UI_VERSION }, { availableDisplayModes: ["inline", "fullscreen"] });
const hostBridge = new HostBridge(app, (id, nodeId, revision) => { void controller.restoreContext(id, nodeId, revision); }, message => { controller.setDisconnected(message); applyHostContext(); });
const extensions = hostBridge.extensions;
let connection: Promise<void> | null = null;
const since = new SinceTracker();
let workbenchOptions: WorkbenchOptions = { surface: "global", displayMode: "inline", canSendFollowUps: false, since };
const bridge: Bridge = {
  async call(tool, args) {
    if (fixturePreview) throw new Error("Fixture preview is read-only. Open Luna Factory in an MCP Apps host to run actions.");
    return hostBridge.call(tool, args);
  },
  async context(context) {
    if (!fixturePreview) await hostBridge.context(context);
  },
  selectContext() { hostBridge.selectContext(); },
};
const controller = new WorkbenchController(bridge, () => renderWorkbench(mount, controller.state, editor, fixturePreview, workbenchOptions));
renderWorkbench(mount, controller.state, editor, fixturePreview, workbenchOptions);

function showEditor(next: Editor): void {
  editor = next;
  renderWorkbench(mount, controller.state, editor, fixturePreview, workbenchOptions);
  mount.querySelector<HTMLElement>("form textarea, form select, form button")?.focus();
}
mount.addEventListener("click", event => {
  if (!(event.target instanceof Element)) return;
  const target = event.target.closest<HTMLElement>("[data-action],[data-run-id]");
  if (!target || target instanceof HTMLButtonElement && target.disabled) return;
  event.preventDefault();
  if (target.dataset.runId) { editor = null; void openRun(target.dataset.runId); return; }
  switch (target.dataset.action) {
    case "overview": editor = null; void controller.select(null); break;
    case "refresh": void (async () => { await controller.refresh(); if (controller.state.graph) await controller.loadGraph(); })(); break;
    case "graph": editor = null; void controller.loadGraph(); break;
    case "graph-node": if (target.dataset.nodeId) controller.selectNode(target.dataset.nodeId); break;
    case "view-mode": if (target.dataset.mode === "map" || target.dataset.mode === "lanes") controller.setViewMode(target.dataset.mode); break;
    case "select-agent": if (target.dataset.threadId) controller.selectAgent(controller.state.selectedAgent === target.dataset.threadId ? null : target.dataset.threadId); break;
    case "apply-change": void controller.applyChange(mount.querySelector<HTMLInputElement>("#confirm-graph-change")?.checked === true); break;
    case "share-context": void controller.select(controller.state.selectedId); break;
    case "start": showEditor("start"); break;
    case "settings": showEditor("settings"); break;
    case "repositories": showEditor("repositories"); void controller.discoverRepositories(); break;
    case "discover-repositories": void controller.discoverRepositories(); break;
    case "chat-repository-form": void controller.requestRepositoryWithHostForm(); break;
    case "control": {
      const run = controller.selected;
      const kind = target.dataset.kind;
      const tool = target.dataset.tool;
      const descriptor = run?.presentation?.actions.find(action => action.kind === kind && action.tool === (tool || null));
      if (run && (!run.control || !run.presentation) && kind === "refresh" && tool === "refresh_factory") { void controller.refresh(); break; }
      if (!run || !descriptor?.allowed) { controller.reportError("This action is no longer available. Refresh the run to read the current server decision."); return; }
      if (descriptor.kind === "answer" && descriptor.tool === "resume_factory_run") { showEditor("steer"); break; }
      if (descriptor.kind === "steer" && descriptor.tool === "steer_factory_run") { showEditor("steer"); break; }
      if (descriptor.kind === "cancel" && descriptor.tool === "cancel_factory_run") { showEditor("stop"); break; }
      if (descriptor.kind === "refresh" && descriptor.tool === "refresh_factory") { void (async () => { await controller.refresh(); if (run.planning_only || controller.state.graph) await controller.loadGraph(); })(); break; }
      if (descriptor.kind === "resume" && descriptor.tool === "resume_factory_run") { void controller.mutate("resume_factory_run", { run_id: run.id }); break; }
      if (descriptor.kind === "reconcile" && descriptor.tool === "reconcile_factory_run") { void controller.mutate("reconcile_factory_run", { run_id: run.id }); break; }
      if (descriptor.kind === "inspect" && descriptor.tool === "get_factory_run") { void controller.select(run.id); break; }
      controller.reportError("The server action is not supported by this workbench version. Refresh to read current status.");
      break;
    }
    case "close-editor": showEditor(null); break;
    case "expand-mode": void requestDisplayMode("fullscreen"); break;
    case "chat-follow-up": {
      const kind = target.dataset.kind;
      if (kind !== "summary" && kind !== "blocker" && kind !== "choose") { controller.reportError("This ChatGPT action is invalid."); break; }
      void sendFollowUp(kind, target.dataset.taskId);
      break;
    }
  }
});
mount.addEventListener("change", event => {
  if (event.target instanceof HTMLInputElement && event.target.id === "confirm-graph-change") {
    const apply = mount.querySelector<HTMLButtonElement>('[data-action="apply-change"]');
    if (apply) apply.disabled = !event.target.checked || !controller.state.connected || !!controller.state.pending || controller.state.graphStale || controller.state.graphLoading;
    return;
  }
  if (event.target instanceof HTMLSelectElement && event.target.name === "candidate_id") {
    const selected = event.target.value;
    const candidate = controller.state.discovery?.candidates.find(item => item.id === selected);
    const field = mount.querySelector<HTMLSelectElement>('select[name="max_finish"]');
    if (candidate && field) field.replaceChildren(...allowedFinishes(candidate.max_finish).map(finish => new Option(finishLabels[finish], finish)));
    return;
  }
  if (!(event.target instanceof HTMLSelectElement) || event.target.name !== "repository") return;
  const repo = controller.state.capabilities?.repositories.find(item => item.alias === (event.target instanceof HTMLSelectElement ? event.target.value : ""));
  const field = mount.querySelector<HTMLSelectElement>('select[name="finish"]');
  if (!repo || !field) return;
  const current = field.value;
  field.replaceChildren(...allowedFinishes(repo.max_finish).map(finish => new Option(finishLabels[finish], finish, false, current === finish)));
});
mount.addEventListener("submit", event => {
  if (!(event.target instanceof HTMLFormElement)) return;
  event.preventDefault();
  if (!event.target.reportValidity()) return;
  const form = event.target;
  const intent = event.submitter instanceof HTMLButtonElement ? event.submitter.value : "";
  const fields: Record<string, string> = {};
  for (const [key, value] of new FormData(form)) if (typeof value === "string") fields[key] = value;
  void (async () => {
    try {
      let success = false;
      if (form.dataset.form === "start") {
        if (!controller.state.capabilities) throw new Error("Approved capabilities have not loaded");
        const fingerprint = JSON.stringify({ fields, intent });
        if (pendingStart?.fingerprint !== fingerprint) pendingStart = { fingerprint, key: crypto.randomUUID() };
        const request = startRequest(fields, controller.state.capabilities, pendingStart.key);
        success = await submitNewRun(controller, intent, request);
        if (success) pendingStart = null;
        if (success && intent !== "start") { await controller.refresh(); await controller.loadGraph(); }
      } else if (form.dataset.form === "graph-node") {
        if (intent !== "target" && intent !== "dependencies") throw new Error("Choose which plan change to propose.");
        const nodeId = controller.state.selectedNodeId;
        if (!nodeId) throw new Error("Select a task first");
        success = await controller.proposeChange(intent === "target" ? { kind: "set_target", node_id: nodeId, target_id: fields.target_id ?? "" } : { kind: "set_dependencies", node_id: nodeId, dependencies: new FormData(form).getAll("dependencies").filter((value): value is string => typeof value === "string") });
        return;
      } else if (form.dataset.form === "settings") {
        const settings = settingsSchema.parse({ ...fields, capacity: Number(fields.capacity) });
        success = await controller.saveSettings(settings);
      } else if (form.dataset.form === "repositories") {
        await controller.requestRepository(fields);
        return;
      } else if (form.dataset.form === "steer") {
        success = await controller.sendOwnerInput(fields.message ?? "");
      } else if (form.dataset.form === "stop" && controller.selected) {
        success = await controller.mutate("cancel_factory_run", { run_id: controller.selected.id });
      }
      if (success) showEditor(null);
    } catch (error) { controller.reportError(error instanceof Error ? error.message : "Check the form fields"); }
  })();
});

let lastDeepLink: string | undefined;
function applyHostContext(): void {
  const context = app.getHostContext();
  if (context?.theme) applyDocumentTheme(context.theme);
  if (context?.styles?.variables) applyHostStyleVariables(context.styles.variables);
  const surface = surfaceFromHostContext(context);
  workbenchOptions = {
    since,
    surface,
    displayMode: context?.displayMode ?? "inline",
    canSendFollowUps: hostBridge.canSendFollowUp(),
    messagePending,
    canExpand: controller.state.connected && context?.displayMode === "inline" && context.availableDisplayModes?.includes("fullscreen") === true,
  };
  mount.dataset.surface = surface;
  mount.dataset.platform = context?.platform ?? "unknown";
  mount.dataset.displayMode = context?.displayMode ?? "inline";
  const deepLink = extensions.deepLink.getCurrent();
  if (deepLink && deepLink.url !== lastDeepLink) {
    lastDeepLink = deepLink.url;
    editor = null; void applyDeepLink(controller, deepLink.url);
  }
  // Rich elicitation is negotiated and rendered by the host during tools/call.
  // There is no client-side native form API in the published extension SDK.
  // Keep complete HTML forms on every host, including mobile and hosts without elicitation.
  mount.dataset.nativeForms = "html-fallback";
  renderWorkbench(mount, controller.state, editor, fixturePreview, workbenchOptions);
}
app.ontoolresult = result => controller.receiveInitial(result);
app.ontoolcancelled = () => controller.reportError("The host cancelled the opening request. Refresh to read the current state.");
app.onteardown = async () => {
  controller.setDisconnected("The MCP Apps host closed this workbench. Reopen Luna Factory to read current state.");
  await hostBridge.context({});
  return {};
};
app.addEventListener("hostcontextchanged", applyHostContext);

if (fixturePreview) {
  const { fixtureWorkbench, fixtureRun } = await import("../test/fixtures");
  const scenarios = [
    fixtureRun({ id: "review-301", pending_decision: { id: "review-301-decision-1", question: "Keep the protected migration local or remove it?" }, objective: "Make review recovery safe after reconnect", state: "NEEDS_INPUT", active_workers: 0, blocker: "The requested scope includes a protected migration. Keep it local or remove the migration?", delta: "Recovery checks pass. One scope decision is holding the final candidate.", remaining_gap: "Resolve the migration scope with the owner." }),
    fixtureRun(),
    fixtureRun({ id: "sample-204", repository: "sample-service", objective: "Preserve job progress across restarts", state: "RUNNING", delta: "The owner reproduced the restart gap. Two bounded workers are testing the fix.", active_workers: 2 }),
    fixtureRun({ id: "friday-41", repository: "friday", objective: "Tighten the nightly handoff", state: "CONVERGED", active_workers: 0, claim_held: false, blocker: null, remaining_gap: null, delta: "Owner accepted the current subject. The requested pull request is ready." }),
    fixtureRun({ id: "blocked-77", repository: "sample", objective: "Inspect blocked recovery safely", state: "BLOCKED", active_workers: 0, claim_held: true, blocker: "Native owner liveness is unknown. Keep the claim held." }),
  ];
  const scenario = previewParams.get("scenario");
  const selected = scenario === "blocked" ? scenarios[4] : scenario === "selected-node" ? scenarios[1] : scenarios[0];
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: scenarios, selected_run: selected } });
  controller.setConnected(true);
  mount.dataset.nativeForms = "html-fallback";
  workbenchOptions = {
    since,
    surface: previewParams.get("surface") === "inline" ? "inline" : previewParams.get("surface") === "thread" ? "thread" : "global",
    displayMode: previewParams.get("surface") === "inline" ? "inline" : "fullscreen",
    canSendFollowUps: previewParams.get("message") === "1",
    canExpand: previewParams.get("surface") === "inline",
  };
  mount.dataset.surface = workbenchOptions.surface;
  mount.dataset.platform = previewParams.get("platform") ?? "desktop";
  mount.dataset.displayMode = workbenchOptions.displayMode;
  if (previewParams.get("theme") === "dark") document.documentElement.dataset.theme = "dark";
  if (scenario === "disconnected") controller.setDisconnected("The MCP Apps host is disconnected. Reopen Luna Factory to refresh current state.");
  if (scenario === "error") controller.reportError("Synthetic host error: current revision could not be loaded.");
  if (previewParams.get("scenario") === "selected-node") {
    const { fixtureGraph } = await import("../test/fixtures");
    const graph = fixtureGraph().graph;
    controller.state.graph = graph;
    controller.state.selectedNodeId = graph.nodes[0]?.id ?? null;
  }
  renderWorkbench(mount, controller.state, editor, fixturePreview, workbenchOptions);
} else {
  connection = hostBridge.connect();
  void connection.then(() => {
    controller.setConnected(true);
    applyHostContext();
    // Single-run tool results render immediately; only fetch missing workbench controls.
    if (!controller.state.initialized || !controller.state.capabilities) void controller.refresh().then(readSelectedPlan);
    else readSelectedPlan();
  }).catch(() => controller.setDisconnected("Could not connect to the MCP Apps host. Reopen Luna Factory from the host to reconnect."));
  // Read-only status polling. It never starts a model, and pauses while hidden or editing.
  const refreshTimer = window.setInterval(() => {
    if (!document.hidden && controller.state.connected && controller.state.initialized && !controller.state.pending && !controller.state.refreshing && !editor) void controller.refresh().then(() => { if (controller.state.graphStale) readSelectedPlan(); void controller.loadTimelines(); });
  }, 30_000);
  window.addEventListener("pagehide", () => window.clearInterval(refreshTimer), { once: true });
}

/** Opening a run also reads its persisted plan; both are read-only calls. */
async function openRun(id: string): Promise<void> {
  await controller.select(id);
  readSelectedPlan();
}
function readSelectedPlan(): void {
  const { selectedId, graph, graphLoading, pending, connected } = controller.state;
  if (!connected || !selectedId || pending || graphLoading || graph?.run_id === selectedId && !controller.state.graphStale) return;
  void controller.loadGraph();
}
// Arrow keys move between tasks in reading order; Enter/Space already activate buttons.
mount.addEventListener("keydown", event => {
  if (!(event.target instanceof HTMLElement) || !event.target.matches(".map-node") || !["ArrowDown", "ArrowUp", "ArrowRight", "ArrowLeft"].includes(event.key)) return;
  const nodes = Array.from(mount.querySelectorAll<HTMLElement>(".map-node"));
  const index = nodes.indexOf(event.target);
  const next = nodes[index + (event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1)];
  if (next) { event.preventDefault(); next.focus(); }
});

async function sendFollowUp(kind: FollowUpKind, taskId?: string): Promise<void> {
  if (fixturePreview) { controller.reportError("Fixture preview is read-only. No message was sent."); return; }
  const run = controller.selected;
  if (messagePending) { controller.reportError("A ChatGPT message is already being sent."); return; }
  if (!run) { controller.reportError("Select a current run before sending a ChatGPT message."); return; }
  let prompt: string;
  try { prompt = controller.currentFollowUpPrompt(kind, taskId); }
  catch (error) { controller.reportError(error instanceof Error ? error.message : "Refresh before sending context."); return; }
  messagePending = true;
  applyHostContext();
  try {
    await hostBridge.sendFollowUp(prompt);
    controller.reportNotice("Message sent to the active ChatGPT conversation.");
  } catch (error) {
    controller.reportError(error instanceof Error ? error.message : "ChatGPT could not receive this message.");
  } finally {
    messagePending = false;
    applyHostContext();
  }
}

async function requestDisplayMode(mode: "inline" | "fullscreen"): Promise<void> {
  if (fixturePreview) { controller.reportError("Fixture preview cannot change ChatGPT display modes."); return; }
  if (!app.getHostContext()?.availableDisplayModes?.includes(mode)) {
    controller.reportError(`This host does not support the ${mode} display mode.`);
    return;
  }
  try { await app.requestDisplayMode({ mode }); }
  catch { controller.reportError("The host could not change the Luna Factory display mode."); }
}
