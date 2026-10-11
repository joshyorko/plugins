// @vitest-environment jsdom
// #69 attribution/created_at/display and #70 typed blockers, as the UI consumes them.
import { describe, expect, it, vi } from "vitest";
import { roster } from "../src/agents";
import { WorkbenchController, type Bridge } from "../src/controller";
import { classifyRun, graphEnvelopeSchema, needsOperatorDecision, parseToolResult, type RunView } from "../src/domain";
import { sourceReference } from "../src/graph-view";
import { renderLanes } from "../src/lanes";
import { blockerCopy, runTier, SinceTracker } from "../src/narrative";
import { renderWorkbench } from "../src/view";
import { fixtureBackends, fixtureBlockerKinds, fixtureCampaignPlan, fixtureGraph, fixturePlanningRun, fixtureRun, fixtureSwarmRun, fixtureWorkbench } from "./fixtures";

const parsedRun = (run: unknown): RunView => {
  const data = parseToolResult({ structuredContent: run });
  if (data.kind !== "run") throw new Error("Expected a run");
  return data.value;
};
function renderRun(run: RunView) {
  const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [run], selected_run: run } });
  controller.setConnected(true);
  // Inspect the run ledger's first task; selection is view state only.
  controller.state.selectedNodeId = run.control?.tasks[0]?.id ?? null;
  const root = document.createElement("div");
  renderWorkbench(root, controller.state, null, false);
  return { root, controller };
}
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

describe("additive projection fields at the server boundary", () => {
  it("keeps older payloads valid without the new fields", () => {
    const run = parsedRun(fixtureRun());
    expect(run.created_at).toBeUndefined();
    expect(run.activity_at).toBeUndefined();
    expect(run.presentation?.blocker_kind).toBeUndefined();
    expect(run.receipts[0]?.thread_id).toBeUndefined();
  });
  it("reads unknown or malformed blocker kinds as unknown and never guesses a category", () => {
    for (const value of [{ kind: "protected_resource", path: "x", permission: "write" }, { kind: "budget_exhausted" }, { kind: "budget_exhausted", budget: "tokens" }, "budget_exhausted"]) {
      const run = fixtureRun();
      (run.presentation as unknown as Record<string, unknown>).blocker_kind = value;
      expect(parsedRun(run).presentation?.blocker_kind).toEqual({ kind: "unknown" });
    }
    const run = fixtureRun();
    run.presentation!.blocker_kind = { kind: "budget_exhausted", budget: "repair" };
    expect(parsedRun(run).presentation?.blocker_kind).toEqual({ kind: "budget_exhausted", budget: "repair" });
  });
  it("treats a malformed receipt thread as unattributed and drops invalid display hints", () => {
    const run = fixtureRun({ receipts: [{ kind: "execution", summary: "x", subject: "s", created_at: 1, thread_id: "bad\nthread" }] });
    expect(parsedRun(run).receipts[0]?.thread_id).toBeNull();
    const envelope = fixtureGraph();
    envelope.graph.nodes[0]!.source = { provider: "github", repository_id: "r", item_id: "i", revision: "v", display: { number: 7, url: "https://evil.example/x" } };
    expect(graphEnvelopeSchema.parse(envelope).graph.nodes[0]?.source?.display).toBeUndefined();
    envelope.graph.nodes[0]!.source!.display = { number: 7, url: "https://github.com/joshyorko/plugins/issues/7" };
    expect(graphEnvelopeSchema.parse(envelope).graph.nodes[0]?.source?.display).toEqual({ number: 7, url: "https://github.com/joshyorko/plugins/issues/7" });
  });
});

