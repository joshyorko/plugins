// @vitest-environment jsdom
// #73: the planning target note never submits an empty target ID.
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
  it("offers No preference only as a disabled placeholder while no note is set", () => {
    const root = document.createElement("select");
    root.innerHTML = targetOptions(null, eligible);
    const placeholder = root.querySelector('option[value=""]') as HTMLOptionElement;
    expect(placeholder.textContent).toBe("No preference");
    expect(placeholder.disabled && placeholder.selected).toBe(true);
    root.innerHTML = targetOptions("native-local", eligible);
    expect(Array.from(root.options).map(option => option.value)).toEqual(["native-local"]);
    expect(root.textContent).not.toContain("No preference");
    root.innerHTML = targetOptions("retired-target", eligible);
    expect(Array.from(root.options).filter(option => !option.disabled).map(option => option.value)).toEqual(["native-local"]);
    root.innerHTML = targetOptions(null, []);
    expect(Array.from(root.options).every(option => option.disabled)).toBe(true);
  });
  it("never submits an empty target from the editor or the controller", async () => {
    const { controller, root, call, render } = await campaign();
    controller.selectNode("issue-102"); render();
    const form = root.querySelector<HTMLFormElement>('form[data-form="graph-node"]')!;
    // No enabled option carries an empty value; whatever the placeholder submits is rejected below.
    expect(Array.from(root.querySelectorAll<HTMLOptionElement>("#target_id option")).filter(option => option.value === "" && !option.disabled)).toHaveLength(0);
    call.mockClear();
    // The same field mapping the submit handler in main.ts uses.
    const submitted = new FormData(form).get("target_id");
    expect(submitted === null || submitted === "").toBe(true);
    expect(await controller.proposeChange({ kind: "set_target", node_id: "issue-102", target_id: typeof submitted === "string" ? submitted : "" })).toBe(false);
    expect(call).not.toHaveBeenCalled();
    expect(controller.state.notice).toContain("Choose a planning target");
    controller.state.graph!.nodes.find(node => node.id === "issue-102")!.target_preference = "native-local"; render();
    expect(root.querySelector("#target_id")?.textContent).not.toContain("No preference");
    expect(await controller.proposeChange({ kind: "set_target", node_id: "issue-102", target_id: "" })).toBe(false);
    expect(call).not.toHaveBeenCalled();
  });
});
