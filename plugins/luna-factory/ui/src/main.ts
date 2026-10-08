import { App, applyDocumentTheme, applyHostStyleVariables } from "@modelcontextprotocol/ext-apps";
import { applyDeepLink, HostBridge } from "./bridge";
import { WorkbenchController, type Bridge } from "./controller";
import { allowedFinishes, finishLabels, settingsSchema, startRequest } from "./domain";
import { renderWorkbench, type Editor } from "./view";
import { submitNewRun } from "./submission";
import "./style.css";

const root = document.getElementById("app");
if (!root) throw new Error("Missing application root");
const mount = root;
let editor: Editor = null;
let pendingStart: { fingerprint: string; key: string } | null = null;
const fixturePreview = import.meta.env.DEV && new URLSearchParams(location.search).get("preview") === "fixture";
const app = new App({ name: "Luna Factory", version: "0.2.0" }, { availableDisplayModes: ["inline", "fullscreen"] });
const hostBridge = new HostBridge(app, (id, nodeId) => { void controller.restoreContext(id, nodeId); });
const extensions = hostBridge.extensions;
let connection: Promise<void> | null = null;
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
const controller = new WorkbenchController(bridge, () => renderWorkbench(mount, controller.state, editor, fixturePreview));
renderWorkbench(mount, controller.state, editor, fixturePreview);

function showEditor(next: Editor): void {
  editor = next;
  renderWorkbench(mount, controller.state, editor, fixturePreview);
  mount.querySelector<HTMLElement>("form textarea, form select, form button")?.focus();
}
mount.addEventListener("click", event => {
  if (!(event.target instanceof Element)) return;
  const target = event.target.closest<HTMLElement>("[data-action],[data-run-id]");
  if (!target || target instanceof HTMLButtonElement && target.disabled) return;
  event.preventDefault();
  if (target.dataset.runId) { editor = null; void controller.select(target.dataset.runId); return; }
  switch (target.dataset.action) {
    case "overview": editor = null; void controller.select(null); break;
    case "refresh": void (async () => { await controller.refresh(); if (controller.state.graph) await controller.loadGraph(); })(); break;
    case "graph": editor = null; void controller.loadGraph(); break;
    case "graph-node": if (target.dataset.nodeId) controller.selectNode(target.dataset.nodeId); break;
    case "apply-change": void controller.applyChange(); break;
    case "share-context": void controller.select(controller.state.selectedId); break;
    case "start": showEditor("start"); break;
    case "settings": showEditor("settings"); break;
    case "repositories": showEditor("repositories"); void controller.discoverRepositories(); break;
    case "discover-repositories": void controller.discoverRepositories(); break;
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
      if (descriptor.kind === "refresh" && descriptor.tool === "refresh_factory") { void controller.refresh(); break; }
      if (descriptor.kind === "resume" && descriptor.tool === "resume_factory_run") { void controller.mutate("resume_factory_run", { run_id: run.id }); break; }
      if (descriptor.kind === "reconcile" && descriptor.tool === "reconcile_factory_run") { void controller.mutate("reconcile_factory_run", { run_id: run.id }); break; }
      if (descriptor.kind === "inspect" && descriptor.tool === "get_factory_run") { void controller.select(run.id); break; }
      controller.reportError("The server action is not supported by this workbench version. Refresh to read current status.");
      break;
    }
    case "close-editor": showEditor(null); break;
  }
});
mount.addEventListener("change", event => {
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
  const deepLink = extensions.deepLink.getCurrent();
  if (deepLink && deepLink.url !== lastDeepLink) {
    lastDeepLink = deepLink.url;
    editor = null; void applyDeepLink(controller, deepLink.url);
  }
  // Rich elicitation is negotiated and rendered by the host during tools/call.
  // There is no client-side native form API in the published extension SDK.
  // Keep complete HTML forms on every host, including mobile and hosts without elicitation.
  mount.dataset.nativeForms = "html-fallback";
}
app.ontoolresult = result => controller.receiveInitial(result);
app.ontoolcancelled = () => controller.reportError("The host cancelled the opening request. Refresh to read the current state.");
app.addEventListener("hostcontextchanged", applyHostContext);

if (fixturePreview) {
  const { fixtureWorkbench, fixtureRun } = await import("../test/fixtures");
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [
    fixtureRun({ id: "review-301", pending_decision: { id: "review-301-decision-1", question: "Keep the protected migration local or remove it?" }, objective: "Make review recovery safe after reconnect", state: "NEEDS_INPUT", active_workers: 0, blocker: "The requested scope includes a protected migration. Keep it local or remove the migration?", delta: "Recovery checks pass. One scope decision is holding the final candidate.", remaining_gap: "Resolve the migration scope with the owner." }),
    fixtureRun(),
    fixtureRun({ id: "sample-204", repository: "sample-service", objective: "Preserve job progress across restarts", state: "RUNNING", delta: "The owner reproduced the restart gap. Two bounded workers are testing the fix.", active_workers: 2 }),
    fixtureRun({ id: "friday-41", repository: "friday", objective: "Tighten the nightly handoff", state: "CONVERGED", active_workers: 0, claim_held: false, blocker: null, remaining_gap: null, delta: "Owner accepted the current subject. The requested pull request is ready." }),
  ] } });
  controller.setConnected(true);
  mount.dataset.nativeForms = "html-fallback";
} else {
  connection = hostBridge.connect();
  void connection.then(() => {
    controller.setConnected(true);
    applyHostContext();
    // Single-run tool results render immediately; only fetch missing workbench controls.
    if (controller.state.initialized && !controller.state.capabilities) void controller.refresh();
  }).catch(() => controller.reportError("Could not connect to the MCP Apps host. Reopen Luna Factory from the host to reconnect."));
  // Read-only status polling. It never starts a model, and pauses while hidden or editing.
  const refreshTimer = window.setInterval(() => {
    if (!document.hidden && controller.state.connected && controller.state.initialized && !controller.state.pending && !controller.state.refreshing && !editor) void controller.refresh();
  }, 30_000);
  window.addEventListener("pagehide", () => window.clearInterval(refreshTimer), { once: true });
}
