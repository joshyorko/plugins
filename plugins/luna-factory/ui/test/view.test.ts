// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { filterRepositories, renderWorkbench, type Editor } from "../src/view";
import { WorkbenchController } from "../src/controller";
import { fixtureGithubRepositories, fixtureGraph, fixturePlanCapabilities, fixtureRun, fixtureWorkbench } from "./fixtures";
import { githubRepositoryListSchema } from "../src/domain";
import { bindRepoMenu, isRepoMenuOpen } from "../src/repo-menu";

function render(editor: Editor = null, run = fixtureRun()) {
  const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [run], selected_run: run } });
  controller.setConnected(true);
  const root = document.createElement("div");
  renderWorkbench(root, controller.state, editor, false);
  return { root, controller };
}
describe("accessible workbench", () => {
  it("puts change, remaining gap, and actual blocker before evidence", () => {
    const { root } = render(null, fixtureRun({ state: "NEEDS_INPUT", blocker: "Choose whether to reduce the scope", pending_decision: { id: "decision-1", question: "Choose whether to reduce the scope" } }));
    expect(root.textContent).toContain("Choose whether to reduce the scope");
    const main = root.querySelector("main");
    expect(main?.querySelector(".story")).not.toBeNull();
    expect(main?.textContent?.indexOf("Choose whether to reduce the scope")).toBeLessThan(main?.textContent?.indexOf("Evidence and audit") ?? 0);
    expect(root.querySelector('[data-kind="answer"]')).not.toBeNull();
    expect(root.textContent).not.toContain("Force");
  });
  it("shows explicit ChatGPT follow-up buttons only when the host advertises message sending", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT", blocker: "Choose a safe scope", pending_decision: { id: "decision-1", question: "Choose a safe scope" } }));
    renderWorkbench(root, controller.state, null, false, { surface: "global", canSendFollowUps: true });
    const buttons = root.querySelectorAll<HTMLButtonElement>('[data-action="chat-follow-up"]');
    expect(buttons).toHaveLength(1);
    expect(buttons[0]?.textContent).toContain("Ask ChatGPT about this");
    expect(root.textContent).toContain("only after your click");
    expect(root.textContent).toContain("Nothing starts");
    expect(Array.from(buttons).every(button => !button.disabled)).toBe(true);
    renderWorkbench(root, controller.state, null, false, { surface: "global", canSendFollowUps: false });
    expect(Array.from(root.querySelectorAll<HTMLButtonElement>('[data-action="chat-follow-up"]')).every(button => button.disabled)).toBe(true);
    expect(root.textContent).toContain("unavailable in this host");
  });
  it("keeps inline cards glanceable and adapts the same data for the thread inspector", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT", blocker: "Choose a safe scope" }));
    renderWorkbench(root, controller.state, null, false, { surface: "inline", displayMode: "inline", canExpand: true });
    expect(root.querySelector(".inline-card")?.textContent).toContain("Choose a safe scope");
    expect(root.querySelector(".sidebar")).toBeNull();
    expect(root.querySelector('[data-action="expand-mode"]')?.textContent).toContain("Open Luna Factory");
    expect(root.querySelectorAll(".inline-card button")).toHaveLength(2);
    renderWorkbench(root, controller.state, null, false, { surface: "thread", displayMode: "fullscreen" });
    expect(root.querySelector('.workbench[data-surface="thread"]')).not.toBeNull();
    expect(root.querySelector(".thread-inspector-label")?.textContent).toContain("This conversation");
    expect(root.textContent).toContain("Choose a safe scope");
  });
  it("shows inline follow-up status and connection errors", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT" }));
    controller.reportError("ChatGPT could not receive this message.");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: false });
    expect(root.querySelector('[role="alert"]')?.textContent).toContain("could not receive");
    expect(Array.from(root.querySelectorAll('[data-action="chat-follow-up"]')).every(button => (button as HTMLButtonElement).disabled)).toBe(true);
    controller.reportNotice("Message sent to the active ChatGPT conversation.");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true });
    expect(root.querySelector('[role="status"]')?.textContent).toContain("Message sent");
  });
  it("restores keyboard focus to the same inline follow-up action after rerender", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT" }));
    document.body.append(root);
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true });
    const button = root.querySelector<HTMLButtonElement>('[data-action="chat-follow-up"][data-kind="blocker"]');
    if (!button) throw new Error("Missing blocker follow-up button");
    button.focus();
    const originalId = button.id;
    controller.reportNotice("Message sent to the active ChatGPT conversation.");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true });
    expect(document.activeElement?.id).toBe(originalId);
    root.remove();
  });
  it("keeps active follow-up focus while sending and restores it after the result", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT" }));
    document.body.append(root);
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true });
    const button = root.querySelector<HTMLButtonElement>('[data-action="chat-follow-up"][data-kind="blocker"]');
    if (!button) throw new Error("Missing blocker follow-up button");
    button.focus();
    const originalId = button.id;
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true, messagePending: true });
    const pending = Array.from(root.querySelectorAll<HTMLButtonElement>("[id]")).find(candidate => candidate.id === originalId);
    expect(pending?.disabled).toBe(false);
    expect(pending?.getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement?.id).toBe(originalId);
    controller.reportNotice("Message sent to the active ChatGPT conversation.");
    renderWorkbench(root, controller.state, null, false, { surface: "inline", canSendFollowUps: true });
    expect(document.activeElement?.id).toBe(originalId);
    root.remove();
  });
  it("labels disconnected host state instead of implying an active connection", () => {
    const { root, controller } = render();
    controller.setDisconnected("The MCP Apps host is disconnected.");
    renderWorkbench(root, controller.state, null, false, { surface: "global", canSendFollowUps: true, canExpand: true });
    expect(root.querySelector(".connection")?.textContent).toContain("Disconnected");
    expect(root.querySelector('[role="alert"]')?.textContent).toContain("disconnected");
    expect(Array.from(root.querySelectorAll<HTMLButtonElement>('[data-action="chat-follow-up"], [data-action="expand-mode"]')).every(button => button.disabled)).toBe(true);
  });
  it("binds a task follow-up to the selected graph node", () => {
    const { root, controller } = render();
    const graph = fixtureGraph().graph;
    controller.state.graph = graph;
    controller.state.selectedNodeId = graph.nodes[0]?.id ?? null;
    renderWorkbench(root, controller.state, null, false, { surface: "global", canSendFollowUps: true });
    const task = root.querySelector<HTMLButtonElement>('[data-action="chat-follow-up"][data-kind="choose"]');
    expect(task?.dataset.taskId).toBe(graph.nodes[0]?.id);
  });
  it("escapes server and repository strings as text", () => {
    const { root } = render(null, fixtureRun({ objective: '<img src=x onerror="alert(1)">' }));
    expect(root.querySelector("img")).toBeNull();
    expect(root.textContent).toContain("<img");
  });
  it("provides real labeled form controls and bounded approved options", () => {
    const { root } = render("start");
    expect(root.querySelector('form[data-form="start"]')).not.toBeNull();
    for (const field of root.querySelectorAll("input:not([type=hidden]),select,textarea")) expect(root.querySelector(`label[for="${field.id}"]`) ?? field.getAttribute("aria-label")).toBeTruthy();
    expect(root.querySelector("#repo-chip")?.textContent).toContain("plugins");
    expect(root.querySelector<HTMLInputElement>('input[name="repository"]')?.value).toBe("plugins");
    expect(root.querySelector('input[name="capacity"]')?.getAttribute("max")).toBe("4");
  });
  it("retains typed form drafts on status refresh", () => {
    const { root, controller } = render("steer");
    const input = root.querySelector("textarea");
    if (!input) throw new Error("Missing correction field");
    input.value = "Keep the current acceptance criteria";
    renderWorkbench(root, controller.state, "steer", false);
    expect(root.querySelector("textarea")?.value).toBe("Keep the current acceptance criteria");
  });
  it("does not carry an old answer into a replacement decision", () => {
    const run = fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "decision-1", question: "Use option A?" } });
    const { root, controller } = render("steer", run);
    const input = root.querySelector("textarea");
    if (!input) throw new Error("Missing answer field");
    input.value = "Yes, option A";
    controller.state.runs = [fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "decision-2", question: "Delete the optional file?" } })];
    renderWorkbench(root, controller.state, "steer", false);
    expect(root.querySelector("textarea")?.value).toBe("");
  });
  it("does not imply stopped descendants while cancellation is pending", () => {
    const { root } = render(null, fixtureRun({ state: "CANCELLING", claim_held: true }));
    expect(root.textContent).toContain("Repository claimownedRetained");
    expect(root.querySelector('[data-kind="resume"]')).toBeNull();
  });
  it("allows NEEDS_INPUT answers without an active turn and explains same-run continuation", () => {
    const run = fixtureRun({ state: "NEEDS_INPUT", turn_id: null, blocker: "Choose the bounded scope", pending_decision: { id: "decision-1", question: "Choose the bounded scope" } });
    const { root } = render(null, run);
    expect(root.querySelector('[data-kind="answer"]')?.textContent).toBe("Answer the owner");
    const form = render("steer", run).root;
    expect(form.querySelector('button[type="submit"]')?.hasAttribute("disabled")).toBe(false);
    expect(form.textContent).toContain("same run and owner thread");
    expect(form.textContent).not.toContain("Sent to the current turn only");
  });
  it("shows no invented runs or repository access when empty", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities: { ...fixtureWorkbench.capabilities, repositories: [] } } });
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.textContent).toContain("No runs yet");
    expect(root.textContent).toContain("No repositories yet");
    expect(root.textContent).toContain("Ask your operator");
    expect(root.querySelector('[data-action="start"]')?.hasAttribute("disabled")).toBe(true);
    expect(root.querySelector('[data-action="repositories"]')).toBeNull();
  });
  it("offers picking a repo from home when the server can connect or create one", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities: { ...fixturePlanCapabilities, repositories: [] } } });
    controller.setConnected(true);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector("#action-start-repository")?.textContent).toContain("Pick a repo");
    expect(root.querySelector('[data-action="repositories"]')).toBeNull();
    renderWorkbench(root, controller.state, "start", false);
    expect(root.textContent).toContain("Connect a repo or start a new one");
    expect(root.querySelector<HTMLButtonElement>('button[value="plan"]')?.disabled).toBe(false);
  });
  it("keeps settings free of the operator-folder flow", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities: { ...fixtureWorkbench.capabilities, repositories: [], repository_onboarding: { enabled: true, approval: "local_operator" } } } });
    controller.setConnected(true);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, "settings", false);
    expect(root.querySelector('[data-action="repositories"]')).toBeNull();
    expect(root.querySelector('input[name="path"]')).toBeNull();
  });
  it("renders discovered choices, explicit finish caps and pending local approval without path controls", () => {
    const { root, controller } = render();
    controller.state.capabilities = { ...fixtureWorkbench.capabilities, repository_onboarding: { enabled: true, approval: "local_operator" } };
    controller.state.discovery = { candidates: [{ id: "a".repeat(64), name: "sample", root_alias: "tests", max_finish: "local_candidate" }], requests: [{ id: "request-1", alias: "sandbox-test", name: "sample", root_alias: "tests", max_finish: "local_candidate", status: "pending" }], approval: "local_operator" };
    renderWorkbench(root, controller.state, "repositories", false);
    expect(root.querySelector('form[data-form="repositories"]')).not.toBeNull();
    expect(root.querySelector('[data-action="chat-repository-form"]')?.textContent).toContain("Choose with a ChatGPT form");
    expect(root.querySelector<HTMLSelectElement>('[name="candidate_id"]')?.value).toBe("a".repeat(64));
    expect(root.querySelector<HTMLSelectElement>('[name="max_finish"]')?.options.length).toBe(1);
    expect(root.querySelector('input[name="path"]')).toBeNull();
    expect(root.querySelector('input[name="approved"]')).toBeNull();
    expect(root.textContent).toContain("Pending operator approval");
    expect(root.textContent).toContain("request-1");
    expect(root.textContent).not.toContain("Starts native Codex execution");
  });
  it("explains unavailable effective routing without implying future telemetry", () => {
    const { root } = render();
    expect(root.textContent).toContain("No execution-side model evidence");
    expect(root.textContent).toContain("Effort telemetry unavailable");
    expect(root.textContent).not.toContain("Not yet observed");
  });
  it("keeps provider configuration and exact-turn mismatch evidence distinct", () => {
    const run = fixtureRun();
    run.route.configured_provider = "headroom-fixture";
    run.route.observed_model = "gpt-6-sol";
    run.route.observed_model_source = "model/rerouted";
    run.route.reroutes = [{ thread_id: "owner-123", turn_id: "turn-123", from_model: "gpt-6-luna", to_model: "gpt-6-sol", source: "model/rerouted", reason: "highRiskCyberActivity" }];
    const { root } = render(null, run);
    expect(root.textContent).toContain("headroom-fixture");
    expect(root.textContent).toContain("Downstream execution and billing unverified");
    expect(root.textContent).toContain("1 native model mismatch recorded");
    expect(root.textContent).toContain("owner-123, turn-123");
    expect(root.textContent).toContain("Effort telemetry unavailable");
  });
  it("puts objective, current story, the decision, and proof counts in order", () => {
    const { root } = render(null, fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "decision-1", question: "Keep the migration local?" } }));
    const text = root.querySelector("main")?.textContent ?? "";
    const story = root.querySelector(".story")?.textContent ?? "";
    expect(story).not.toBe("");
    expect(text.indexOf("Make Luna Factory a first-class workbench")).toBeLessThan(text.indexOf(story));
    expect(text.indexOf(story)).toBeLessThan(text.indexOf("Keep the migration local?"));
    expect(root.querySelector(".decision-panel .eyebrow")?.textContent).toBe("Needs you");
    expect(text).toContain("1 of 2 done");
    expect(root.querySelectorAll(".needs-you .button.primary")).toHaveLength(1);
  });
  it("renders stopped unresolved separately from finished verified and retains unknown liveness", () => {
    const run = fixtureRun({ state: "CANCELLED", active_workers: 0, claim_held: true });
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.result = { kind: "stopped_unresolved", label: "Stopped with work unresolved" };
    run.presentation.owner.liveness = "unknown";
    run.presentation.workers = [{ thread_id: "worker-retained", liveness: "unknown" }];
    const { root } = render(null, run);
    expect(root.textContent).toContain("Stopped with work unresolved");
    expect(root.textContent).toContain("unknown");
    expect(root.textContent).toMatch(/(Worker \d+|Unlisted worker): unknown/);
    expect(root.textContent).not.toContain("worker-retained");
    expect(root.textContent).toContain("Repository claim");
  });
  it("offers an allowed secondary stop only from the server action list", () => {
    const run = fixtureRun();
    const { root } = render(null, run);
    expect(root.querySelector('[data-kind="cancel"][data-tool="cancel_factory_run"]')).not.toBeNull();
    const noCancel = fixtureRun();
    const presentation = noCancel.presentation;
    if (!presentation) throw new Error("Missing fixture presentation");
    noCancel.presentation = { ...presentation, actions: presentation.actions.filter(action => action.kind !== "cancel") };
    const second = render(null, noCancel).root;
    expect(second.querySelector('[data-kind="cancel"]')).toBeNull();
  });
  it("folds an allowed secondary steer under wait without promoting it", () => {
    const run = fixtureRun();
    const current = run.presentation;
    if (!current) throw new Error("Missing fixture presentation");
    const wait = { kind: "wait" as const, label: "Wait for observed work", reason: "owned_execution_active", tool: null, allowed: false };
    const steer = { kind: "steer" as const, label: "Correct the owner", reason: "current_owned_turn", tool: "steer_factory_run", allowed: true };
    run.presentation = { ...current, primary_action: wait, actions: [wait, steer] };
    const { root } = render(null, run);
    expect(root.querySelector(".decision-panel .button.primary")?.textContent).toBe("Wait for observed work");
    const secondary = root.querySelector('[data-kind="steer"]');
    expect(secondary?.closest(".decision-panel .run-controls")).not.toBeNull();
    expect(secondary?.classList.contains("primary")).toBe(false);
    expect(root.querySelector(".decision-panel details")).toBeNull();
  });
  it("hides machine reason codes behind plain copy and keeps legacy action read-only", () => {
    const run = fixtureRun({ blocker: null });
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.primary_action = { ...run.presentation.primary_action, kind: "reconcile", label: "Reconcile existing work", reason: "owned_liveness_unknown", tool: "reconcile_factory_run", allowed: true };
    run.presentation.actions = [run.presentation.primary_action];
    const { root } = render(null, run);
    expect(root.textContent).toContain("Luna can't confirm the earlier run stopped.");
    expect(root.querySelector('[data-kind="reconcile"][data-tool="reconcile_factory_run"]')?.textContent).toBe("Check running work");
    expect(root.textContent).not.toContain("owned_liveness_unknown");
    const legacy = fixtureRun();
    delete legacy.control;
    delete legacy.presentation;
    const oldRoot = render(null, legacy).root;
    expect(oldRoot.textContent).toContain("Outcome unverified");
    expect(oldRoot.querySelector(".decision-panel .button.primary")?.textContent).toBe("Refresh current state");
    expect(oldRoot.querySelector('[data-kind="cancel"]')).toBeNull();
  });
});

