import { App, applyDocumentTheme, applyHostStyleVariables } from "@modelcontextprotocol/ext-apps";
import { OpenAIExtensions } from "@openai/mcp-extensions/app";
import { WorkbenchController, type Bridge } from "./controller";
import { allowedFinishes, finishLabels, parseRunLink, settingsSchema, startRequest } from "./domain";
import { renderWorkbench, type Editor } from "./view";
import "./style.css";

const root = document.getElementById("app");
if (!root) throw new Error("Missing application root");
const mount = root;
let editor: Editor = null;
let pendingStart: { fingerprint: string; key: string } | null = null;
const fixturePreview = import.meta.env.DEV && new URLSearchParams(location.search).get("preview") === "fixture";
const app = new App({ name: "Luna Factory", version: "0.2.0" }, { availableDisplayModes: ["inline", "fullscreen"] });
const extensions = new OpenAIExtensions(app);
let connection: Promise<void> | null = null;
const bridge: Bridge = {
  async call(tool, args) {
    if (fixturePreview) throw new Error("Fixture preview is read-only. Open Luna Factory in an MCP Apps host to run actions.");
    if (!connection) throw new Error("The host is not connected");
    await connection;
    if (!app.getHostCapabilities()?.serverTools) throw new Error("This host does not support app tool calls");
    return app.callServerTool({ name: tool, arguments: args });
  },
  async context(context) {
    if (fixturePreview || !connection) return;
    await connection;
    const helper = extensions.modelContext;
    if (helper) { await helper.update({ structuredContent: context }); return; }
    const support = app.getHostCapabilities()?.updateModelContext;
    if (support?.structuredContent) await app.updateModelContext({ structuredContent: context });
    else if (support?.text) await app.updateModelContext({ content: [{ type: "text", text: JSON.stringify(context) }] });
  },
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
    case "refresh": void controller.refresh(); break;
    case "start": showEditor("start"); break;
    case "settings": showEditor("settings"); break;
    case "repositories": showEditor("repositories"); void controller.discoverRepositories(); break;
    case "discover-repositories": void controller.discoverRepositories(); break;
    case "steer": showEditor("steer"); break;
    case "stop": showEditor("stop"); break;
    case "close-editor": showEditor(null); break;
    case "resume": if (controller.selected) void controller.mutate("resume_factory_run", { run_id: controller.selected.id }); break;
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
  const fields: Record<string, string> = {};
  for (const [key, value] of new FormData(form)) if (typeof value === "string") fields[key] = value;
  void (async () => {
    try {
      let success = false;
      if (form.dataset.form === "start") {
        if (!controller.state.capabilities) throw new Error("Approved capabilities have not loaded");
        const fingerprint = JSON.stringify(fields);
        if (pendingStart?.fingerprint !== fingerprint) pendingStart = { fingerprint, key: crypto.randomUUID() };
        const request = startRequest(fields, controller.state.capabilities, pendingStart.key);
        success = await controller.mutate("start_factory", request);
        if (success) pendingStart = null;
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
    if (deepLink.url === "/") { editor = null; void controller.select(null); }
    else {
      const id = parseRunLink(deepLink.url);
      if (id) { editor = null; void controller.select(id); }
      else controller.reportError("This exact-run link is invalid");
    }
  }
  // Rich elicitation is negotiated and rendered by the host during tools/call.
  // There is no client-side native form API in the published extension SDK.
  // Keep complete HTML forms on every host, including mobile and hosts without elicitation.
  const elicitation = app.getHostCapabilities()?.experimental?.["openai/elicitation"];
  const nativeForms = elicitation && "form" in elicitation && elicitation.form;
  mount.dataset.nativeForms = nativeForms ? "host-supported" : "html-fallback";
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
  connection = app.connect();
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
