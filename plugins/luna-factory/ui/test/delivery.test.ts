// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { DELIVERY_INTERVAL_MS, WorkbenchController, type Bridge } from "../src/controller";
import { deliverySchema } from "../src/domain";
import { runTier } from "../src/narrative";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureCampaignPlan, fixtureDelivery, fixtureDeliveryUnavailable, fixtureGraph, fixtureWorkbench } from "./fixtures";

const github = { configured: true, reason: "configured_reachability_unverified" };
async function campaign(options: { capabilities?: typeof github | null; delivery?: unknown; fail?: boolean } = {}) {
  const plan = fixtureCampaignPlan();
  const capabilities = options.capabilities === null ? fixtureWorkbench.capabilities : { ...fixtureWorkbench.capabilities, github: options.capabilities ?? github };
  const workbench = { ...fixtureWorkbench, runs: [plan.run], selected_run: plan.run, capabilities };
  const call = vi.fn<Bridge["call"]>().mockImplementation(async tool => {
    if (tool === "read_factory_delivery") {
      if (options.fail) return { isError: true, content: [{ type: "text", text: "GitHub could not be reached" }] };
      return { structuredContent: options.delivery ?? fixtureDelivery(plan.run.id) };
    }
    return { structuredContent: tool === "get_factory_backends" ? fixtureBackends : tool === "get_factory_run" ? plan.run : tool === "refresh_factory" ? workbench : plan.graph };
  });
  const controller = new WorkbenchController({ call, context: vi.fn<Bridge["context"]>().mockResolvedValue() }, () => undefined);
  controller.setConnected(true);
  controller.receiveInitial({ structuredContent: workbench });
  await controller.loadGraph();
  return { controller, call, plan };
}
function render(controller: WorkbenchController, node?: string): HTMLElement {
  if (node) controller.selectNode(node);
  const root = document.createElement("div");
  renderWorkbench(root, controller.state, null, false);
  return root;
}
const deliveryCalls = (call: ReturnType<typeof vi.fn>) => call.mock.calls.filter(([tool]) => tool === "read_factory_delivery");
afterEach(() => vi.useRealTimers());

