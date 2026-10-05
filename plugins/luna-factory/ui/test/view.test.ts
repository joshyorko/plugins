// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderWorkbench, type Editor } from "../src/view";
import { WorkbenchController } from "../src/controller";
import { fixtureRun, fixtureWorkbench } from "./fixtures";

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
    expect(main?.textContent?.indexOf("What changed")).toBeLessThan(main?.textContent?.indexOf("Evidence") ?? 0);
    expect(root.querySelector('[data-action="steer"]')).not.toBeNull();
    expect(root.textContent).not.toContain("Force");
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
    expect(root.textContent).toContain("Repository claim held");
    expect(root.querySelector('[data-action="resume"]')).toBeNull();
  });
  it("allows NEEDS_INPUT answers without an active turn and explains same-run continuation", () => {
    const run = fixtureRun({ state: "NEEDS_INPUT", turn_id: null, blocker: "Choose the bounded scope", pending_decision: { id: "decision-1", question: "Choose the bounded scope" } });
    const { root } = render(null, run);
    expect(root.querySelector('[data-action="steer"]')?.textContent).toBe("Answer the owner");
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
    expect(root.textContent).toContain("downstream execution and billing unverified");
    expect(root.textContent).toContain("1 native model mismatch recorded");
    expect(root.textContent).toContain("owner-123, turn-123");
    expect(root.textContent).toContain("Effort telemetry unavailable");
  });
});