function renderPlan(capabilities: Record<string, unknown>, github = true) {
  const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities } });
  controller.setConnected(true);
  if (github) controller.state.githubRepos = githubRepositoryListSchema.parse(fixtureGithubRepositories());
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  renderWorkbench(root, controller.state, "start", false);
  return { root, controller };
}
/** Text a person sees without opening the repo menu or More options. */
const visibleText = (root: HTMLElement): string => {
  const copy = root.querySelector("form")!.cloneNode(true) as HTMLElement;
  copy.querySelector("#plan-more .plan-more-body")?.remove();
  copy.querySelector("#repo-popover")?.remove();
  return copy.textContent ?? "";
};
const items = (root: HTMLElement) => Array.from(root.querySelectorAll<HTMLElement>('#repo-menu [role^="menuitem"]'));
const key = (target: Element, name: string) => target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
describe("one-box plan form", () => {
  it("puts the request first, then one row with the repo chip and Plan it", () => {
    const { root } = renderPlan(fixturePlanCapabilities);
    const form = root.querySelector<HTMLFormElement>('form[data-form="start"]')!;
    expect(form.querySelector('label[for="objective"]')?.textContent).toBe("What do you want done?");
    const chip = form.querySelector<HTMLButtonElement>("#repo-chip")!;
    expect(form.querySelector("#objective")!.compareDocumentPosition(chip) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(chip.closest(".plan-controls")?.querySelector('button[value="plan"]')).not.toBeNull();
    expect(chip.textContent).toContain("plugins");
    expect(chip.getAttribute("aria-haspopup")).toBe("menu");
    expect(chip.getAttribute("aria-expanded")).toBe("false");
    expect(root.querySelector<HTMLElement>("#repo-popover")?.hidden).toBe(true);
    // No scrolling radio list: the picked repo travels as one hidden field.
    expect(form.querySelectorAll('input[type="radio"]')).toHaveLength(0);
    expect(form.querySelector<HTMLInputElement>('input[type="hidden"][name="repository"]')?.value).toBe("plugins");
    const submits = Array.from(form.querySelectorAll<HTMLButtonElement>('button[type="submit"]'));
    expect(submits.map(button => button.textContent)).toEqual(["Plan it"]);
    expect(root.querySelectorAll(".button.primary")).toHaveLength(1);
    expect(root.querySelector<HTMLDetailsElement>("#plan-more")?.open).toBe(false);
    // Every other control is in the repo menu or the closed More options.
    const outside = Array.from(form.querySelectorAll("input:not([type=hidden]),select,textarea")).filter(element => !element.closest("#repo-popover, #plan-more") && element.id !== "objective");
    expect(outside).toHaveLength(0);
    expect(root.querySelector("#plan-repo-error")?.hasAttribute("hidden")).toBe(true);
  });
  it("hides the header Plan work button and keeps Close on the composer grid", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    expect(root.querySelector("#action-start")).toBeNull();
    expect(root.querySelector("#action-close-editor")?.closest(".plan-composer")).not.toBeNull();
    expect(root.querySelector(".plan-composer .plan-card")).not.toBeNull();
    renderWorkbench(root, controller.state, null, false);
    expect(root.querySelector("#action-start")).not.toBeNull();
  });
  it("links one-time setup to read-only setup guidance and never enables running", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    const link = root.querySelector<HTMLButtonElement>('.plan-note .text-link[data-action="setup"]');
    expect(link?.textContent).toBe("one-time setup");
    expect(link?.type).toBe("button");
    renderWorkbench(root, controller.state, "settings", false);
    const setup = root.querySelector("#setup-running")?.closest("section");
    expect(setup?.textContent).toContain("Not set up yet");
    expect(setup?.querySelectorAll("button,input,select,form")).toHaveLength(0);
    expect(root.querySelector('button[value="start"]')).toBeNull();
  });
  it("uses plain words in the default view", () => {
    const { root } = renderPlan(fixturePlanCapabilities);
    const text = visibleText(root).toLowerCase();
    expect(root.querySelector(".footer")).toBeNull();
    for (const jargon of ["finish authority", "admission", "profile alias", "qualification", "qualified", "mandatory", "acceptance", "non-goal", "capacity"]) expect(text).not.toContain(jargon);
  });
  it("fills More options with the defaults Plan it would use", () => {
    const { root } = renderPlan(fixturePlanCapabilities);
    const value = (name: string) => root.querySelector<HTMLInputElement | HTMLSelectElement>(`[name="${name}"]`)?.value;
    expect([value("finish"), value("profile"), value("capacity"), value("wall_minutes"), value("repair_attempts"), value("done_when"), value("non_goals")]).toEqual(["local_candidate", "luna", "1", "30", "2", "", ""]);
    expect(root.querySelector('[name="done_when"]')?.closest("#plan-more")).not.toBeNull();
  });
  it("shows Start run only when the server says execution is eligible, and Plan it stays the primary", () => {
    const off = renderPlan({ ...fixturePlanCapabilities, execution: { eligible: false, reason: "qualification_unverified" } }).root;
    expect(off.querySelector('button[value="start"]')).toBeNull();
    expect(off.textContent).toContain("Running it needs a one-time setup.");
    const { root } = renderPlan({ ...fixturePlanCapabilities, execution: { eligible: true, reason: "qualified" } });
    const start = root.querySelector<HTMLButtonElement>('button[value="start"]');
    expect(start?.textContent).toBe("Start run");
    expect(start?.classList.contains("primary")).toBe(false);
    expect(root.querySelector('button[value="plan"]')?.classList.contains("primary")).toBe(true);
    expect(root.textContent).toContain("Nothing runs until you press Start run.");
    expect(root.textContent).not.toContain("one-time setup");
  });
  it("menu lists your repos, then GitHub repos with Connect, then + New repo…", () => {
    const { root } = renderPlan(fixturePlanCapabilities);
    const menu = root.querySelector("#repo-menu")!;
    expect(menu.getAttribute("role")).toBe("menu");
    expect(Array.from(menu.querySelectorAll('[role="group"]')).map(group => group.getAttribute("aria-label"))).toEqual(["Your repos", "On GitHub"]);
    expect(Array.from(menu.querySelectorAll<HTMLElement>('[role="menuitemradio"]')).map(item => [item.dataset.alias, item.getAttribute("aria-checked")])).toEqual([["plugins", "true"], ["luna-sandbox", "false"]]);
    expect(menu.querySelector('[data-alias="plugins"]')?.textContent).toContain("joshyorko/plugins");
    // The connected GitHub repo is the approved alias above, never a second row.
    expect(menu.querySelector('[data-github-repo="joshyorko/plugins"]')).toBeNull();
    expect(Array.from(menu.querySelectorAll<HTMLElement>('[data-repo-action="connect"]')).map(item => item.dataset.repository)).toEqual(["joshyorko/rcc", "joshyorko/ror"]);
    const blocked = menu.querySelector<HTMLElement>('[data-github-repo="joshyorko/archive-notes"]')!;
    expect(blocked.textContent).toContain("Not available");
    expect(blocked.getAttribute("aria-disabled")).toBe("true");
    expect(blocked.dataset.repoAction).toBeUndefined();
    expect(items(root).at(-1)?.textContent).toBe("New repo…");
    expect(root.querySelector("#new_repository")).toBeNull();
    expect(root.querySelector('[data-action="create-repository"]')).toBeNull();
    // Only the popover may scroll; the form itself has no inner scrolling list.
    expect(root.querySelector(".repo-options")).toBeNull();
  });
  it("degrades to approved repos only when the server offers no workspace tools", () => {
    const { root } = renderPlan({ ...fixtureWorkbench.capabilities, repositories: fixturePlanCapabilities.repositories });
    expect(root.querySelectorAll('[role="menuitemradio"]')).toHaveLength(2);
    expect(root.querySelector("[data-github-repo]")).toBeNull();
    expect(root.querySelector('[data-repo-action="new"]')).toBeNull();
    expect(root.querySelector('[data-action="repositories"]')).toBeNull();
  });
  it("keeps the operator-folder path inside More options, only when discovery is offered", () => {
    const { root } = renderPlan({ ...fixturePlanCapabilities, repository_onboarding: { enabled: true, approval: "local_operator" } });
    expect(root.querySelector('[data-action="repositories"]')?.closest("#plan-more")).not.toBeNull();
  });
  it("keeps the picked repo and limits finish options to what it allows", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    expect(Array.from(root.querySelectorAll<HTMLOptionElement>('select[name="finish"] option')).map(option => option.value)).toEqual(["local_candidate", "push", "pr"]);
    controller.pickRepository("luna-sandbox");
    renderWorkbench(root, controller.state, "start", false);
    expect(root.querySelector("#repo-chip")?.textContent).toContain("luna-sandbox");
    expect(root.querySelector<HTMLInputElement>('input[name="repository"]')?.value).toBe("luna-sandbox");
    expect(Array.from(root.querySelectorAll<HTMLOptionElement>('select[name="finish"] option')).map(option => option.value)).toEqual(["local_candidate"]);
    // A refresh re-render keeps the pick: the hidden field renders from state, not the old draft.
    renderWorkbench(root, controller.state, "start", false);
    expect(root.querySelector<HTMLInputElement>('input[name="repository"]')?.value).toBe("luna-sandbox");
  });
  it("+ New repo… shows one inline name field, no Create button, and only local finishes", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    controller.pickRepository(null);
    renderWorkbench(root, controller.state, "start", false);
    expect(root.querySelector("#repo-chip")?.textContent).toContain("New repo");
    const name = root.querySelector<HTMLInputElement>("#new_repository")!;
    expect(name.closest(".plan-controls")).not.toBeNull();
    expect(name.getAttribute("aria-describedby")).toBe("plan-repo-error");
    expect(root.querySelector<HTMLInputElement>('input[name="repository"]')?.value).toBe("");
    expect(Array.from(root.querySelectorAll<HTMLButtonElement>("form button")).map(button => button.textContent?.trim()).filter(text => /create/i.test(text ?? ""))).toEqual([]);
    expect(Array.from(root.querySelectorAll<HTMLOptionElement>('select[name="finish"] option')).map(option => option.value)).toEqual(["local_candidate"]);
    // No static helper text: the rules show up only as an error when the name is invalid.
    expect(root.querySelector("#plan-repo-error")?.hasAttribute("hidden")).toBe(true);
    expect(visibleText(root)).not.toContain("Lowercase letters");
  });
  it("adds a find box only for long lists and filters rows without reading anything", () => {
    const many = { ...fixturePlanCapabilities, repositories: Array.from({ length: 8 }, (_, index) => ({ alias: `repo-${index}`, max_finish: "local_candidate" as const })) };
    const { root } = renderPlan(many, false);
    const filter = root.querySelector<HTMLInputElement>("#repo_filter");
    expect(filter?.closest("#repo-popover")).not.toBeNull();
    filterRepositories(root, "repo-3");
    expect(items(root).filter(row => !row.hidden).map(row => row.textContent)).toEqual(["repo-3", "New repo…"]);
    expect(renderPlan(fixturePlanCapabilities, false).root.querySelector("#repo_filter")).toBeNull();
  });
  it("shows a quiet line when GitHub can't be read, and still plans with approved repos", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities, false);
    controller.state.githubRepos = githubRepositoryListSchema.parse(fixtureGithubRepositories({ available: false, reason: "rate_limited", repositories: [] }));
    renderWorkbench(root, controller.state, "start", false);
    expect(root.textContent).toContain("GitHub repos couldn't be read right now.");
    expect(root.querySelector<HTMLButtonElement>('button[value="plan"]')?.disabled).toBe(false);
  });
  it("keeps the request text and the new repo name when a creation error re-renders the form", () => {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    controller.pickRepository(null);
    renderWorkbench(root, controller.state, "start", false);
    root.querySelector<HTMLTextAreaElement>("#objective")!.value = "Keep this request after a creation error";
    root.querySelector<HTMLInputElement>("#new_repository")!.value = "moon-garden";
    controller.reportError("workspace creation failed");
    renderWorkbench(root, controller.state, "start", false);
    expect(root.querySelector<HTMLTextAreaElement>("#objective")?.value).toBe("Keep this request after a creation error");
    expect(root.querySelector<HTMLInputElement>("#new_repository")?.value).toBe("moon-garden");
    expect(root.textContent).toContain("workspace creation failed");
  });
});