describe("source display on the Campaign Map", () => {
  it("shows #N on map nodes and links it in the inspector only when display data exists", async () => {
    const { controller, root, render } = await campaign();
    controller.selectNode("issue-102"); render();
    expect(root.querySelector('[data-node-id="issue-102"] .node-ref')?.textContent).toBe("#102");
    // A button cannot contain a link, so the map shows the number and the inspector carries the link.
    expect(root.querySelector('[data-node-id="issue-102"] a')).toBeNull();
    const link = root.querySelector<HTMLAnchorElement>(".graph-inspector a.source-link");
    expect(link?.textContent).toBe("#102");
    expect(link?.getAttribute("href")).toBe("https://github.com/example-org/actions/issues/102");
    expect(link?.getAttribute("target")).toBe("_blank");
    expect(link?.getAttribute("rel")).toBe("noopener noreferrer");
    expect(root.querySelector(".graph-inspector .eyebrow")?.textContent).toContain("GitHub #102");
    // Without display data the provider-only label remains.
    expect(root.querySelector('[data-node-id="issue-104"] .node-ref')).toBeNull();
    controller.selectNode("issue-104"); render();
    expect(root.querySelector(".graph-inspector a.source-link")).toBeNull();
    expect(root.querySelector(".graph-inspector .eyebrow")?.textContent).toMatch(/GitHub$/);
  });
  it("escapes display data and never treats it as proof", async () => {
    const { controller, root, render } = await campaign();
    const node = controller.state.graph!.nodes.find(item => item.id === "issue-102")!;
    expect(node.state).toBe("candidate");
    expect(controller.state.graph!.criteria.every(criterion => criterion.status !== "proven")).toBe(true);
    expect(sourceReference({ provider: "github", repository_id: "r", item_id: "i", revision: "v" })).toBe("GitHub");
    expect(sourceReference({ provider: "github", repository_id: "r", item_id: "i", revision: "v", display: { number: 5 } })).toBe("GitHub #5");
    expect(sourceReference({ provider: "github", repository_id: "r", item_id: "i", revision: "v", display: { url: 'https://github.com/a/b/issues/5"onmouseover="x' } })).not.toContain('"onmouseover');
    render();
    expect(root.querySelector(".group-needs, .needs-you")).toBeNull();
  });
});

describe("receipt attribution in Lanes", () => {
  it("places attributed receipts on the matching agent lane and keeps the rest on Run receipts", () => {
    const run = fixtureSwarmRun();
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    const root = document.createElement("div");
    root.innerHTML = renderLanes(run, roster(run, null), controller.state, (run.updated_at ?? 0) + 60);
    const lanes = Array.from(root.querySelectorAll(".lane-list > li"));
    const ticks = (index: number) => lanes[index]?.querySelectorAll(".tick").length;
    expect(lanes.map(lane => lane.querySelector("strong")?.textContent)).toEqual(["Run receipts", "Luna", "Worker 1", "Worker 2"]);
    expect([ticks(0), ticks(1), ticks(2), ticks(3)]).toEqual([1, 2, 2, 0]);
    expect(lanes[3]?.querySelector(".lane-track.unavailable")?.textContent).toBe("No receipts attributed to this agent");
    expect(root.textContent).toContain("recorded on that agent's thread");
  });
  it("keeps receipts naming an unlisted thread unattributed and older servers on the run lane", () => {
    const run = fixtureSwarmRun();
    run.receipts = run.receipts.map(receipt => ({ ...receipt, thread_id: receipt.thread_id ? "not-in-roster" : null }));
    const controller = new WorkbenchController({ call: async () => ({}), context: async () => undefined }, () => undefined);
    const root = document.createElement("div");
    root.innerHTML = renderLanes(run, roster(run, null), controller.state);
    expect(root.querySelector(".run-lane")?.querySelectorAll(".tick").length).toBe(5);
    const legacy = fixtureSwarmRun();
    legacy.receipts = legacy.receipts.map(({ thread_id: _thread, ...receipt }) => receipt);
    root.innerHTML = renderLanes(legacy, roster(legacy, null), controller.state);
    expect(root.querySelector(".run-lane")?.querySelectorAll(".tick").length).toBe(5);
    expect(root.querySelectorAll(".lane-track.unavailable")).toHaveLength(3);
    expect(root.textContent).toContain("Per-agent timeline not reported by this server");
  });
});