describe("GitHub-reported delivery contract", () => {
  it("parses only the read_factory_delivery shape, without proof or merge authority", () => {
    expect(deliverySchema.safeParse(fixtureDelivery()).success).toBe(true);
    expect(deliverySchema.safeParse(fixtureGraph()).success).toBe(false);
    expect(deliverySchema.safeParse({ ...fixtureDelivery(), proof: "github_checks" }).success).toBe(false);
    const merge = fixtureDelivery();
    (merge.nodes[1]!.pull_requests[0] as { merge_authority: boolean }).merge_authority = true;
    expect(deliverySchema.safeParse(merge).success).toBe(false);
  });

  it("reads delivery once per plan load and at most once a minute, only when the app is configured", async () => {
    vi.useFakeTimers({ now: 1_000_000, toFake: ["Date"] });
    const { controller, call } = await campaign();
    expect(deliveryCalls(call)).toEqual([["read_factory_delivery", { run_id: "campaign-actions-v2" }]]);
    expect(controller.state.delivery?.nodes).toHaveLength(13);
    await controller.loadDelivery();
    expect(deliveryCalls(call)).toHaveLength(1);
    vi.setSystemTime(1_000_000 + DELIVERY_INTERVAL_MS);
    await controller.loadDelivery();
    expect(deliveryCalls(call)).toHaveLength(2);

    const unconfigured = await campaign({ capabilities: { configured: false, reason: "github_app_not_configured" } });
    expect(deliveryCalls(unconfigured.call)).toHaveLength(0);
    const legacy = await campaign({ capabilities: null });
    expect(deliveryCalls(legacy.call)).toHaveLength(0);
    expect(render(legacy.controller, "issue-109").querySelector(".github-report, .pr-chip")).toBeNull();
  });

  it("keeps a failed read quiet: no error banner, no data and nothing shown", async () => {
    const { controller, call } = await campaign({ fail: true });
    expect(deliveryCalls(call)).toHaveLength(1);
    expect(controller.state.error).toBeNull();
    expect(controller.state.delivery).toBeNull();
    const root = render(controller, "issue-109");
    expect(root.querySelector(".pr-chip, .github-report")).toBeNull();
    const malformed = await campaign({ delivery: fixtureGraph() });
    expect(malformed.controller.state.delivery).toBeNull();
    expect(malformed.controller.state.error).toBeNull();
  });

  it("shows PR chips with check dots only for nodes GitHub reported a pull request for", async () => {
    const { controller } = await campaign();
    const root = render(controller);
    const chips = Array.from(root.querySelectorAll<HTMLElement>(".map-node .pr-chip"));
    expect(chips.map(chip => chip.closest(".map-node")!.getAttribute("data-node-id"))).toEqual(["issue-103", "issue-105", "issue-109", "issue-111"]);
    const chip = (id: string) => root.querySelector<HTMLElement>(`[data-node-id="${id}"] .pr-chip`)!;
    expect(chip("issue-103").classList.contains("ready")).toBe(true);
    expect(chip("issue-103").querySelectorAll(".check-dot.c-success")).toHaveLength(3);
    expect(chip("issue-109").querySelectorAll(".check-dot.c-failure")).toHaveLength(1);
    expect(chip("issue-109").querySelectorAll(".check-dot.c-pending")).toHaveLength(1);
    expect(chip("issue-105").classList.contains("ready")).toBe(false);
    expect(chip("issue-111").classList.contains("stale")).toBe(true);
    expect(chip("issue-111").classList.contains("ready")).toBe(false);
    expect(chip("issue-103").textContent).toContain("Reported by GitHub");
    // The coral "needs" hue and Needs you stay reserved for real operator decisions.
    expect(root.querySelector(".pr-chip .needs, .pr-chip [class*='needs']")).toBeNull();
  });

  it("separates Ready for review from merge and never changes proof or attention", async () => {
    const { controller, plan } = await campaign();
    const ready = render(controller, "issue-103").querySelector<HTMLElement>(".github-report")!;
    expect(ready.textContent).toContain("Reported by GitHub");
    expect(ready.textContent).toContain("Ready for review");
    expect(ready.textContent).toContain("review approved");
    expect(ready.textContent).toContain("+412");
    expect(ready.textContent).toContain("9 files");
    expect(ready.textContent).toContain("not Luna proof");
    expect(ready.textContent).not.toMatch(/ready to (merge|integrate)|mergeable|merge now/i);

    const failing = render(controller, "issue-109");
    const section = failing.querySelector<HTMLElement>(".github-report")!;
    expect(section.textContent).toContain("Cache key mismatch on windows-latest");
    expect(section.textContent).toContain("changes requested");
    expect(section.textContent).not.toContain("Ready for review");
    // Luna's own proof is untouched by GitHub's report.
    expect(failing.querySelector(".graph-proof")?.textContent).toContain("Evidence pending");
    expect(runTier(plan.run)).toBe(runTier(controller.selected!));
    const proofWithout = (await campaign({ capabilities: null })).controller;
    expect(render(proofWithout, "issue-103").querySelector(".graph-proof")?.textContent).toBe(render(controller, "issue-103").querySelector(".graph-proof")?.textContent);
    expect(render(proofWithout, "issue-103").querySelector(".decision, .needs-you")?.outerHTML).toBe(render(controller, "issue-103").querySelector(".decision, .needs-you")?.outerHTML);
  });

  it("shows stale data with its age and unavailable data with its reason", async () => {
    const { controller } = await campaign();
    const stale = render(controller, "issue-111").querySelector<HTMLElement>(".github-report")!;
    expect(stale.textContent).toContain("Stale · observed 20 min ago");
    expect(stale.textContent).toContain("Ready for review (as last observed)");
    expect(render(controller, "issue-113").querySelector(".github-report")?.textContent).toContain("no linked pull request");

    const limited = await campaign({ delivery: fixtureDeliveryUnavailable() });
    const unavailable = render(limited.controller, "issue-109").querySelector<HTMLElement>(".github-report")!;
    expect(unavailable.textContent).toContain("Unavailable: GitHub's rate limit was reached.");
    expect(unavailable.textContent).toContain("about 15 min");
    expect(unavailable.textContent).not.toContain("rate_limited");
    const kept = render(limited.controller, "issue-103").querySelector<HTMLElement>(".github-report")!;
    expect(kept.textContent).toContain("Stale · observed 10 min ago");
    expect(limited.controller.state.error).toBeNull();
  });

  it("drops the report when another run is selected", async () => {
    const { controller } = await campaign();
    expect(controller.state.delivery).not.toBeNull();
    await controller.select("run-123");
    expect(controller.state.delivery).toBeNull();
  });
});
