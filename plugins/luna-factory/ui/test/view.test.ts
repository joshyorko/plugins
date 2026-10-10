// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderWorkbench, type Editor } from "../src/view";
import { WorkbenchController } from "../src/controller";
import { fixtureGraph, fixtureRun, fixtureWorkbench } from "./fixtures";

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
    expect(main?.textContent?.indexOf("What changed")).toBeLessThan(main?.textContent?.indexOf("Execution evidence") ?? 0);
    expect(root.querySelector('[data-kind="answer"]')).not.toBeNull();
    expect(root.textContent).not.toContain("Force");
  });
  it("shows explicit ChatGPT follow-up buttons only when the host advertises message sending", () => {
    const { root, controller } = render(null, fixtureRun({ state: "NEEDS_INPUT", blocker: "Choose a safe scope", pending_decision: { id: "decision-1", question: "Choose a safe scope" } }));
    renderWorkbench(root, controller.state, null, false, { surface: "global", canSendFollowUps: true });
    const buttons = root.querySelectorAll<HTMLButtonElement>('[data-action="chat-follow-up"]');
    expect(buttons.length).toBeGreaterThanOrEqual(3);
    expect(root.textContent).toContain("after your click");
    expect(root.textContent).toContain("does not start Factory work");
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
    expect(root.querySelector('[data-action="expand-mode"]')?.textContent).toContain("Review in Luna Factory");
    renderWorkbench(root, controller.state, null, false, { surface: "thread", displayMode: "fullscreen" });
    expect(root.querySelector('.workbench[data-surface="thread"]')).not.toBeNull();
    expect(root.querySelector(".thread-inspector-label")?.textContent).toContain("Thread inspector");
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
    for (const field of root.querySelectorAll("input,select,textarea")) expect(root.querySelector(`label[for="${field.id}"]`)).not.toBeNull();
    expect(root.querySelector('select[name="repository"]')?.textContent).toContain("plugins");
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
    expect(root.textContent).toContain("No repositories configured");
    expect(root.querySelector('[data-action="start"]')?.hasAttribute("disabled")).toBe(true);
    expect(root.querySelector('[data-action="repositories"]')?.textContent).toContain("Add repository");
  });
  it("allows repository onboarding from settings even with no approved repositories", () => {
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities: { ...fixtureWorkbench.capabilities, repositories: [] } } });
    controller.setConnected(true);
    const root = document.createElement("div");
    renderWorkbench(root, controller.state, "settings", false);
    expect(root.querySelector('[data-action="repositories"]')?.textContent).toContain("Add repository");
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
  it("puts objective, change, Josh's next action, and proof counts in order", () => {
    const { root } = render(null, fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "decision-1", question: "Keep the migration local?" } }));
    const text = root.querySelector("main")?.textContent ?? "";
    expect(text.indexOf("Make Luna Factory a first-class workbench")).toBeLessThan(text.indexOf("What changed"));
    expect(text.indexOf("What changed")).toBeLessThan(text.indexOf("Needs Josh"));
    expect(text.indexOf("Needs Josh")).toBeLessThan(text.indexOf("Proven"));
    expect(text).toContain("Proven");
    expect(text).toContain("Failed");
    expect(text).toContain("Evidence pending");
    expect(root.querySelectorAll(".needs-josh .button.primary")).toHaveLength(1);
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
    expect(root.textContent).toContain("worker-retained: unknown");
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
    expect(root.querySelector(".needs-josh .button.primary")?.textContent).toBe("Wait for observed work");
    const secondary = root.querySelector('[data-kind="steer"]');
    expect(secondary?.closest("details")?.textContent).toContain("Other available actions");
  });
  it("hides machine reason codes behind plain copy and keeps legacy action read-only", () => {
    const run = fixtureRun({ blocker: null });
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.primary_action = { ...run.presentation.primary_action, kind: "reconcile", label: "Reconcile existing work", reason: "owned_liveness_unknown", tool: "reconcile_factory_run", allowed: true };
    run.presentation.actions = [run.presentation.primary_action];
    const { root } = render(null, run);
    expect(root.textContent).toContain("Owned execution has not been proved stopped.");
    expect(root.textContent).not.toContain("owned_liveness_unknown");
    const legacy = fixtureRun();
    delete legacy.control;
    delete legacy.presentation;
    const oldRoot = render(null, legacy).root;
    expect(oldRoot.textContent).toContain("Outcome unverified");
    expect(oldRoot.querySelector(".needs-josh .button.primary")?.textContent).toBe("Refresh current state");
    expect(oldRoot.querySelector('[data-kind="cancel"]')).toBeNull();
  });
});
