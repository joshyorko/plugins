// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { WorkbenchController } from "../src/controller";
import { renderWorkbench, toggleDisclosure } from "../src/view";
import { fixtureBackends, fixtureGraph, fixtureRun } from "./fixtures";

async function setup() {
  const controller = new WorkbenchController({ call: async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : fixtureGraph() }), context: async () => undefined }, () => undefined);
  controller.receiveInitial({ structuredContent: fixtureRun() }); controller.setConnected(true);
  await controller.loadGraph();
  const root = document.createElement("div");
  const render = () => renderWorkbench(root, controller.state, null, false);
  render();
  return { root, controller, render };
}
describe("interactive task graph", () => {
  it("exposes node selection, proof and a labeled planning editor", async () => {
    const { root } = await setup();
    expect(root.querySelector('[data-action="graph-node"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector('[aria-label="Selected task inspector"]')?.textContent).toContain("Complete the objective");
    expect(root.querySelector("fieldset#dependencies legend")?.textContent).toBe("Waits on");
    expect(root.querySelector<HTMLButtonElement>('button[value="dependencies"]')?.disabled).toBe(false);
    expect(root.querySelector('option[value="codex-cloud"]')).toBeNull();
    expect(root.textContent).toContain("does not authorize execution or verify subscription access");
  });
  it("keeps the view but disables proposal/apply controls when stale", async () => {
    const { root, controller, render } = await setup();
    controller.state.graphStale = true; render();
    expect(root.querySelector('[data-action="graph-node"]')).not.toBeNull();
    expect(root.querySelector<HTMLButtonElement>('button[value="target"]')?.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>('button[value="dependencies"]')?.disabled).toBe(true);
  });
  it("resets inspector fields when switching nodes and escapes imported content", async () => {
    const { root, controller, render } = await setup();
    controller.state.graph!.nodes.push({ ...controller.state.graph!.nodes[0]!, id: "second", title: '<img src=x onerror="alert(1)">', dependencies: ["task-owner"] });
    controller.selectNode("second"); render();
    expect(Array.from(root.querySelectorAll<HTMLInputElement>("#dependencies input:checked")).map(input => input.value)).toEqual(["task-owner"]);
    expect(root.querySelector(".graph-inspector img")).toBeNull();
    controller.selectNode("task-owner"); render();
    expect(root.querySelectorAll("#dependencies input:checked")).toHaveLength(0);
  });
  it("shows the exact proposed scope before enabling apply", async () => {
    const { root, controller, render } = await setup();
    controller.state.proposal = { id: "p", idempotency_key: "k", fingerprint: "hash", actor: "local_operator", base_revision: 6, subject: "subject-123", status: "proposed", applied_revision: null, change: { kind: "set_target", node_id: "task-owner", target_id: "native-local" } };
    render();
    const review = root.querySelector('[aria-label="Review graph change"]')?.textContent ?? "";
    expect(review).toContain("Complete the objective prefers Native local.");
    // Titles and labels only: no node, target or change identifiers and no revision numbers.
    for (const internal of ["task-owner", "native-local", "revision"]) expect(review).not.toContain(internal);
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(true);
    root.querySelector<HTMLInputElement>("#confirm-graph-change")!.checked = true; render();
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(false);
    controller.state.graph!.revision = 8; render();
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(true);
  });
  it("shows an applied change as a quiet, dismissible note without IDs or revisions", async () => {
    const { root, controller, render } = await setup();
    const applied = { id: "75d01e55-9b57-450c-a0af-65afadca112c", idempotency_key: "k", fingerprint: "hash", actor: "local_operator" as const, base_revision: 8, subject: "subject-123", status: "applied" as const, applied_revision: 10, change: { kind: "set_target" as const, node_id: "task-owner", target_id: "native-local" } };
    controller.state.proposal = applied; render();
    const note = root.querySelector(".plan-updated");
    expect(note?.textContent).toContain("Plan updated");
    expect(note?.textContent).toContain("Complete the objective prefers Native local.");
    for (const internal of ["75d01e55", "revision", "10", "8", "native-local"]) expect(note?.textContent).not.toContain(internal);
    expect(root.querySelector('[aria-label="Review graph change"]')).toBeNull();
    controller.dismissAppliedChange(); render();
    expect(root.querySelector(".plan-updated")).toBeNull();
    // A later read of the same applied change keeps it dismissed.
    (controller as unknown as { acceptGraph(envelope: unknown): void }).acceptGraph({ graph: controller.state.graph, proposal: applied }); render();
    expect(root.querySelector(".plan-updated")).toBeNull();
  });
  it("keeps the inspector in plain language with one ask action and a plain Edit toggle", async () => {
    const { root, controller, render } = await setup();
    controller.state.graph!.nodes.push({ ...controller.state.graph!.nodes[0]!, id: "planned", title: "Planned follow-up", state: "CANDIDATE", admission: "candidate", reason: "planning_candidate_no_authority", owner_thread: null, attempt_ids: [], dependencies: ["task-owner"], target_preference: "native-local" });
    controller.selectNode("planned"); render();
    const inspector = root.querySelector<HTMLElement>(".graph-inspector")!;
    const editor = inspector.querySelector<HTMLElement>(".planning-editor")!;
    const visible = inspector.cloneNode(true) as HTMLElement;
    visible.querySelector(".planning-editor")?.remove();
    for (const heading of ["Status", "What it waits on", "What it unblocks", "Done when"]) expect(visible.textContent).toContain(heading);
    expect(visible.textContent).not.toContain("Parent issue");
    expect(visible.textContent).not.toContain("Not running yet");
    expect(inspector.querySelector(".eyebrow")?.textContent).toContain("Step 2");
    for (const jargon of ["admission", "execution authority", "Planning note", "native-local", "Summarize this run", "Help me choose"]) expect(visible.textContent).not.toContain(jargon);
    expect(inspector.querySelectorAll('[data-action="chat-follow-up"]')).toHaveLength(1);
    expect(inspector.querySelector('[data-action="chat-follow-up"]')?.textContent).toBe("Ask ChatGPT about this");
    expect(inspector.querySelector("details")).toBeNull();
    expect(editor.hidden).toBe(true);
    const toggle = inspector.querySelector<HTMLElement>('[data-action="toggle-disclosure"]')!;
    expect(toggle.textContent).toBe("Edit");
    toggleDisclosure(root, toggle);
    expect(editor.hidden).toBe(false);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    // The open editor survives a server-driven rerender.
    render();
    expect(root.querySelector<HTMLElement>(".planning-editor")?.hidden).toBe(false);
  });
});
