import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { fixtureRun, fixtureWorkbench } from "./fixtures";

function deferred() {
  let resolve: (value: unknown) => void = () => undefined;
  let reject: (error: Error) => void = () => undefined;
  const promise = new Promise<unknown>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
function setup(call = vi.fn<Bridge["call"]>()) {
  const bridge: Bridge = { call, context: vi.fn(async () => undefined) };
  const controller = new WorkbenchController(bridge, () => undefined);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, selected_run: fixtureRun() } });
  return { controller, bridge, call };
}

describe("workbench state and MCP lifecycle", () => {
  it("uses the initial result without a duplicate read", () => {
    const { controller, call } = setup();
    expect(controller.state.runs).toHaveLength(1);
    expect(call).not.toHaveBeenCalled();
  });
  it("does not let repeated initial results overwrite later data", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: { ...fixtureWorkbench, runs: [fixtureRun({ state: "CONVERGED", updated_at: 5 })] } });
    await controller.refresh();
    controller.receiveInitial({ structuredContent: fixtureWorkbench });
    expect(controller.state.runs[0]?.state).toBe("CONVERGED");
  });
  it("preserves valid data after a malformed refresh", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: { runs: "nope" } });
    await controller.refresh();
    expect(controller.state.runs).toHaveLength(1);
    expect(controller.state.error).toContain("unrecognized");
    expect(controller.state.refreshing).toBe(false);
  });
  it("ignores late selections and refresh snapshots", async () => {
    const old = deferred();
    const fresh = deferred();
    const { controller, call } = setup();
    call.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
    const first = controller.select("run-old");
    const second = controller.select("run-new");
    fresh.resolve({ structuredContent: fixtureRun({ id: "run-new", objective: "New selection" }) });
    await second;
    old.resolve({ structuredContent: fixtureRun({ id: "run-old" }) });
    await first;
    expect(controller.selected?.id).toBe("run-new");
    expect(controller.state.error).toBeNull();
  });
  it("fences an in-flight read when a mutation begins", async () => {
    const old = deferred();
    const { controller, call } = setup();
    await controller.select("run-123");
    call.mockReturnValueOnce(old.promise).mockResolvedValueOnce({ structuredContent: fixtureRun({ state: "CANCELLED", claim_held: false, updated_at: 6 }) });
    const refresh = controller.refresh();
    await controller.mutate("cancel_factory_run", { run_id: "run-123" });
    old.resolve({ structuredContent: fixtureWorkbench });
    await refresh;
    expect(controller.selected?.state).toBe("CANCELLED");
  });
  it("keeps stop pending until the server reports an outcome and prevents duplicates", async () => {
    const result = deferred();
    const { controller, call } = setup();
    call.mockReturnValue(result.promise);
    const first = controller.mutate("cancel_factory_run", { run_id: "run-123" });
    expect(controller.state.pending?.tool).toBe("cancel_factory_run");
    expect(controller.state.runs[0]?.state).toBe("VERIFYING");
    expect(await controller.mutate("cancel_factory_run", { run_id: "run-123" })).toBe(false);
    result.resolve({ structuredContent: fixtureRun({ state: "BLOCKED", blocker: "Owned descendant status is unknown", claim_held: true, updated_at: 6 }) });
    await first;
    expect(controller.state.runs[0]?.state).toBe("BLOCKED");
    expect(controller.state.runs[0]?.claim_held).toBe(true);
    expect(controller.state.pending).toBeNull();
    expect(call).toHaveBeenCalledTimes(1);
  });
  it("preserves history and run identity on same-run resume", async () => {
    const { controller, call } = setup();
    await controller.select("run-123");
    call.mockResolvedValue({ structuredContent: fixtureRun({ state: "RUNNING", updated_at: 8 }) });
    await controller.mutate("resume_factory_run", { run_id: "run-123" });
    expect(call).toHaveBeenCalledWith("resume_factory_run", { run_id: "run-123" });
    expect(controller.selected?.receipts).toHaveLength(1);
    expect(controller.selected?.owner_thread).toBe("owner-123");
  });
  it("recovers pending state on failure without claiming cancellation", async () => {
    const { controller, call } = setup();
    call.mockRejectedValue(new Error("Connection lost"));
    expect(await controller.mutate("cancel_factory_run", { run_id: "run-123" })).toBe(false);
    expect(controller.state.pending).toBeNull();
    expect(controller.state.runs[0]?.state).toBe("VERIFYING");
    expect(controller.state.error).toContain("Refresh before retrying");
  });
  it("shares bounded context on selection and clears it on overview", async () => {
    const { controller, bridge } = setup();
    await controller.select("run-123");
    await vi.waitFor(() => expect(bridge.context).toHaveBeenCalledWith(expect.objectContaining({ run_id: "run-123" })));
    await controller.select(null);
    await vi.waitFor(() => expect(bridge.context).toHaveBeenLastCalledWith({}));
  });
  it("rejects a result for another run after a targeted mutation", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: fixtureRun({ id: "unexpected" }) });
    expect(await controller.mutate("cancel_factory_run", { run_id: "run-123" })).toBe(false);
    expect(controller.state.runs.map(run => run.id)).toEqual(["run-123"]);
  });
  it("loads full evidence when selecting a list summary", async () => {
    const call = vi.fn<Bridge["call"]>().mockResolvedValue({ structuredContent: fixtureRun() });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    const { receipts: _receipts, ...summary } = fixtureRun();
    controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [summary] } });
    expect(controller.state.initialized).toBe(true);
    await controller.select("run-123");
    expect(call).toHaveBeenCalledWith("get_factory_run", { run_id: "run-123" });
    expect(controller.selected?.receipts).toHaveLength(1);
  });
  it("does not claim a blocked resume succeeded", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: fixtureRun({ state: "BLOCKED", blocker: "Owned execution is still active" }) });
    expect(await controller.mutate("resume_factory_run", { run_id: "run-123" })).toBe(false);
    expect(controller.state.notice).toBeNull();
    expect(controller.state.error).toContain("Owned execution is still active");
  });
  it("answers completed NEEDS_INPUT turns by resuming the same run with input", async () => {
    const call = vi.fn<Bridge["call"]>().mockResolvedValue({ structuredContent: fixtureRun({ state: "RUNNING", turn_id: "new-turn" }) });
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: { ...fixtureRun({ state: "NEEDS_INPUT", turn_id: null }), pending_decision: { id: "decision-1", question: "Keep the migration local?" } } });
    expect(await controller.sendOwnerInput("  Keep the migration local  ")).toBe(true);
    expect(call).toHaveBeenCalledWith("resume_factory_run", { run_id: "run-123", message: "Keep the migration local", expected_decision_id: "decision-1" });
    expect(controller.selected?.owner_thread).toBe("owner-123");
    expect(controller.selected?.turn_id).toBe("new-turn");
  });
  it("does not submit native approvals as decision answers", async () => {
    const call = vi.fn<Bridge["call"]>();
    const controller = new WorkbenchController({ call, context: async () => undefined }, () => undefined);
    controller.receiveInitial({ structuredContent: fixtureRun({ state: "NEEDS_INPUT", blocker: "Approval required in Codex" }) });
    await expect(controller.sendOwnerInput("Approve")).rejects.toThrow("native Codex");
    expect(call).not.toHaveBeenCalled();
  });
  it("continues to fence active-turn corrections with the current turn ID", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: fixtureRun({ state: "RUNNING" }) });
    expect(await controller.sendOwnerInput("Keep the current acceptance")).toBe(true);
    expect(call).toHaveBeenCalledWith("steer_factory_run", { run_id: "run-123", expected_turn_id: "turn-123", message: "Keep the current acceptance" });
  });
  it.each(["", " ", "x".repeat(4001)])("rejects invalid owner input before sending", async message => {
    const { controller, call } = setup();
    await expect(controller.sendOwnerInput(message)).rejects.toThrow();
    expect(call).not.toHaveBeenCalled();
  });
  it("does not roll back a newer generation or update time", async () => {
    const { controller, call } = setup();
    call.mockResolvedValueOnce({ structuredContent: fixtureRun({ generation: 3, updated_at: 200, state: "CONVERGED" }) });
    await controller.mutate("resume_factory_run", { run_id: "run-123" });
    call.mockResolvedValueOnce({ structuredContent: { ...fixtureWorkbench, runs: [fixtureRun({ generation: 2, updated_at: 300, state: "RUNNING" })] } });
    await controller.refresh();
    expect(controller.selected?.state).toBe("CONVERGED");
    call.mockResolvedValueOnce({ structuredContent: { ...fixtureWorkbench, runs: [fixtureRun({ generation: 3, updated_at: 100, state: "RUNNING" })] } });
    await controller.refresh();
    expect(controller.selected?.state).toBe("CONVERGED");
  });
  it("saves only safe defaults and reads the server's validated values", async () => {
    const { controller, call } = setup();
    call.mockResolvedValue({ structuredContent: { schema: {}, values: { capacity: 3, profile: "luna", finish: "local_candidate" } } });
    expect(await controller.saveSettings({ capacity: 3 })).toBe(true);
    expect(call).toHaveBeenCalledWith("update_factory_settings", { capacity: 3 });
    expect(controller.state.settings.capacity).toBe(3);
    expect(controller.state.pending).toBeNull();
  });
});
