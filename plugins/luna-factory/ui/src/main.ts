import { App, applyDocumentTheme, applyHostStyleVariables } from "@modelcontextprotocol/ext-apps";
import { applyDeepLink, HostBridge, surfaceFromHostContext } from "./bridge";
import { WorkbenchController, type Bridge } from "./controller";
import { allowedFinishes, finishLabels, localRepositoryName, settingsSchema, startRequest, taskTitleUpdate, UI_VERSION, type FollowUpKind } from "./domain";
import { filterRepositories, renderWorkbench, toggleDisclosure, type Editor, type WorkbenchOptions } from "./view";
import { SinceTracker, type Baseline } from "./narrative";
import { registerViewTools } from "./view-tools";
import { submitNewRun } from "./submission";
import { bindRepoMenu, openRepoMenu } from "./repo-menu";
import "./style.css";

const root = document.getElementById("app");
if (!root) throw new Error("Missing application root");
const mount = root;
let editor: Editor = null;
let pendingStart: { fingerprint: string; key: string } | null = null;
let messagePending = false;
const fixturePreview = import.meta.env.DEV && new URLSearchParams(location.search).get("preview") === "fixture";
const previewParams = new URLSearchParams(location.search);
const app = new App({ name: "Luna Factory", version: UI_VERSION }, { availableDisplayModes: ["inline", "fullscreen"], tools: { listChanged: false } });
const hostBridge = new HostBridge(app, (id, nodeId, revision) => { void controller.restoreContext(id, nodeId, revision); }, message => { controller.setDisconnected(message); applyHostContext(); });
const extensions = hostBridge.extensions;
let connection: Promise<void> | null = null;
// ChatGPT can persist small widget state; other hosts keep the baseline for this session only.
type WidgetHost = { widgetState?: unknown; setWidgetState?: (state: Record<string, unknown>) => unknown };
const widgetHost = (globalThis as { openai?: WidgetHost }).openai;
const restoredSince = (widgetHost?.widgetState as { since?: Record<string, Baseline> } | undefined)?.since ?? {};
const since = new SinceTracker(undefined, restoredSince, typeof widgetHost?.setWidgetState === "function" ? baselines => { try { void widgetHost.setWidgetState?.({ ...(widgetHost.widgetState as Record<string, unknown> | undefined), since: baselines }); } catch { /* Persistence is best effort. */ } } : undefined);
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
  (mount.querySelector<HTMLElement>('form[data-form="start"] #objective') ?? mount.querySelector<HTMLElement>("form textarea, form select, form button"))?.focus();
}
mount.addEventListener("click", event => {
  if (!(event.target instanceof Element)) return;
  const target = event.target.closest<HTMLElement>("[data-action],[data-run-id]");
  const headerMenu = mount.querySelector<HTMLDetailsElement>(".header-menu[open]");
  if (headerMenu && (!target || !target.closest(".header-menu") || target.dataset.action)) headerMenu.open = false;
  if (!target || target instanceof HTMLButtonElement && target.disabled) return;
  if (event.target.closest("a.source-link") && target.dataset.runId) return;
  event.preventDefault();
  if (target.dataset.runId) { editor = null; void openRun(target.dataset.runId); return; }
  switch (target.dataset.action) {
    case "overview": editor = null; void controller.select(null); break;
    case "refresh": void (async () => { await controller.refresh(); if (controller.state.graph) await controller.loadGraph(); })(); break;
    case "reconnect": void (async () => {
      try { connection = hostBridge.connect(); await connection; controller.setConnected(true); applyHostContext(); await controller.refresh(); }
      catch { controller.setDisconnected("Could not reconnect to Luna Factory. Check the host connection and try again."); }
    })(); break;
    case "graph": editor = null; void controller.loadGraph(); break;
    case "graph-node": if (target.dataset.nodeId) controller.selectNode(target.dataset.nodeId); break;
    case "close-task": controller.selectNode(null); break;
    case "view-mode": if (target.dataset.mode === "map" || target.dataset.mode === "lanes" || target.dataset.mode === "details") controller.setViewMode(target.dataset.mode); break;
    case "select-agent": if (target.dataset.threadId) controller.selectAgent(controller.state.selectedAgent === target.dataset.threadId ? null : target.dataset.threadId); break;
    case "dismiss-change": controller.dismissAppliedChange(); break;
    case "cancel-review": controller.dismissProposal(); break;
    case "toggle-disclosure": {
      toggleDisclosure(mount, target);
      if (target.getAttribute("aria-expanded") === "true") document.getElementById(target.getAttribute("aria-controls") ?? "")?.querySelector<HTMLElement>("input:not(:disabled), select:not(:disabled)")?.focus();
      break;
    }
    case "close-edit-sheet": {
      const sheet = target.closest<HTMLElement>(".edit-sheet[data-disclosure]");
      const toggle = sheet ? Array.from(mount.querySelectorAll<HTMLElement>('[data-action="toggle-disclosure"]')).find(element => element.getAttribute("aria-controls") === sheet.id) : null;
      if (toggle) { toggleDisclosure(mount, toggle, false); toggle.focus(); }
      break;
    }
    case "apply-change": void controller.applyChange(true); break;
    case "share-context": void controller.select(controller.state.selectedId); break;
    case "start": controller.state.newRepository = false; editor = null; void controller.select(null); void controller.loadGithubRepositories(); break;
    case "promote-campaign": void controller.promoteCampaign(); break;
    case "settings": showEditor("settings"); break;
    case "connect-github": showEditor("start"); openRepoMenu(mount, "first"); break;
    case "setup": {
      // The plan form's "one-time setup" link: read-only guidance in Settings. Nothing is enabled.
      showEditor("settings");
      const heading = mount.querySelector<HTMLElement>("#setup-running");
      heading?.focus({ preventScroll: true });
      heading?.scrollIntoView({ block: "nearest" });
      break;
    }
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
  if (event.target instanceof HTMLElement && event.target.closest(".planning-editor")) {
    const saveError = mount.querySelector<HTMLElement>("#task-save-error");
    if (saveError) { saveError.textContent = ""; saveError.hidden = true; }
  }
  if (event.target instanceof HTMLSelectElement && event.target.name === "candidate_id") {
    const selected = event.target.value;
    const candidate = controller.state.discovery?.candidates.find(item => item.id === selected);
    const field = mount.querySelector<HTMLSelectElement>('select[name="max_finish"]');
    if (candidate && field) field.replaceChildren(...allowedFinishes(candidate.max_finish).map(finish => new Option(finishLabels[finish], finish)));
    return;
  }
});
mount.addEventListener("input", event => {
  if (event.target instanceof HTMLInputElement && event.target.id === "repo_filter") filterRepositories(mount, event.target.value);
  if (event.target instanceof HTMLTextAreaElement && event.target.id === "objective") {
    const plan = mount.querySelector<HTMLButtonElement>('button[type="submit"][value="plan"]');
    if (plan) plan.disabled = !event.target.value.trim() || !controller.state.connected || Boolean(controller.state.pending);
  }
  // The name error shows only while the name is invalid; typing clears it.
  if (event.target instanceof HTMLInputElement && event.target.id === "new_repository") showRepoError(null);
});
bindRepoMenu(mount, {
  pick: alias => controller.pickRepository(alias),
  newRepository: () => { showRepoError(null); controller.pickRepository(null); },
  connect: fullName => { void controller.connectGithubRepository(fullName); },
  more: () => { if (controller.state.githubRepos?.next_cursor) void controller.loadGithubRepositories(controller.state.githubRepos.next_cursor); },
});
// Enter in a plan field always means Plan it, never the browser's default (first) submit button.
mount.addEventListener("keydown", event => {
  const field = event.target;
  if (event.key !== "Enter" || event.defaultPrevented || !(field instanceof HTMLInputElement) || field.type === "hidden") return;
  const form = field.closest<HTMLFormElement>('form[data-form="start"]');
  if (!form || field.closest("#repo-popover")) return;
  event.preventDefault();
  form.requestSubmit(form.querySelector<HTMLButtonElement>('button[type="submit"][value="plan"]'));
});
/** Inline repo error under the composer, or clear it. Never a toast or a page error. */
function showRepoError(message: string | null): void {
  const error = mount.querySelector<HTMLElement>("#plan-repo-error");
  const input = mount.querySelector<HTMLInputElement>("#new_repository");
  if (error) { error.textContent = message ?? ""; error.hidden = !message; }
  if (message && input) input.setAttribute("aria-invalid", "true"); else input?.removeAttribute("aria-invalid");
}
mount.addEventListener("submit", event => {
  if (!(event.target instanceof HTMLFormElement)) return;
  event.preventDefault();
  const form = event.target;
  const newRepoInput = form.dataset.form === "start" ? form.querySelector<HTMLInputElement>("#new_repository") : null;
  if (form.dataset.form === "start") {
    // Repo checks come first and stay inline: a bad name never reaches the server.
    if (newRepoInput && !localRepositoryName.test(newRepoInput.value.trim())) {
      showRepoError(newRepoInput.value.trim() ? "Use lowercase letters, digits, dots, dashes or underscores, starting with a letter or digit (64 at most)." : "Name the new repo.");
      newRepoInput.focus();
      return;
    }
    if (!newRepoInput && !form.querySelector<HTMLInputElement>('input[name="repository"]')?.value) {
      showRepoError("Pick a repo first.");
      openRepoMenu(mount);
      return;
    }
    showRepoError(null);
  }
  if (!form.reportValidity()) return;
  const intent = event.submitter instanceof HTMLButtonElement ? event.submitter.value : "";
  const fields: Record<string, string> = {};
  for (const [key, value] of new FormData(form)) if (typeof value === "string") fields[key] = value;
  void (async () => {
    try {
      let success = false;
      if (form.dataset.form === "start") {
        if (!controller.state.capabilities) throw new Error("Approved capabilities have not loaded");
        // The new repo's alias is only known after it is created, so it is not part of the retry key.
        if (newRepoInput) delete fields.repository;
        const fingerprint = JSON.stringify({ fields, intent });
        if (pendingStart?.fingerprint !== fingerprint) pendingStart = { fingerprint, key: crypto.randomUUID() };
        const key = pendingStart.key;
        const capabilities = controller.state.capabilities;
        const settings = controller.state.settings;
        const { new_repository: name, ...request } = fields;
        success = newRepoInput
          ? await controller.createRepositoryAndSubmit(name ?? "", alias => submitNewRun(controller, intent, startRequest({ ...request, repository: alias }, controller.state.capabilities ?? capabilities, key, controller.state.settings)))
          : await submitNewRun(controller, intent, startRequest(request, capabilities, key, settings));
        if (success) pendingStart = null;
        if (success && intent !== "start") { await controller.refresh(); await controller.loadGraph(); }
      } else if (form.dataset.form === "graph-node") {
        if (intent !== "save") throw new Error("Choose Save changes.");
        const nodeId = controller.state.selectedNodeId;
        const graph = controller.state.graph;
        const selected = graph?.nodes.find(node => node.id === nodeId);
        if (!nodeId || !graph || !selected) throw new Error("Select a task first.");
        const dependencies = new FormData(form).getAll("dependencies").filter((value): value is string => typeof value === "string");
        const titleField = form.querySelector<HTMLTextAreaElement>("#task-title");
        const canonicalTitle = selected.editor_title === null ? null : selected.editor_title ?? selected.title;
        const title = titleField ? taskTitleUpdate(titleField.value, canonicalTitle) : fields.title ?? "";
        if (title === "") {
          const saveError = form.querySelector<HTMLElement>("#task-save-error");
          if (saveError) { saveError.textContent = "Add a task title."; saveError.hidden = false; }
          form.querySelector<HTMLTextAreaElement>("#task-title")?.focus();
          return;
        }
        const targetId = form.querySelector<HTMLSelectElement>("#target_id")?.value ?? "";
        const editToggle = Array.from(mount.querySelectorAll<HTMLElement>('[data-action="toggle-disclosure"]')).find(element => element.getAttribute("aria-controls") === `edit-${nodeId}`);
        if (editToggle) toggleDisclosure(mount, editToggle, false);
        success = await controller.proposeChange({ kind: "update_task", node_id: nodeId, title, dependencies, target_id: targetId || null });
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
    fixtureRun({ id: "sample-204", repository: "sample-service", objective: "Preserve job progress across restarts", state: "RUNNING", delta: "Luna reproduced the restart gap. Two agents are testing the fix.", active_workers: 2 }),
    fixtureRun({ id: "friday-41", repository: "friday", objective: "Tighten the nightly handoff", state: "CONVERGED", active_workers: 0, claim_held: false, blocker: null, remaining_gap: null, delta: "Luna accepted the current plan. The requested pull request is ready." }),
    fixtureRun({ id: "blocked-77", repository: "sample", objective: "Inspect blocked recovery safely", state: "BLOCKED", active_workers: 0, claim_held: true, blocker: "Codex status is unknown. Keep the work paused." }),
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
  if (previewParams.get("scenario") === "plan") void controller.loadGithubRepositories();
} else {
  // Model-callable view tools must exist before the handshake advertises them.
  registerViewTools(app as never, controller, { surface: () => workbenchOptions.surface ?? "global", readPlan: readSelectedPlan });
  connection = hostBridge.connect();
  void connection.then(() => {
    controller.setConnected(true);
    applyHostContext();
    // Single-run tool results render immediately; only fetch missing workbench controls.
    if (!controller.state.initialized || !controller.state.capabilities) void controller.refresh().then(readSelectedPlan).then(() => controller.loadGithubRepositories());
    else { readSelectedPlan(); void controller.loadCampaigns(); void controller.loadGithubRepositories(); }
  }).catch(() => controller.setDisconnected("Could not connect to the MCP Apps host. Reopen Luna Factory from the host to reconnect."));
  // Read-only status polling. It never starts a model, and pauses while hidden or editing.
  const refreshTimer = window.setInterval(() => {
    if (!document.hidden && controller.state.connected && controller.state.initialized && !controller.state.pending && !controller.state.refreshing && !editor) void controller.refresh().then(() => { if (controller.state.graphStale) readSelectedPlan(); else void controller.loadDelivery(); void controller.loadTimelines(); });
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
  const headerMenu = mount.querySelector<HTMLDetailsElement>(".header-menu[open]");
  if (headerMenu && event.target instanceof HTMLElement && headerMenu.contains(event.target)) {
    const items = Array.from(headerMenu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)'));
    const activeItem = event.target.closest<HTMLButtonElement>("button[role='menuitem']");
    const current = activeItem ? items.indexOf(activeItem) : -1;
    if (event.key === "Escape") {
      event.preventDefault();
      headerMenu.open = false;
      headerMenu.querySelector<HTMLElement>("summary")?.focus();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Home" || event.key === "End") {
      if (!items.length) return;
      event.preventDefault();
      const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
      return;
    }
  }
  const openSheet = mount.querySelector<HTMLElement>(".edit-sheet[data-disclosure]:not([hidden])");
  if (event.key === "Escape" && openSheet) {
    const toggle = Array.from(mount.querySelectorAll<HTMLElement>('[data-action="toggle-disclosure"]')).find(element => element.getAttribute("aria-controls") === openSheet.id);
    if (toggle) { event.preventDefault(); toggleDisclosure(mount, toggle, false); toggle.focus(); return; }
  }
  if (event.key === "Escape" && controller.state.selectedNodeId) {
    event.preventDefault();
    controller.selectNode(null);
    return;
  }
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
