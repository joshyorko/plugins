import { describe, expect, it } from "vitest";
import { boundedContext, classifyRun, parseRunLink, parseToolResult, startRequest } from "../src/domain";
import { fixtureRun, fixtureWorkbench } from "./fixtures";

describe("the server boundary", () => {
  it("accepts the documented initial workbench and a single run", () => {
    expect(parseToolResult({ structuredContent: fixtureWorkbench }).kind).toBe("workbench");
    expect(parseToolResult({ structuredContent: fixtureRun() }).kind).toBe("run");
  });
  it.each([null, {}, { structuredContent: { runs: [] } }, { structuredContent: fixtureRun({ capacity: -1 }) }])("rejects malformed data without manufacturing a run", (result) => {
    expect(() => parseToolResult(result)).toThrow();
  });
  it("treats an MCP error as an error even if it contains structured data", () => {
    expect(() => parseToolResult({ isError: true, structuredContent: fixtureRun(), content: [{ type: "text", text: "repository_busy" }] })).toThrow("repository_busy");
  });
  it("keeps an empty authorized repository list empty", () => {
    const result = parseToolResult({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities: { ...fixtureWorkbench.capabilities, repositories: [] } } });
    expect(result.kind === "workbench" && result.value.capabilities.repositories).toEqual([]);
  });
  it("excludes explicitly unsupported profiles from run choices", () => {
    const result = parseToolResult({ structuredContent: { ...fixtureWorkbench, capabilities: { ...fixtureWorkbench.capabilities, profiles: [{ alias: "unsupported", effort: "xhigh", supported: false }, { alias: "luna", effort: "xhigh", supported: true }] } } });
    expect(result.kind === "workbench" && result.value.capabilities.profiles.map(profile => profile.alias)).toEqual(["luna"]);
  });
  it("rejects impossible timestamps before rendering", () => {
    expect(() => parseToolResult({ structuredContent: fixtureRun({ deadline_at: 1e30 }) })).toThrow();
  });
  it("rejects presentation revisions that do not match the control snapshot", () => {
    const run = fixtureRun();
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.revision += 1;
    expect(() => parseToolResult({ structuredContent: run })).toThrow("mismatched control revisions");
  });
  it("rejects unsafe JSON revision integers without coercion", () => {
    const run = fixtureRun();
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.revision = Number.MAX_SAFE_INTEGER + 1;
    expect(() => parseToolResult({ structuredContent: run })).toThrow();
  });
  it("rejects proof counts that disagree with criterion statuses", () => {
    const run = fixtureRun();
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.criteria.proven = 0;
    expect(() => parseToolResult({ structuredContent: run })).toThrow("inconsistent criterion counts");
  });
  it("rejects finished_verified while a mandatory criterion is unresolved", () => {
    const run = fixtureRun();
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.result = { kind: "finished_verified", label: "Finished and verified" };
    expect(() => parseToolResult({ structuredContent: run })).toThrow("unresolved criteria as finished");
  });
});

describe("operator attention", () => {
  it("keeps blocked and quiescent outcomes in Needs me", () => {
    expect(classifyRun(fixtureRun({ state: "BLOCKED" }))).toBe("needs");
    expect(classifyRun(fixtureRun({ state: "QUIESCENT" }))).toBe("needs");
    expect(classifyRun(fixtureRun({ state: "RUNNING" }))).toBe("active");
    expect(classifyRun(fixtureRun({ state: "CONVERGED" }))).toBe("recent");
  });
  it("trusts the server result projection over a legacy terminal state", () => {
    const stopped = fixtureRun({ state: "CONVERGED" });
    if (!stopped.presentation) throw new Error("Missing fixture presentation");
    stopped.presentation.result = { kind: "stopped_unresolved", label: "Stopped with work unresolved" };
    expect(classifyRun(stopped)).toBe("needs");
    const legacy = fixtureRun({ state: "CONVERGED" });
    delete legacy.control;
    delete legacy.presentation;
    expect(classifyRun(legacy)).toBe("needs");
  });
  it("does not export stale CONVERGED as model context", () => {
    const run = fixtureRun({ state: "CONVERGED" });
    if (!run.presentation) throw new Error("Missing fixture presentation");
    run.presentation.result = { kind: "unverified", label: "Outcome unverified" };
    expect(boundedContext(run).state).toBe("UNVERIFIED");
    delete run.presentation;
    delete run.control;
    expect(boundedContext(run).state).toBe("UNVERIFIED");
  });
});

describe("exact run links", () => {
  it("accepts only an app-relative exact run route", () => {
    expect(parseRunLink("/runs/run-123")).toBe("run-123");
    expect(parseRunLink("/runs/run-123?view=evidence")).toBe("run-123");
  });
  it.each(["//evil.test/runs/a", "https://evil.test/runs/a", "/runs/a/b", "/runs/a#b", "/runs/%2e%2e", "/runs/a%2Fb", "/runs/", "/runs/aaa%00", "/runs/a/", "/runs/%ZZ"])("rejects invalid or ambiguous route %s", (url) => {
    expect(parseRunLink(url)).toBeNull();
  });
});

describe("bounded Model-App Context", () => {
  it("shares only the eight permitted fields with hard text bounds", () => {
    const context = boundedContext(fixtureRun({ objective: "x".repeat(8000), delta: "private receipt", blocker: "b".repeat(2000) }));
    expect(Object.keys(context)).toEqual(["run_id", "repository", "objective", "state", "current_subject", "remaining_mandatory_gap", "blocker", "finish"]);
    expect(JSON.stringify(context).length).toBeLessThan(3500);
    expect(JSON.stringify(context)).not.toContain("private receipt");
    expect(context.objective.length).toBeLessThanOrEqual(1200);
  });
});

describe("start request", () => {
  const fields = { repository: "plugins", objective: "Ship the workbench", acceptance: "Build passes\nTests pass", non_goals: "No deployment", finish: "local_candidate", profile: "luna", capacity: "2", repair_attempts: "1", wall_seconds: "600" };
  it("constructs only real tool arguments from approved choices", () => {
    const request = startRequest(fields, fixtureWorkbench.capabilities, "attempt-1");
    expect(request.acceptance).toEqual(["Build passes", "Tests pass"]);
    expect(request.idempotency_key).toBe("attempt-1");
    expect(Object.keys(request).sort()).toEqual(["repository", "objective", "acceptance", "non_goals", "finish", "profile", "capacity", "repair_attempts", "wall_seconds", "idempotency_key"].sort());
  });
  it.each([{ repository: "../../private" }, { finish: "deploy" }, { capacity: "100" }, { profile: "custom-provider" }, { acceptance: " " }, { wall_seconds: "1" }])("rejects unsafe or out-of-bounds values", (change) => {
    expect(() => startRequest({ ...fields, ...change }, fixtureWorkbench.capabilities, "attempt-1")).toThrow();
  });
});
