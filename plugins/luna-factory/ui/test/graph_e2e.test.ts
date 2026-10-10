// @vitest-environment jsdom
// Opt-in integration: actual Rust/SQLite/HTTP and production UI controller, synthetic MCP App host.
import { createHash } from "node:crypto";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createServer } from "node:net";
import { once } from "node:events";
import { expect, it, vi } from "vitest";
import { App } from "@modelcontextprotocol/ext-apps";
import { AppBridge } from "@modelcontextprotocol/ext-apps/app-bridge";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { CallToolResultSchema } from "@modelcontextprotocol/sdk/types.js";
import { HostBridge } from "../src/bridge";
import { WorkbenchController } from "../src/controller";
import { classifyRun, graphEnvelopeSchema } from "../src/domain";
import { renderWorkbench } from "../src/view";

it.skipIf(process.env.LUNA_GRAPH_E2E !== "1")("operates an issue graph through model tools and the workbench with zero native dispatch", async () => {
  const directory = await mkdtemp(join(tmpdir(), "luna-graph-e2e-"));
  const portProbe = createServer();
  portProbe.listen(0, "127.0.0.1"); await once(portProbe, "listening");
  const address = portProbe.address();
  if (!address || typeof address === "string") throw new Error("Missing test port");
  const port = address.port;
  await new Promise<void>((done, reject) => portProbe.close(error => error ? reject(error) : done()));
  const endpoint = `http://127.0.0.1:${port}/mcp`;
  let daemon: ChildProcess | undefined;
  const app = new App({ name: "Factory integration", version: "test" }, {}, { autoResize: false });
  const host = new AppBridge(null, { name: "Synthetic host, not ChatGPT acceptance", version: "test" }, {
    serverTools: {}, updateModelContext: { structuredContent: {} }, message: { text: {} }, experimental: { "openai/modelContext": {}, "openai/message": {} },
  });
  const contexts: Record<string, unknown>[] = [];
  const calls: string[] = [];
  const messages: unknown[] = [];
  const root = document.createElement("div"); document.body.append(root);
  try {
    const source = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
    const repo = join(directory, "plugins");
    // Actual inspected engineering repository, isolated from all active worktrees and services.
    execFileSync("git", ["clone", "--quiet", "--shared", source, repo]);
    const snapshotBytes = await readFile(resolve("../tests/fixtures/github/issue-61.json"), "utf8");
    const issue = JSON.parse(snapshotBytes) as { node_id: string; title: string; repository_id: number; number: number; html_url: string };
    const issueRevision = `sha256:${createHash("sha256").update(snapshotBytes).digest("hex")}`;
    const config = join(directory, "config.json");
    await writeFile(config, JSON.stringify({
      listen: `127.0.0.1:${port}`, database: join(directory, "state", "runs.sqlite"),
      codex_binary: join(directory, "native-must-not-exist"), skill_path: join(directory, "SKILL.md"),
      repositories: { plugins: { root: repo, max_finish: "local_candidate" } },
      profiles: { default: { effort: "high" } }, limits: { capacity: 1, repair_attempts: 0, wall_seconds: 300 },
    }));
    daemon = spawn(resolve(process.env.LUNA_FACTORY_BINARY ?? "../server/target/debug/luna-factoryd"), ["serve", "--config", config, "--ui", resolve("dist/index.html")], { stdio: ["ignore", "pipe", "pipe"] });
    let stderr = ""; daemon.stderr?.on("data", data => { stderr += String(data); });
    const rpc = async (method: string, params: Record<string, unknown>) => {
      const response = await fetch(endpoint, { method: "POST", headers: {
        "content-type": "application/json", accept: "application/json, text/event-stream",
        "mcp-protocol-version": "2026-07-28", "mcp-method": method,
        ...(typeof params.name === "string" ? { "mcp-name": params.name } : typeof params.uri === "string" ? { "mcp-name": params.uri } : {}),
      }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params: { ...params, _meta: {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { name: "graph-e2e", version: "test" },
        "io.modelcontextprotocol/clientCapabilities": {},
      } } }) });
      const data = await response.json() as { result?: unknown; error?: unknown };
      if (!response.ok || data.error) throw new Error(JSON.stringify(data));
      return data.result;
    };
    await vi.waitFor(async () => {
      if (daemon?.exitCode !== null) throw new Error(`Fixture service exited: ${stderr}`);
      expect(await rpc("server/discover", {})).toBeTruthy();
    }, { timeout: 10_000, interval: 50 });
    const catalog = await rpc("tools/list", {}) as { tools: { name: string; _meta?: { ui?: { visibility?: string[] } } }[] };
    const modelTools = new Set(catalog.tools.filter(t => t._meta?.ui?.visibility?.includes("model") ?? true).map(t => t.name));
    for (const name of ["create_factory_graph", "get_factory_graph", "propose_factory_change", "apply_factory_change", "get_factory_backends"]) expect(modelTools.has(name)).toBe(true);
    const call = async (name: string, args: Record<string, unknown>) => {
      calls.push(name);
      return CallToolResultSchema.parse(await rpc("tools/call", { name, arguments: args }));
    };
    const model = async (name: string, args: Record<string, unknown>) => {
      expect(modelTools.has(name)).toBe(true);
      const result = await call(name, args);
      expect(result.isError, JSON.stringify(result)).not.toBe(true);
      return result.structuredContent;
    };
    const objectivePrefix = "Plan the existing bounded-continuation issue: ";
    const objective = objectivePrefix + "😀".repeat(700);
    const titleCases = [
      { id: "long-ascii-title", raw: "a".repeat(2000), public: "a".repeat(1000) },
      { id: "long-unicode-title", raw: "😀".repeat(1000), public: "😀".repeat(500) },
      { id: "unicode-boundary-title", raw: "a".repeat(999) + "😀", public: "a".repeat(999) },
    ];
    const created = graphEnvelopeSchema.parse(await model("create_factory_graph", {
      repository: "plugins", objective, acceptance: ["Retain execution identity and prove convergence"], non_goals: ["No execution in this planning graph"],
      finish: "local_candidate", profile: "default", capacity: 1, repair_attempts: 0, wall_seconds: 300, idempotency_key: "real-issue-plan",
    }));
    const runId = created.graph.run_id;
    const noop = await call("propose_factory_change", { run_id: runId, expected_revision: created.graph.revision, idempotency_key: "empty-prerequisites", change: { kind: "set_dependencies", node_id: "objective", dependencies: [] } });
    expect(noop.isError).toBe(true);
    expect(JSON.stringify(noop.content)).toContain("graph_change_noop");
    expect(graphEnvelopeSchema.parse(await model("get_factory_graph", { run_id: runId })).graph).toEqual(created.graph);
    const resources = await rpc("resources/list", {}) as { resources: { uri: string; mimeType?: string }[] };
    const uiResource = resources.resources.find(resource => resource.mimeType === "text/html;profile=mcp-app");
    if (!uiResource) throw new Error("Bundled UI resource missing");
    const resource = await rpc("resources/read", { uri: uiResource.uri }) as { contents: { text?: string }[] };
    expect(resource.contents[0]?.text).toContain("0.2.1");
    const proposal = graphEnvelopeSchema.parse(await model("propose_factory_change", {
      run_id: runId, expected_revision: created.graph.revision, idempotency_key: "import-issue-61", change: { kind: "import_candidates", nodes: [{
        id: "issue-61", title: issue.title, criterion_ids: ["A1"], dependencies: [],
        // Display hints from the recorded snapshot; never proof or identity.
        source: { provider: "github", repository_id: created.graph.repository.identity, item_id: `${issue.repository_id}:${issue.node_id}`, revision: issueRevision, display: { number: issue.number, url: issue.html_url } },
      }, ...titleCases.map(title => ({
        id: title.id, title: title.raw, criterion_ids: ["A1"], dependencies: [],
        source: { provider: "fixture", repository_id: created.graph.repository.identity, item_id: title.id, revision: "title-contract-v1" },
      }))] },
    }));
    // Proposals retain original import data; only the public task projection is shortened.
    expect(proposal.proposal?.change.kind).toBe("import_candidates");
    if (proposal.proposal?.change.kind !== "import_candidates") throw new Error("Missing import proposal");
    for (const title of titleCases) expect(proposal.proposal.change.nodes.find(node => node.id === title.id)?.title).toBe(title.raw);
    await model("apply_factory_change", { run_id: runId, expected_revision: proposal.graph.revision, change_id: proposal.proposal?.id });
    host.oncalltool = params => call(params.name, params.arguments ?? {});
    host.onupdatemodelcontext = async params => {
      contexts.push(params.structuredContent ?? {});
      return { _meta: { "openai/modelContext": { updateId: `e2e-${contexts.length}` } } };
    };
    host.onmessage = async params => { messages.push(params); return {}; };
    const bridge = new HostBridge(app, (id, node, revision) => { void controller.restoreContext(id, node, revision); });
    const controller = new WorkbenchController(bridge, () => renderWorkbench(root, controller.state, null, false));
    app.ontoolresult = result => controller.receiveInitial(result);
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    await host.connect(hostTransport); await bridge.connect(appTransport); controller.setConnected(true);
    await host.sendToolInput({ arguments: {} });
    await host.sendToolResult(await call("open_factory", {}));
    await controller.select(runId); await controller.loadGraph();
    expect(controller.state.error).toBeNull();
    expect(controller.selected && classifyRun(controller.selected)).toBe("recent");
    expect(root.querySelector(".decision-panel .eyebrow")?.textContent).toBe("Plan navigation");
    for (const title of titleCases) {
      expect(controller.state.graph?.nodes.find(node => node.id === title.id)?.title).toBe(title.public);
      expect(controller.selected?.control?.tasks.find(node => node.id === title.id)?.title).toBe(title.public);
    }
    const publicObjective = objectivePrefix + "😀".repeat(Math.floor((1000 - objectivePrefix.length) / 2));
    expect(controller.state.graph?.nodes.find(node => node.id === "objective")?.title).toBe(publicObjective);
    expect(controller.selected?.control?.tasks.find(node => node.id === "objective")?.title).toBe(publicObjective);
    expect(controller.selected?.objective).toBe(objective);
    controller.selectNode("issue-61");
    await vi.waitFor(() => expect(contexts.at(-1)).toMatchObject({ run_id: runId, node_id: "issue-61" }));
    expect(root.textContent).toContain(issue.title);
    expect(controller.state.graph?.nodes.find(node => node.id === "issue-61")?.source?.display).toEqual({ number: 61, url: "https://github.com/joshyorko/plugins/issues/61" });
    expect(root.querySelector('[data-node-id="issue-61"] .node-ref')?.textContent).toBe("#61");
    const issueLink = root.querySelector<HTMLAnchorElement>(".graph-inspector a.source-link");
    expect(issueLink?.getAttribute("href")).toBe(issue.html_url);
    expect(issueLink?.getAttribute("rel")).toBe("noopener noreferrer");
    expect(controller.state.graph?.nodes.find(node => node.id === "long-ascii-title")?.source?.display).toBeUndefined();
    // Additive run projection from the compiled server.
    expect(controller.selected?.created_at).toEqual(expect.any(Number));
    expect(controller.selected?.activity_at).toBeGreaterThanOrEqual(controller.selected?.updated_at ?? Infinity);
    expect(controller.selected?.presentation?.blocker_kind).toEqual({ kind: "planning_only" });
    expect(root.querySelector(".campaign-head .started")?.textContent).toMatch(/^Created /);
    const before = controller.state.graph?.revision;
    expect(await controller.proposeChange({ kind: "set_target", node_id: "issue-61", target_id: "native-local" }), controller.state.error ?? "proposal").toBe(true);
    expect(await controller.applyChange(true), controller.state.error ?? "apply").toBe(true);
    let fromModel = graphEnvelopeSchema.parse(await model("get_factory_graph", { run_id: runId })).graph;
    expect(fromModel.revision).toBeGreaterThan(before ?? 0);
    expect(controller.state.graph).toEqual(fromModel);
    expect(fromModel.nodes.find(node => node.id === "issue-61")?.target_preference).toBe("native-local");
    expect(fromModel.changes.filter(change => change.status === "applied")).toHaveLength(2);
    const persistedImport = fromModel.changes.find(change => change.change.kind === "import_candidates")?.change;
    if (persistedImport?.kind !== "import_candidates") throw new Error("Missing persisted import");
    for (const title of titleCases) expect(persistedImport.nodes.find(node => node.id === title.id)?.title).toBe(title.raw);
    expect(fromModel.changes.every(change => change.actor === "local_operator")).toBe(true);
    expect(fromModel.attempts).toEqual([]); expect(fromModel.claim.held).toBe(false);
    expect(controller.state.backends?.targets.every(target => !target.execution_eligible)).toBe(true);
    expect(root.textContent).toContain("native-local");
    // The reverse direction uses the identical server command boundary and journal.
    const modelChange = graphEnvelopeSchema.parse(await model("propose_factory_change", {
      run_id: runId, expected_revision: fromModel.revision, idempotency_key: "model-prerequisite",
      change: { kind: "set_dependencies", node_id: "issue-61", dependencies: ["objective"] },
    }));
    fromModel = graphEnvelopeSchema.parse(await model("apply_factory_change", {
      run_id: runId, expected_revision: modelChange.graph.revision, change_id: modelChange.proposal?.id,
    })).graph;
    await controller.refresh();
    expect(() => controller.currentFollowUpPrompt("choose", "issue-61")).toThrow("Refresh");
    await controller.loadGraph();
    expect(controller.state.graph).toEqual(fromModel);
    expect(controller.state.graph?.nodes.find(node => node.id === "issue-61")?.dependencies).toEqual(["objective"]);
    expect(controller.selected?.control?.revision).toBe(fromModel.revision);
    await vi.waitFor(() => expect(contexts.at(-1)).toMatchObject({ node_id: "issue-61", graph_revision: fromModel.revision }));
    expect(messages).toEqual([]);
    await bridge.sendFollowUp(controller.currentFollowUpPrompt("choose", "issue-61"));
    await bridge.sendFollowUp(controller.currentFollowUpPrompt("summary"));
    expect(messages).toHaveLength(2);
    expect(JSON.stringify(messages[0])).toContain(`Revision: ${fromModel.revision}`);
    expect(JSON.stringify(messages[0])).toContain("Task ID: issue-61");
    // Stop/recreate only this disposable fixture daemon; preserve the private SQLite + WAL state.
    daemon.kill("SIGTERM"); await once(daemon, "exit");
    controller.setDisconnected("Disposable fixture daemon stopped");
    expect(await controller.proposeChange({ kind: "set_dependencies", node_id: "objective", dependencies: [] })).toBe(false);
    await vi.waitFor(() => expect(contexts.at(-1)).toEqual({}));
    daemon = spawn(resolve(process.env.LUNA_FACTORY_BINARY ?? "../server/target/debug/luna-factoryd"), ["serve", "--config", config, "--ui", resolve("dist/index.html")], { stdio: ["ignore", "pipe", "pipe"] });
    await vi.waitFor(async () => expect(await rpc("server/discover", {})).toBeTruthy(), { timeout: 10_000, interval: 50 });
    controller.setConnected(true);
    expect(() => controller.currentFollowUpPrompt("summary")).toThrow("Refresh");
    await controller.refresh(); await controller.loadGraph();
    expect(controller.state.graph).toEqual(fromModel);
    expect(controller.state.selectedNodeId).toBe("issue-61");
    expect(controller.selected?.control?.revision).toBe(fromModel.revision);
    expect(controller.currentFollowUpPrompt("choose", "issue-61")).toContain(`Revision: ${fromModel.revision}`);
    expect(calls.some(name => ["start_factory", "resume_factory_run", "steer_factory_run", "cancel_factory_run"].includes(name))).toBe(false);
    if (process.env.LUNA_GRAPH_EVIDENCE) await writeFile(process.env.LUNA_GRAPH_EVIDENCE, JSON.stringify({
      proof: "actual Rust/SQLite/HTTP + production UI controller + synthetic AppBridge; not live ChatGPT acceptance",
      source_head: execFileSync("git", ["-C", repo, "rev-parse", "HEAD"], { encoding: "utf8" }).trim(), issue_revision: issueRevision,
      graph: fromModel, model_context: contexts.at(-1), tool_calls: calls, native_dispatches: 0,
    }, null, 2));
  } finally {
    await app.close(); await host.close(); root.remove();
    if (daemon && daemon.exitCode === null) { daemon.kill("SIGTERM"); await once(daemon, "exit"); }
    await rm(directory, { recursive: true, force: true });
  }
}, 30_000);
