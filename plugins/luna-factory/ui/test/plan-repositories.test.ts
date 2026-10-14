import { describe, expect, it, vi } from "vitest";
import { WorkbenchController, type Bridge } from "../src/controller";
import { fixtureGithubConnect, fixtureGithubRepositories, fixtureGraph, fixtureLocalCreate, fixturePlanCapabilities, fixtureWorkbench } from "./fixtures";

/** Synthetic server: the GitHub list, connect and create follow the github-repos contract. */
function server(capabilities: Record<string, unknown> = fixturePlanCapabilities, overrides: Partial<Record<string, (args: Record<string, unknown>) => unknown>> = {}) {
  let approved = [...(capabilities.repositories as Array<{ alias: string; max_finish: string }>)];
  const call = vi.fn<Bridge["call"]>(async (tool, args) => {
    const override = overrides[tool];
    if (override) { const result = override(args); if (result !== undefined) return result; }
    if (tool === "refresh_factory") return { structuredContent: { ...fixtureWorkbench, runs: [], selected_run: null, capabilities: { ...capabilities, repositories: approved } } };
    if (tool === "list_factory_campaigns") return { structuredContent: { campaigns: [], skipped: [] } };
    if (tool === "list_factory_github_repositories") return { structuredContent: fixtureGithubRepositories() };
    if (tool === "connect_factory_github_repository") { approved = [...approved, { alias: "rcc", max_finish: "local_candidate" }]; return { structuredContent: fixtureGithubConnect() }; }
    if (tool === "create_factory_local_repository") { approved = [...approved, { alias: String(args.name), max_finish: "local_candidate" }]; return { structuredContent: fixtureLocalCreate(String(args.name)) }; }
    throw new Error(`Unexpected tool ${tool}`);
  });
  const controller = new WorkbenchController({ call, context: vi.fn(async () => undefined) }, () => undefined);
  controller.receiveInitial({ structuredContent: { ...fixtureWorkbench, runs: [], capabilities } });
  controller.setConnected(true);
  return { controller, call };
}
const calls = (call: ReturnType<typeof vi.fn<Bridge["call"]>>, tool: string) => call.mock.calls.filter(([name]) => name === tool);

