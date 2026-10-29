// @vitest-environment jsdom
// The task editor can clear a saved location preference through a nullable full-snapshot update.
import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { targetOptions } from "../src/graph-view";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureCampaignPlan } from "./fixtures";

async function campaign() {
  const plan = fixtureCampaignPlan();
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => ({ structuredContent: tool === "get_factory_backends" ? fixtureBackends : tool === "get_factory_run" ? plan.run : plan.graph }));
  const controller = new WorkbenchController({ call, context: vi.fn<Bridge["context"]>().mockResolvedValue() }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: plan.run });
  await controller.loadGraph();
  const root = document.createElement("div");
  return { controller, root, call, render: () => renderWorkbench(root, controller.state, null, false) };
}

describe("planning target note (#73)", () => {
  const eligible = [{ id: "native-local", label: "Native local", planning_eligible: true }];
  it("offers No preference as the clearable choice and preserves unavailable current values", () => {
    const root = document.createElement("select");
    root.innerHTML = targetOptions(null, eligible);
    const placeholder = root.querySelector('option[value=""]') as HTMLOptionElement;
    expect(placeholder.textContent).toBe("No preference");
    expect(placeholder.disabled).toBe(false);
    expect(placeholder.selected).toBe(true);
    root.innerHTML = targetOptions("native-local", eligible);
    expect(Array.from(root.options).map(option => option.value)).toEqual(["", "native-local"]);
    expect(root.querySelector<HTMLOptionElement>('option[value=""]')?.disabled).toBe(false);
    root.innerHTML = targetOptions("retired-target", eligible);
    expect(root.querySelector<HTMLOptionElement>('option[value="retired-target"]')?.disabled).toBe(true);
    expect(root.querySelector<HTMLOptionElement>('option[value=""]')?.disabled).toBe(false);
    root.innerHTML = targetOptions(null, []);
    expect(Array.from(root.options).map(option => option.value)).toEqual([""]);
    expect(root.options[0]?.disabled).toBe(false);
  });
  it("sends one full task update with a cleared location", async () => {
    const { controller, root, call, render } = await campaign();
    controller.state.graph!.nodes.find(node => node.id === "issue-102")!.target_preference = "native-local";
    controller.selectNode("issue-102"); render();
    const form = root.querySelector<HTMLFormElement>('form[data-form="graph-node"]')!;
    const target = root.querySelector<HTMLSelectElement>("#target_id")!;
    expect(target.querySelector<HTMLOptionElement>('option[value=""]')?.disabled).toBe(false);
    form.querySelector<HTMLTextAreaElement>("#task-title")!.value = "Rename the task";
    form.querySelector<HTMLInputElement>('input[name="dependencies"][value="issue-103"]')!.checked = true;
    target.value = "";
    call.mockClear();
    const change = {
      kind: "update_task" as const,
      node_id: "issue-102",
      title: form.querySelector<HTMLTextAreaElement>("#task-title")!.value,
      dependencies: Array.from(form.querySelectorAll<HTMLInputElement>('input[name="dependencies"]:checked'), input => input.value),
      target_id: target.value || null,
    };
    await controller.proposeChange(change);
    expect(call.mock.calls.filter(([tool]) => tool === "propose_factory_change")).toHaveLength(1);
    expect(call).toHaveBeenCalledWith("propose_factory_change", expect.objectContaining({ change: { kind: "update_task", node_id: "issue-102", title: "Rename the task", dependencies: ["issue-103"], target_id: null } }));
  });
});
