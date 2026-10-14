import { describe, expect, it } from "vitest";
import { boundedContext, capabilitiesSchema, classifyRun, githubConnectSchema, githubRepositoryListSchema, localCreateSchema, parseFactoryLink, parseRunLink, parseToolResult, planDefaults, repositoryReasonCopy, startRequest } from "../src/domain";
import { fixtureGithubConnect, fixtureGithubRepositories, fixtureLocalCreate, fixtureRun, fixtureWorkbench } from "./fixtures";

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
  it("keeps nondecision blockers and quiescent plans in history", () => {
    expect(classifyRun(fixtureRun({ state: "BLOCKED" }))).toBe("recent");
    expect(classifyRun(fixtureRun({ state: "QUIESCENT" }))).toBe("recent");
    expect(classifyRun(fixtureRun({ state: "RUNNING" }))).toBe("active");
    expect(classifyRun(fixtureRun({ state: "CONVERGED" }))).toBe("recent");
  });
  it("trusts the server result projection over a legacy terminal state", () => {
    const stopped = fixtureRun({ state: "CONVERGED" });
    if (!stopped.presentation) throw new Error("Missing fixture presentation");
    stopped.presentation.result = { kind: "stopped_unresolved", label: "Stopped with work unresolved" };
    expect(classifyRun(stopped)).toBe("recent");
    const legacy = fixtureRun({ state: "CONVERGED" });
    delete legacy.control;
    delete legacy.presentation;
    expect(classifyRun(legacy)).toBe("recent");
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
  it("restores exact tasks only with a revision fence", () => {
    expect(parseFactoryLink("/runs/run-123?task=issue-61&revision=7")).toEqual({ runId: "run-123", taskId: "issue-61", revision: 7 });
    for (const link of ["/runs/run-123?task=issue-61", "/runs/run-123?revision=7", "/runs/run-123?task=issue-61&revision=7&task=issue-62", "/runs/run-123?task=issue-61&revision=9007199254740992", "/runs/run-123?task=issue-61&revision=bad", "/runs/run-123?unknown=1"]) {
      expect(parseFactoryLink(link)).toBeNull();
    }
  });
  it.each(["//evil.test/runs/a", "https://evil.test/runs/a", "/runs/a/b", "/runs/a#b", "/runs/%2e%2e", "/runs/a%2Fb", "/runs/", "/runs/aaa%00", "/runs/a/", "/runs/%ZZ"])("rejects invalid or ambiguous route %s", (url) => {
    expect(parseRunLink(url)).toBeNull();
  });
});

describe("bounded Model-App Context", () => {
  it("shares only revision-fenced permitted fields with hard text bounds", () => {
    const context = boundedContext(fixtureRun({ objective: "x".repeat(8000), delta: "private receipt", blocker: "b".repeat(2000) }));
    expect(Object.keys(context)).toEqual(["run_id", "revision", "repository", "objective", "state", "current_subject", "remaining_mandatory_gap", "blocker", "finish"]);
    expect(JSON.stringify(context).length).toBeLessThan(3500);
    expect(JSON.stringify(context)).not.toContain("private receipt");
    expect(context.objective.length).toBeLessThanOrEqual(1200);
  });
});