describe("plan form repositories", () => {
  it("never lists GitHub repos when the server doesn't offer GitHub connect", async () => {
    for (const capabilities of [fixtureWorkbench.capabilities, { ...fixturePlanCapabilities, workspaces: { connect_github: false, create_local: true } }]) {
      const { controller, call } = server(capabilities);
      await controller.loadGithubRepositories();
      expect(call).not.toHaveBeenCalled();
      expect(controller.state.githubRepos).toBeNull();
    }
  });
  it("lists GitHub repos read-only and keeps approved repos when the list fails", async () => {
    const { controller, call } = server();
    await controller.loadGithubRepositories();
    expect(call).toHaveBeenCalledExactlyOnceWith("list_factory_github_repositories", {});
    expect(controller.state.githubRepos?.repositories.map(repo => repo.full_name)).toContain("joshyorko/rcc");
    const missing = server(fixturePlanCapabilities, { list_factory_github_repositories: () => ({ isError: true, content: [{ type: "text", text: "Unknown tool" }] }) });
    await missing.controller.loadGithubRepositories();
    expect(missing.controller.state.githubRepos).toBeNull();
    expect(missing.controller.state.error).toBeNull();
    expect(missing.controller.state.capabilities?.repositories.map(repo => repo.alias)).toEqual(["plugins", "luna-sandbox"]);
  });
  it("rejects a list that claims supplied provenance instead of showing it", async () => {
    const { controller } = server(fixturePlanCapabilities, { list_factory_github_repositories: () => ({ structuredContent: fixtureGithubRepositories({ provenance: "supplied" }) }) });
    await controller.loadGithubRepositories();
    expect(controller.state.githubRepos).toBeNull();
  });
  it("pages with the server cursor and merges without duplicates", async () => {
    let page = 0;
    const { controller, call } = server(fixturePlanCapabilities, { list_factory_github_repositories: () => ({ structuredContent: page++ === 0 ? fixtureGithubRepositories({ next_cursor: "page-2" }) : fixtureGithubRepositories({ repositories: [{ ...fixtureGithubRepositories().repositories[1]! }, { full_name: "joshyorko/moon", private: false, default_branch: "main", description: null, connected_alias: null, connectable: true, blocked_reason: null }] }) }) });
    await controller.loadGithubRepositories();
    await controller.loadGithubRepositories(controller.state.githubRepos!.next_cursor!);
    expect(calls(call, "list_factory_github_repositories").map(([, args]) => args)).toEqual([{}, { cursor: "page-2" }]);
    expect(controller.state.githubRepos?.repositories.map(repo => repo.full_name).filter(name => name === "joshyorko/rcc")).toHaveLength(1);
    expect(controller.state.githubRepos?.repositories.at(-1)?.full_name).toBe("joshyorko/moon");
  });
  it("connects a GitHub repo, waits for the server to approve it, then picks it", async () => {
    const { controller, call } = server();
    await controller.loadGithubRepositories();
    expect(await controller.connectGithubRepository("joshyorko/rcc")).toBe(true);
    const [, args] = calls(call, "connect_factory_github_repository")[0]!;
    expect(Object.keys(args).sort()).toEqual(["idempotency_key", "repository"]);
    expect(args.repository).toBe("joshyorko/rcc");
    expect(controller.state.capabilities?.repositories.map(repo => repo.alias)).toContain("rcc");
    expect(controller.state.pickedRepository).toBe("rcc");
    expect(controller.state.notice).toContain("ready to plan");
    expect(controller.state.pending).toBeNull();
    // Connecting never plans, starts or dispatches anything.
    expect(call.mock.calls.map(([tool]) => tool).filter(tool => /start|graph|campaign_create|create_factory_graph/.test(tool))).toEqual([]);
  });
  it("refuses to connect repos the list marks as blocked, connected or unknown", async () => {
    const { controller, call } = server();
    await controller.loadGithubRepositories();
    for (const name of ["joshyorko/archive-notes", "joshyorko/plugins", "someone/else"]) expect(await controller.connectGithubRepository(name)).toBe(false);
    expect(calls(call, "connect_factory_github_repository")).toHaveLength(0);
  });
  it("reuses one idempotency key per repo until a connect succeeds", async () => {
    let fail = true;
    const { controller, call } = server(fixturePlanCapabilities, { connect_factory_github_repository: () => { if (fail) { fail = false; throw new Error("The host lost the reply"); } return { structuredContent: fixtureGithubConnect() }; } });
    await controller.loadGithubRepositories();
    expect(await controller.connectGithubRepository("joshyorko/rcc")).toBe(false);
    expect(controller.state.error).toContain("lost the reply");
    expect(await controller.connectGithubRepository("joshyorko/rcc")).toBe(true);
    const keys = calls(call, "connect_factory_github_repository").map(([, args]) => args.idempotency_key);
    expect(keys).toHaveLength(2);
    expect(keys[0]).toBe(keys[1]);
  });
  it("names a typed refusal in plain words and picks nothing", async () => {
    const { controller } = server(fixturePlanCapabilities, { connect_factory_github_repository: () => ({ isError: true, content: [{ type: "text", text: "github_connect_not_enabled" }] }) });
    await controller.loadGithubRepositories();
    expect(await controller.connectGithubRepository("joshyorko/rcc")).toBe(false);
    expect(controller.state.error).toBe("Connecting GitHub repos isn't switched on for this Luna yet.");
    expect(controller.state.pickedRepository).toBeNull();
    const structured = server(fixturePlanCapabilities, { create_factory_local_repository: () => ({ structuredContent: { reason: "local_create_not_enabled" } }) });
    expect(await structured.controller.createLocalRepository("moon-garden")).toBe(false);
    expect(structured.controller.state.error).toBe("Creating new repos isn't switched on for this Luna yet.");
  });
  it("rejects a connect reply for a different repo", async () => {
    const { controller } = server(fixturePlanCapabilities, { connect_factory_github_repository: () => ({ structuredContent: fixtureGithubConnect("joshyorko/ror", "ror") }) });
    await controller.loadGithubRepositories();
    expect(await controller.connectGithubRepository("joshyorko/rcc")).toBe(false);
    expect(controller.state.error).toContain("different repo");
  });
  it("creates a new empty repo by name and picks it once approved", async () => {
    const { controller, call } = server();
    expect(await controller.createLocalRepository("  moon-garden ")).toBe(true);
    const [, args] = calls(call, "create_factory_local_repository")[0]!;
    expect(args.name).toBe("moon-garden");
    expect(typeof args.idempotency_key).toBe("string");
    expect(controller.state.pickedRepository).toBe("moon-garden");
    expect(controller.state.notice).toBe("Created moon-garden. It's ready to plan.");
  });
  /** The new-repo path main.ts takes on Plan it: create first, then plan with the new alias. */
  const planWith = (controller: WorkbenchController, key = "plan-key") => (alias: string) => controller.createGraph({ repository: alias, objective: "Build the moon tool", idempotency_key: key });
  it("creates the new repo when Plan it is pressed, then plans in it", async () => {
    const { controller, call } = server(fixturePlanCapabilities, { create_factory_graph: args => ({ structuredContent: { ...fixtureGraph(), graph: { ...fixtureGraph().graph, repository: { ...fixtureGraph().graph.repository, alias: String(args.repository) } } } }) });
    controller.pickRepository(null);
    expect(controller.state.newRepository).toBe(true);
    expect(await controller.createRepositoryAndSubmit(" moon-garden ", planWith(controller))).toBe(true);
    const order = call.mock.calls.map(([tool]) => tool).filter(tool => tool === "create_factory_local_repository" || tool === "refresh_factory" || tool === "create_factory_graph");
    expect(order).toEqual(["create_factory_local_repository", "refresh_factory", "create_factory_graph"]);
    expect(calls(call, "create_factory_graph")[0]![1].repository).toBe("moon-garden");
    expect(controller.state.newRepository).toBe(false);
    expect(controller.state.pickedRepository).toBe("moon-garden");
  });
  it("retries a failed plan with the same key and without creating the repo again", async () => {
    let failPlan = true;
    const { controller, call } = server(fixturePlanCapabilities, {
      create_factory_graph: () => { if (failPlan) { failPlan = false; throw new Error("The host lost the plan reply"); } return { structuredContent: fixtureGraph() }; },
    });
    controller.pickRepository(null);
    expect(await controller.createRepositoryAndSubmit("moon-garden", planWith(controller))).toBe(false);
    expect(controller.state.error).toContain("lost the plan reply");
    // Still in new-repo mode, so the form keeps the name and the same request fingerprint.
    expect(controller.state.newRepository).toBe(true);
    expect(await controller.createRepositoryAndSubmit("moon-garden", planWith(controller))).toBe(true);
    expect(calls(call, "create_factory_local_repository")).toHaveLength(1);
    expect(calls(call, "create_factory_graph").map(([, args]) => [args.repository, args.idempotency_key])).toEqual([["moon-garden", "plan-key"], ["moon-garden", "plan-key"]]);
  });
  it("reuses the create key after a lost create reply and never plans until the repo exists", async () => {
    let fail = true;
    const { controller, call } = server(fixturePlanCapabilities, {
      create_factory_local_repository: () => { if (fail) { fail = false; throw new Error("The host lost the create reply"); } return undefined; },
      create_factory_graph: () => ({ structuredContent: fixtureGraph() }),
    });
    expect(await controller.createRepositoryAndSubmit("moon-garden", planWith(controller))).toBe(false);
    expect(calls(call, "create_factory_graph")).toHaveLength(0);
    expect(await controller.createRepositoryAndSubmit("moon-garden", planWith(controller))).toBe(true);
    const keys = calls(call, "create_factory_local_repository").map(([, request]) => request.idempotency_key);
    expect(keys).toHaveLength(2);
    expect(keys[0]).toBe(keys[1]);
  });
  it("does not plan when creation fails or the new repo isn't approved yet, and never falls back to another repo", async () => {
    const failed = server(fixturePlanCapabilities, { create_factory_local_repository: () => { throw new Error("workspace creation failed"); } });
    const submit = vi.fn(async () => true);
    expect(await failed.controller.createRepositoryAndSubmit("moon-garden", submit)).toBe(false);
    expect(failed.controller.state.error).toContain("workspace creation failed");
    const late = server(fixturePlanCapabilities, { create_factory_local_repository: () => ({ structuredContent: fixtureLocalCreate("late-repo") }), refresh_factory: () => ({ structuredContent: { ...fixtureWorkbench, runs: [], selected_run: null, capabilities: fixturePlanCapabilities } }) });
    late.controller.state.pickedRepository = "plugins";
    expect(await late.controller.createRepositoryAndSubmit("late-repo", submit)).toBe(false);
    expect(late.controller.state.error).toContain("hasn't listed it as ready yet");
    expect(submit).not.toHaveBeenCalled();
    for (const name of ["", "Moon", "../x"]) expect(await failed.controller.createRepositoryAndSubmit(name, submit)).toBe(false);
    expect(submit).not.toHaveBeenCalled();
  });
  it("picks only approved aliases, and new-repo mode only when creation is offered", () => {
    const { controller } = server();
    controller.pickRepository("luna-sandbox");
    expect(controller.state.pickedRepository).toBe("luna-sandbox");
    controller.pickRepository("not-approved");
    expect(controller.state.pickedRepository).toBe("luna-sandbox");
    const off = server({ ...fixturePlanCapabilities, workspaces: { connect_github: true, create_local: false } });
    off.controller.pickRepository(null);
    expect(off.controller.state.newRepository).toBe(false);
  });
  it("validates new repo names locally and only when the server offers creation", async () => {
    const { controller, call } = server();
    for (const name of ["", "Moon", "../escape", "-dash", "a".repeat(65), "with space"]) expect(await controller.createLocalRepository(name)).toBe(false);
    expect(calls(call, "create_factory_local_repository")).toHaveLength(0);
    const off = server({ ...fixturePlanCapabilities, workspaces: { connect_github: true, create_local: false } });
    expect(await off.controller.createLocalRepository("moon-garden")).toBe(false);
    expect(calls(off.call, "create_factory_local_repository")).toHaveLength(0);
  });
  it("does not pick an alias the server hasn't approved yet", async () => {
    const { controller } = server(fixturePlanCapabilities, { create_factory_local_repository: () => ({ structuredContent: fixtureLocalCreate("late-repo") }), refresh_factory: () => ({ structuredContent: { ...fixtureWorkbench, runs: [], selected_run: null, capabilities: fixturePlanCapabilities } }) });
    expect(await controller.createLocalRepository("late-repo")).toBe(true);
    expect(controller.state.pickedRepository).toBeNull();
    expect(controller.state.notice).toContain("Refresh if it doesn't show up yet");
  });
});
