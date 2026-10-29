// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { preserveCanonicalTaskTitle, taskTitleUpdate } from "../src/domain";
import { renderWorkbench, toggleDisclosure } from "../src/view";
import { fixtureBackends, fixtureGraph, fixtureRun } from "./fixtures";

async function setup() {
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : fixtureGraph() }));
  const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
  controller.receiveInitial({ structuredContent: fixtureRun() }); controller.setConnected(true);
  await controller.loadGraph();
  controller.selectNode("task-owner");
  const root = document.createElement("div");
  const render = () => renderWorkbench(root, controller.state, null, false);
  render();
  return { root, controller, render, call };
}
describe("interactive task graph", () => {
  it("exposes node selection, proof and a labeled planning editor", async () => {
    const { root } = await setup();
    expect(root.querySelector('[data-action="graph-node"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector('[aria-label="Task details"]')?.textContent).toContain("Complete the objective");
    expect(root.querySelector("fieldset#dependencies legend")?.textContent).toBe("Waits on");
    expect(root.querySelector<HTMLButtonElement>('button[value="save"]')?.disabled).toBe(false);
    expect(root.querySelector('option[value="codex-cloud"]')).toBeNull();
    expect(root.querySelector<HTMLTextAreaElement>("textarea#task-title")?.readOnly).toBe(false);
    expect(root.querySelector<HTMLTextAreaElement>("textarea#task-title")?.maxLength).toBe(4000);
  });
  it("keeps the view but disables proposal/apply controls when stale", async () => {
    const { root, controller, render } = await setup();
    controller.state.graphStale = true; render();
    expect(root.querySelector('[data-action="graph-node"]')).not.toBeNull();
    expect(root.querySelector<HTMLButtonElement>('button[value="save"]')?.disabled).toBe(true);
  });
  it("keeps reconnect visible inside an open task sheet after disconnect", async () => {
    const { root, controller, render } = await setup();
    controller.setDisconnected("Connection lost.");
    render();
    const sheet = root.querySelector<HTMLElement>(".graph-inspector.task-sheet");
    expect(sheet?.querySelector(".sheet-offline")?.textContent).toContain("Offline. Showing saved details.");
    expect(sheet?.querySelector('[data-action="reconnect"]')?.textContent).toBe("Reconnect");
    expect(sheet?.querySelector('[data-action="close-task"]')).not.toBeNull();
    expect(root.querySelector(".mission .notice.error")).toBeNull();
    const editToggle = root.querySelector<HTMLElement>('[data-action="toggle-disclosure"]')!;
    toggleDisclosure(root, editToggle);
    const editor = root.querySelector<HTMLElement>('.edit-sheet[data-disclosure]:not([hidden])');
    expect(editor?.querySelector(".sheet-offline")).not.toBeNull();
    expect(editor?.querySelector('[data-action="close-edit-sheet"]')).not.toBeNull();
    expect(editor?.querySelector<HTMLButtonElement>('button[value="save"]')?.disabled).toBe(true);
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
    const review = root.querySelector('[aria-label="Review plan change"]')?.textContent ?? "";
    expect(review).toContain("Before");
    expect(review).toContain("After");
    // Titles and labels only: no node, target or change identifiers and no revision numbers.
    for (const internal of ["task-owner", "native-local", "revision"]) expect(review).not.toContain(internal);
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(false);
    controller.state.graph!.revision = 8; render();
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(true);
  });
  it("reviews title, task order and cleared location in one combined proposal", async () => {
    const { root, controller, render } = await setup();
    const node = controller.state.graph!.nodes.find(item => item.id === "task-owner")!;
    node.target_preference = "native-local";
    controller.state.graph!.nodes.push({ ...node, id: "task-a", title: "First task", dependencies: [], target_preference: null });
    controller.state.proposal = { id: "p", idempotency_key: "k", fingerprint: "hash", actor: "local_operator", base_revision: controller.state.graph!.revision - 1, subject: "subject-123", status: "proposed", applied_revision: null, change: { kind: "update_task", node_id: "task-owner", title: "Rename the task", dependencies: ["task-a"], target_id: null } };
    render();
    const review = root.querySelector<HTMLElement>('[aria-label="Review plan change"]')!;
    expect(review.querySelector("h2")?.textContent).toContain("Complete the objective");
    expect(Array.from(review.querySelectorAll(".change-diff dt"), label => label.textContent)).toEqual(["Before", "After"]);
    const sides = Array.from(review.querySelectorAll<HTMLElement>(".change-diff dd"), side => side.textContent);
    expect(sides[0]).toContain("TitleComplete the objective");
    expect(sides[0]).toContain("Where it runsLocal machine");
    expect(sides[1]).toContain("TitleRename the task");
    expect(sides[1]).toContain("Waits onFirst task");
    expect(sides[1]).toContain("Where it runsNo preference");
    expect(review.textContent).not.toMatch(/task-owner|task-a|native-local|revision \d/);
  });
  it("preserves an 1,500-character objective title when proposing only a dependency change", async () => {
    const { root, controller, render, call } = await setup();
    const longTitle = `\nObjective intro\n${"A".repeat(1480)}\n`;
    const graph = controller.state.graph!;
    graph.nodes[0] = { ...graph.nodes[0]!, id: "objective", title: longTitle.slice(0, 1000), editor_title: longTitle, dependencies: [], target_preference: null };
    graph.nodes.push({ ...graph.nodes[0]!, id: "support-task", title: "Supporting task", editor_title: undefined, dependencies: [], target_preference: null });
    controller.selectNode("objective"); render();
    const title = root.querySelector<HTMLTextAreaElement>("#task-title")!;
    expect(title.value).toBe(longTitle);
    expect(title.maxLength).toBe(8000);
    expect(root.querySelector(".drawer-heading h3")?.textContent).toBe(longTitle.slice(0, 1000));
    expect(preserveCanonicalTaskTitle(title.value, longTitle)).toBe(longTitle);
    root.querySelector<HTMLInputElement>('input[name="dependencies"][value="support-task"]')!.checked = true;
    const form = root.querySelector<HTMLFormElement>('form[data-form="graph-node"]')!;
    const submitted = new FormData(form);
    const submittedTitle = taskTitleUpdate(form.querySelector<HTMLTextAreaElement>("#task-title")!.value, longTitle);
    expect(submittedTitle).toBeNull();
    call.mockClear();
    expect(await controller.proposeChange({ kind: "update_task", node_id: "objective", title: submittedTitle, dependencies: submitted.getAll("dependencies").map(String), target_id: null })).toBe(false);
    expect(call).toHaveBeenCalledWith("propose_factory_change", expect.objectContaining({ change: { kind: "update_task", node_id: "objective", title: null, dependencies: ["support-task"], target_id: null } }));
  });
  it("compares canonical long titles in the proposal without expanding the drawer heading", async () => {
    const { root, controller, render } = await setup();
    const before = `Objective intro\n${"A".repeat(1480)}\n`;
    const after = `Revised objective\n${"B".repeat(1484)}\n`;
    const node = controller.state.graph!.nodes[0]!;
    node.id = "objective";
    node.title = before.slice(0, 1000);
    node.editor_title = before;
    controller.state.graph!.revision = 7;
    controller.selectNode("objective");
    controller.state.proposal = { id: "p", idempotency_key: "k", fingerprint: "hash", actor: "local_operator", base_revision: 6, subject: "subject-123", status: "proposed", applied_revision: null, change: { kind: "update_task", node_id: "objective", title: after, dependencies: [], target_id: null } };
    render();
    const review = root.querySelector<HTMLElement>('[aria-label="Review plan change"]')!;
    const sides = Array.from(review.querySelectorAll<HTMLElement>(".change-diff dd"), side => side.textContent ?? "");
    expect(sides[0]).toContain(before);
    expect(sides[1]).toContain(after);
    expect(review.querySelector("h2")?.textContent?.length).toBeLessThan(1100);
    expect(root.querySelector(".drawer-heading h3")?.textContent).toBe(before.slice(0, 1000));
  });
  it("keeps a withheld safe-summary title private and sends null until it is replaced", async () => {
    const { root, controller, render, call } = await setup();
    const node = controller.state.graph!.nodes[0]!;
    node.id = "objective";
    node.title = "Title hidden";
    node.editor_title = null;
    controller.state.graph!.nodes.push({ ...node, id: "support-task", title: "Supporting task", editor_title: undefined, dependencies: [], target_preference: null });
    controller.selectNode("objective"); render();
    const title = root.querySelector<HTMLTextAreaElement>("#task-title")!;
    expect(title.value).toBe("");
    expect(title.placeholder).toBe("Title hidden for privacy");
    expect(root.textContent).not.toContain("secret");
    expect(taskTitleUpdate(title.value, node.editor_title)).toBeNull();
    expect(taskTitleUpdate("Replacement title", node.editor_title)).toBe("Replacement title");
    root.querySelector<HTMLInputElement>('input[name="dependencies"][value="support-task"]')!.click();
    const form = root.querySelector<HTMLFormElement>('form[data-form="graph-node"]')!;
    const submitted = new FormData(form);
    call.mockClear();
    await controller.proposeChange({ kind: "update_task", node_id: "objective", title: taskTitleUpdate(title.value, node.editor_title), dependencies: submitted.getAll("dependencies").map(String), target_id: null });
    expect(call).toHaveBeenCalledWith("propose_factory_change", expect.objectContaining({ change: expect.objectContaining({ kind: "update_task", title: null }) }));
  });
  it("shows an applied change as a quiet, dismissible note without IDs or revisions", async () => {
    const { root, controller, render } = await setup();
    const applied = { id: "75d01e55-9b57-450c-a0af-65afadca112c", idempotency_key: "k", fingerprint: "hash", actor: "local_operator" as const, base_revision: 8, subject: "subject-123", status: "applied" as const, applied_revision: 10, change: { kind: "set_target" as const, node_id: "task-owner", target_id: "native-local" } };
    controller.state.proposal = applied; render();
    const note = root.querySelector(".plan-updated");
    expect(note?.textContent).toContain("Plan updated");
    expect(note?.textContent).toContain("Complete the objective prefers Local machine.");
    for (const internal of ["75d01e55", "revision", "10", "8", "native-local"]) expect(note?.textContent).not.toContain(internal);
    expect(root.querySelector('[aria-label="Review plan change"]')).toBeNull();
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
    const editor = root.querySelector<HTMLElement>(".planning-editor")!;
    const visible = inspector.cloneNode(true) as HTMLElement;
    visible.querySelector(".planning-editor")?.remove();
    expect(visible.textContent).toContain("Done when");
    expect(visible.textContent).not.toContain("Parent issue");
    expect(visible.textContent).not.toContain("Not running yet");
    expect(inspector.querySelector(".drawer-heading p")?.textContent).toContain("Step 2");
    for (const jargon of ["admission", "execution authority", "Planning note", "native-local", "Summarize this run", "Help me choose"]) expect(visible.textContent).not.toContain(jargon);
    expect(inspector.querySelectorAll('[data-action="chat-follow-up"]')).toHaveLength(1);
    expect(inspector.querySelector('[data-action="chat-follow-up"]')?.textContent).toBe("Ask in chat");
    expect(editor.querySelector("details")).toBeNull();
    expect(editor.hidden).toBe(true);
    const toggle = inspector.querySelector<HTMLElement>('[data-action="toggle-disclosure"]')!;
    expect(toggle.textContent).toBe("Edit");
    toggleDisclosure(root, toggle);
    expect(editor.hidden).toBe(false);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(editor.querySelector('[role="dialog"]')).not.toBeNull();
    expect(editor.querySelector('[value="save"]')?.textContent).toContain("Save changes");
    // The open editor survives a server-driven rerender.
    render();
    expect(root.querySelector<HTMLElement>(".planning-editor")?.hidden).toBe(false);
  });
});