describe("run timing", () => {
  it("shows Started from created_at, Created for plans, and nothing when absent", () => {
    const started = renderRun(fixtureRun({ created_at: 1791136800 })).root.querySelector(".campaign-head .started");
    expect(started?.textContent).toMatch(/^Started /);
    expect(started?.querySelector("time")?.getAttribute("datetime")).toBe(new Date(1791136800 * 1000).toISOString());
    const plan = fixturePlanningRun(); plan.created_at = 1791136800;
    expect(renderRun(plan).root.querySelector(".campaign-head .started")?.textContent).toMatch(/^Created /);
    expect(renderRun(fixtureRun()).root.querySelector(".campaign-head .started")).toBeNull();
  });
  it("lets the since-tracker see receipt activity reported as activity_at", () => {
    let now = 1_000;
    const tracker = new SinceTracker(() => now);
    const run = fixtureRun({ updated_at: 900, activity_at: 950, receipts: [] });
    tracker.observe([run]);
    now = 1_200;
    const later = { ...run, activity_at: 1_150, receipts: [{ kind: "execution", summary: "New receipt", subject: "s", created_at: 1_150, thread_id: null }] };
    expect(tracker.changes(later)?.lines).toEqual([expect.stringContaining("New receipt")]);
    expect(tracker.changes({ ...run, receipts: [] })).toBeNull();
  });
});

describe("typed blocker copy", () => {
  it("renders plain language for every kind and nothing for none or planning", () => {
    const copy = fixtureBlockerKinds.map(kind => { const run = fixtureRun(); run.presentation!.blocker_kind = kind; return [kind.kind === "budget_exhausted" ? `budget:${kind.budget}` : kind.kind, blockerCopy(run)]; });
    expect(Object.fromEntries(copy)).toEqual({
      "budget:time": "Time budget used up", "budget:repair": "Repair budget used up", diagnosis_required: "A diagnosis is needed before another repair",
      native_approval: "Waiting on an approval in native Codex", effect_outcome_unknown: "An effect's outcome is unknown", liveness_unknown: "Owned execution isn't proved stopped yet",
      planning_only: null, none: null, unknown: "Blocker not categorized by the server",
    });
  });
  it("names the blocker in the decision panel and inspector", () => {
    const run = fixtureRun({ state: "BLOCKED", blocker: "Observed native failure repair budget exhausted.", pending_decision: null });
    run.presentation!.blocker_kind = { kind: "budget_exhausted", budget: "repair" };
    const { root } = renderRun(run);
    expect(root.querySelector(".decision-panel .eyebrow")?.textContent).toBe("Repair budget used up");
    expect(root.querySelector(".decision-panel h2")?.textContent).toBe("Observed native failure repair budget exhausted.");
    expect(root.querySelector(".graph-inspector .blocker-kind")?.textContent).toBe("Repair budget used up");
    run.presentation!.blocker_kind = { kind: "unknown" };
    expect(renderRun(run).root.querySelector(".decision-panel .eyebrow")?.textContent).toBe("Execution blocker");
  });
  it("never changes attention, tier, Needs you or actions", () => {
    const bases = [
      fixtureRun({ state: "BLOCKED", pending_decision: null }),
      fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick one?" } }),
      fixtureRun({ state: "RUNNING" }),
      fixturePlanningRun(),
    ];
    const approval = fixtureRun({ state: "NEEDS_INPUT", pending_decision: null });
    approval.presentation!.primary_action = { kind: "inspect", label: "Open native Codex", reason: "native_approval_requires_native_ui", tool: "get_factory_run", allowed: true };
    bases.push(approval);
    const denied = fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "d", question: "Pick one?" } });
    denied.presentation!.primary_action = { ...denied.presentation!.primary_action, allowed: false };
    bases.push(denied);
    const shape = (root: HTMLElement) => ({
      panel: root.querySelector(".decision-panel")?.className, needs: root.querySelectorAll(".needs-you, .group-needs").length,
      controls: Array.from(root.querySelectorAll<HTMLButtonElement>('[data-action="control"]')).map(button => `${button.dataset.kind}:${button.disabled}`),
    });
    for (const base of bases) {
      const expected = { needs: needsOperatorDecision(base), classify: classifyRun(base), tier: runTier(base), view: shape(renderRun(base).root) };
      for (const kind of fixtureBlockerKinds) {
        const run = structuredClone(base);
        run.presentation!.blocker_kind = kind;
        const parsed = parsedRun(run);
        expect({ needs: needsOperatorDecision(parsed), classify: classifyRun(parsed), tier: runTier(parsed), view: shape(renderRun(parsed).root) }).toEqual(expected);
      }
    }
  });
});
