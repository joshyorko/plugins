// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { WorkbenchController } from "../src/controller";
import { renderWorkbench } from "../src/view";
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
    expect(root.querySelector('label[for="dependencies"]')).not.toBeNull();
    expect(root.querySelector<HTMLButtonElement>('button[value="dependencies"]')?.disabled).toBe(false);
    expect(root.querySelector<HTMLOptionElement>('option[value="codex-cloud"]')?.disabled).toBe(true);
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
    expect(root.querySelector<HTMLSelectElement>("#dependencies")?.selectedOptions[0]?.value).toBe("task-owner");
    expect(root.querySelector(".graph-inspector img")).toBeNull();
    controller.selectNode("task-owner"); render();
    expect(root.querySelector<HTMLSelectElement>("#dependencies")?.selectedOptions.length).toBe(0);
  });
  it("shows the exact proposed scope before enabling apply", async () => {
    const { root, controller, render } = await setup();
    controller.state.proposal = { id: "p", idempotency_key: "k", fingerprint: "hash", actor: "local_operator", base_revision: 6, subject: "subject-123", status: "proposed", applied_revision: null, change: { kind: "set_target", node_id: "task-owner", target_id: "native-local" } };
    render();
    expect(root.querySelector('[aria-label="Review graph change"]')?.textContent).toContain("task-owner: prefer native-local");
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(false);
    controller.state.graph!.revision = 8; render();
    expect(root.querySelector<HTMLButtonElement>('[data-action="apply-change"]')?.disabled).toBe(true);
  });
});