describe("repo menu keyboard and focus", () => {
  function mounted() {
    const { root, controller } = renderPlan(fixturePlanCapabilities);
    const handlers = { pick: vi.fn((alias: string) => { controller.pickRepository(alias); renderWorkbench(root, controller.state, "start", false); }), newRepository: vi.fn(() => { controller.pickRepository(null); renderWorkbench(root, controller.state, "start", false); }), connect: vi.fn(), more: vi.fn() };
    const unbind = bindRepoMenu(root, handlers);
    return { root, controller, handlers, unbind, chip: () => root.querySelector<HTMLButtonElement>("#repo-chip")! };
  }
  it("opens from the chip (a native button, so Enter and Space click it) onto the checked repo", () => {
    const { root, chip, unbind } = mounted();
    expect(chip().tagName).toBe("BUTTON");
    expect(chip().type).toBe("button");
    chip().focus();
    chip().click();
    expect(isRepoMenuOpen(root)).toBe(true);
    expect(chip().getAttribute("aria-expanded")).toBe("true");
    expect(root.querySelector<HTMLElement>("#repo-popover")?.hidden).toBe(false);
    expect(document.activeElement).toBe(root.querySelector('[data-alias="plugins"]'));
    chip().click();
    expect(isRepoMenuOpen(root)).toBe(false);
    unbind();
  });
  it("moves with arrows, Home and End, wraps, and skips nothing that's visible", () => {
    const { root, chip, unbind } = mounted();
    chip().focus();
    key(chip(), "ArrowDown");
    const list = items(root);
    expect(document.activeElement).toBe(list[0]);
    key(list[0]!, "ArrowDown");
    expect(document.activeElement).toBe(list[1]);
    key(list[1]!, "End");
    expect(document.activeElement).toBe(list.at(-1));
    key(list.at(-1)!, "ArrowDown");
    expect(document.activeElement).toBe(list[0]);
    key(list[0]!, "ArrowUp");
    expect(document.activeElement).toBe(list.at(-1));
    key(list.at(-1)!, "Home");
    expect(document.activeElement).toBe(list[0]);
    unbind();
  });
  it("ArrowUp on the chip opens onto the last item", () => {
    const { chip, unbind } = mounted();
    chip().focus();
    key(chip(), "ArrowUp");
    expect(document.activeElement?.textContent).toBe("New repo…");
    unbind();
  });
  it("Escape closes and returns focus to the chip", () => {
    const { root, chip, unbind } = mounted();
    chip().click();
    key(document.activeElement!, "Escape");
    expect(isRepoMenuOpen(root)).toBe(false);
    expect(root.querySelector<HTMLElement>("#repo-popover")?.hidden).toBe(true);
    expect(document.activeElement).toBe(chip());
    unbind();
  });
  it("picking closes the menu, updates the chip and returns focus to it", () => {
    const { root, chip, handlers, unbind } = mounted();
    chip().click();
    root.querySelector<HTMLElement>('[data-alias="luna-sandbox"]')!.click();
    expect(handlers.pick).toHaveBeenCalledWith("luna-sandbox");
    expect(isRepoMenuOpen(root)).toBe(false);
    expect(chip().textContent).toContain("luna-sandbox");
    expect(document.activeElement).toBe(chip());
    unbind();
  });
  it("Connect hands off one repo; disabled rows do nothing", () => {
    const { root, chip, handlers, unbind } = mounted();
    chip().click();
    root.querySelector<HTMLElement>('[data-github-repo="joshyorko/archive-notes"]')!.click();
    expect(handlers.connect).not.toHaveBeenCalled();
    expect(isRepoMenuOpen(root)).toBe(true);
    root.querySelector<HTMLElement>('[data-repository="joshyorko/rcc"]')!.click();
    expect(handlers.connect).toHaveBeenCalledExactlyOnceWith("joshyorko/rcc");
    expect(isRepoMenuOpen(root)).toBe(false);
    unbind();
  });
  it("+ New repo… closes the menu and focuses the inline name field", () => {
    const { root, chip, handlers, unbind } = mounted();
    chip().click();
    root.querySelector<HTMLElement>('[data-repo-action="new"]')!.click();
    expect(handlers.newRepository).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(root.querySelector("#new_repository"));
    unbind();
  });
  it("stays open across a re-render, such as the GitHub list arriving", () => {
    const { root, controller, chip, unbind } = mounted();
    chip().click();
    renderWorkbench(root, controller.state, "start", false);
    expect(isRepoMenuOpen(root)).toBe(true);
    expect(root.querySelector<HTMLElement>("#repo-popover")?.hidden).toBe(false);
    unbind();
  });
  it("a click outside closes the menu without moving focus into it", () => {
    const { root, chip, unbind } = mounted();
    chip().click();
    root.querySelector<HTMLElement>("#objective")!.click();
    expect(isRepoMenuOpen(root)).toBe(false);
    unbind();
  });
});
