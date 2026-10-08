import { describe, expect, it, vi } from "vitest";
import { submitNewRun } from "../src/submission";

describe("new run submission intent", () => {
  it.each([undefined, "", "plan", "other", "START"])("only creates a plan for intent %s", async intent => {
    const controller = { createGraph: vi.fn(async () => true), mutate: vi.fn(async () => true) };
    const args = { objective: "Plan this work", idempotency_key: "k" };
    expect(await submitNewRun(controller, intent, args)).toBe(true);
    expect(controller.createGraph).toHaveBeenCalledExactlyOnceWith(args);
    expect(controller.mutate).not.toHaveBeenCalled();
  });
  it("starts execution only for the explicit start action", async () => {
    const controller = { createGraph: vi.fn(async () => true), mutate: vi.fn(async () => true) };
    const args = { objective: "Run this work", idempotency_key: "k" };
    await submitNewRun(controller, "start", args);
    expect(controller.mutate).toHaveBeenCalledExactlyOnceWith("start_factory", args);
    expect(controller.createGraph).not.toHaveBeenCalled();
  });
});