describe("start request", () => {
  const fields = { repository: "plugins", objective: "Ship the workbench", done_when: "Build passes and tests pass", non_goals: "No deployment", finish: "local_candidate", profile: "luna", capacity: "2", repair_attempts: "1", wall_minutes: "10" };
  it("plans with sensible defaults from just a repository and a request", () => {
    const request = startRequest({ repository: "plugins", objective: "  Ship the workbench  " }, fixtureWorkbench.capabilities, "one-box");
    expect(request).toEqual({ repository: "plugins", objective: "Ship the workbench", acceptance: ["Ship the workbench"], non_goals: [], finish: "local_candidate", profile: "luna", capacity: 1, repair_attempts: 2, wall_seconds: 1800, idempotency_key: "one-box" });
  });
  it("uses the first approved profile and stays inside smaller server limits", () => {
    const capabilities = { ...fixtureWorkbench.capabilities, profiles: [{ alias: "quick", effort: "low" }, { alias: "luna", effort: "xhigh" }], limits: { capacity: 1, repair_attempts: 0, wall_seconds: 600 } };
    const request = startRequest({ repository: "plugins", objective: "Tidy the README" }, capabilities, "k");
    expect(request).toMatchObject({ profile: "quick", capacity: 1, repair_attempts: 0, wall_seconds: 600 });
    expect(planDefaults(capabilities)).toEqual({ finish: "local_candidate", profile: "quick", capacity: 1, wall_seconds: 600, repair_attempts: 0 });
  });
  it("makes the optional Done when line the single criterion", () => {
    expect(startRequest(fields, fixtureWorkbench.capabilities, "k").acceptance).toEqual(["Build passes and tests pass"]);
    expect(startRequest({ ...fields, done_when: "   " }, fixtureWorkbench.capabilities, "k").acceptance).toEqual(["Ship the workbench"]);
  });
  it("asks for a Done when line instead of truncating a long request into a criterion", () => {
    expect(() => startRequest({ repository: "plugins", objective: "x".repeat(1001) }, fixtureWorkbench.capabilities, "k")).toThrow("Done when");
    expect(startRequest({ repository: "plugins", objective: "x".repeat(1001), done_when: "It works" }, fixtureWorkbench.capabilities, "k").acceptance).toEqual(["It works"]);
  });
  it("does not turn pasted headings or bullet syntax into non-goals", () => {
    const request = startRequest({ ...fields, non_goals: "**Non-goals:**\n• No deployment\n- No merge" }, fixtureWorkbench.capabilities, "normalized-form");
    expect(request.non_goals).toEqual(["No deployment", "No merge"]);
  });
  it("constructs only real tool arguments from approved choices", () => {
    const request = startRequest({ ...fields, new_repository: "ignored", repo_filter: "plu" }, fixtureWorkbench.capabilities, "attempt-1");
    expect(request.wall_seconds).toBe(600);
    expect(request.capacity).toBe(2);
    expect(request.idempotency_key).toBe("attempt-1");
    expect(Object.keys(request).sort()).toEqual(["repository", "objective", "acceptance", "non_goals", "finish", "profile", "capacity", "repair_attempts", "wall_seconds", "idempotency_key"].sort());
  });
  it.each([{ repository: "../../private" }, { repository: "" }, { finish: "deploy" }, { capacity: "100" }, { profile: "custom-provider" }, { objective: " " }, { wall_minutes: "0" }])("rejects unsafe or out-of-bounds values", (change) => {
    expect(() => startRequest({ ...fields, ...change }, fixtureWorkbench.capabilities, "attempt-1")).toThrow();
  });
  it("never asks for more than a repository allows", () => {
    const capabilities = { ...fixtureWorkbench.capabilities, repositories: [{ alias: "plugins", max_finish: "local_candidate" as const }] };
    expect(() => startRequest({ ...fields, finish: "pr" }, capabilities, "k")).toThrow("can't finish that way");
  });
});

describe("repository tool results", () => {
  it("parses the synthetic list, connect and create shapes and rejects inconsistent ones", () => {
    expect(githubRepositoryListSchema.parse(fixtureGithubRepositories()).repositories).toHaveLength(4);
    expect(githubRepositoryListSchema.safeParse(fixtureGithubRepositories({ provenance: "supplied" })).success).toBe(false);
    expect(githubRepositoryListSchema.safeParse(fixtureGithubRepositories({ repositories: [{ ...fixtureGithubRepositories().repositories[0], full_name: "Joshyorko/Plugins" }] })).success).toBe(false);
    expect(githubConnectSchema.parse(fixtureGithubConnect()).alias).toBe("rcc");
    expect(githubConnectSchema.safeParse({ ...fixtureGithubConnect(), repository: { alias: "other", identity: "b".repeat(64) } }).success).toBe(false);
    expect(localCreateSchema.parse(fixtureLocalCreate()).status).toBe("created");
    expect(localCreateSchema.safeParse({ ...fixtureLocalCreate(), repository: { alias: "moon-garden", identity: "/home/private" } }).success).toBe(false);
  });
  it("names typed reasons in plain words and never shows unknown codes raw", () => {
    expect(repositoryReasonCopy("github_connect_not_enabled")).toContain("isn't switched on");
    expect(repositoryReasonCopy("local_create_not_enabled")).toContain("isn't switched on");
    expect(repositoryReasonCopy("workspace_path_conflict")).toContain("already exists");
    expect(repositoryReasonCopy("some_new_code")).not.toContain("some_new_code");
  });
  it("treats missing or malformed workspace switches as not offered", () => {
    const base = { ...fixtureWorkbench.capabilities };
    expect(capabilitiesSchema.parse(base).workspaces).toBeUndefined();
    expect(capabilitiesSchema.parse({ ...base, workspaces: { connect_github: "yes" } }).workspaces).toBeUndefined();
    expect(capabilitiesSchema.parse({ ...base, workspaces: { connect_github: true, create_local: false } }).workspaces).toEqual({ connect_github: true, create_local: false });
  });
});
