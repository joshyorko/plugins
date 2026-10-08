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
import { graphEnvelopeSchema } from "../src/domain";
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
    serverTools: {}, updateModelContext: { structuredContent: {} }, experimental: { "openai/modelContext": {} },
  });
  const contexts: Record<string, unknown>[] = [];
  const calls: string[] = [];
  const root = document.createElement("div"); document.body.append(root);
  try {
    const source = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
    const repo = join(directory, "plugins");
    // Actual inspected engineering repository, isolated from all active worktrees and services.
    execFileSync("git", ["clone", "--quiet", "--shared", source, repo]);
    const snapshotBytes = await readFile(resolve("../tests/fixtures/github/issue-61.json"), "utf8");
    const issue = JSON.parse(snapshotBytes) as { node_id: string; title: string; repository_id: number };
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
        ...(typeof params.name === "string" ? { "mcp-name": params.name } : {}),
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
    const created = graphEnvelopeSchema.parse(await model("create_factory_graph", {
      repository: "plugins", objective: "Plan the existing bounded-continuation issue", acceptance: ["Retain execution identity and prove convergence"], non_goals: ["No execution in this planning graph"],
      finish: "local_candidate", profile: "default", capacity: 1, repair_attempts: 0, wall_seconds: 300, idempotency_key: "real-issue-plan",
    }));
    const runId = created.graph.run_id;
    const proposal = graphEnvelopeSchema.parse(await model("propose_factory_change", {
      run_id: runId, expected_revision: created.graph.revision, idempotency_key: "import-issue-61", change: { kind: "import_candidates", nodes: [{
        id: "issue-61", title: issue.title, criterion_ids: ["A1"], dependencies: [],
        source: { provider: "github", repository_id: created.graph.repository.identity, item_id: `${issue.repository_id}:${issue.node_id}`, revision: issueRevision },
      }] },
    }));
    await model("apply_factory_change", { run_id: runId, expected_revision: proposal.graph.revision, change_id: proposal.proposal?.id });
    host.oncalltool = params => call(params.name, params.arguments ?? {});
    host.onupdatemodelcontext = async params => {
      contexts.push(params.structuredContent ?? {});
      return { _meta: { "openai/modelContext": { updateId: `e2e-${contexts.length}` } } };
    };
    const bridge = new HostBridge(app, (id, node) => { void controller.restoreContext(id, node); });
    const controller = new WorkbenchController(bridge, () => renderWorkbench(root, controller.state, null, false));
    app.ontoolresult = result => controller.receiveInitial(result);
    const [appTransport, hostTransport] = InMemoryTransport.createLinkedPair();
    await host.connect(hostTransport); await bridge.connect(appTransport); controller.setConnected(true);
    await host.sendToolInput({ arguments: {} });
    await host.sendToolResult(await call("open_factory", {}));
    await controller.select(runId); await controller.loadGraph();
    controller.selectNode("issue-61");
    await vi.waitFor(() => expect(contexts.at(-1)).toMatchObject({ run_id: runId, node_id: "issue-61" }));
    expect(root.textContent).toContain(issue.title);
    const before = controller.state.graph?.revision;
    expect(await controller.proposeChange({ kind: "set_target", node_id: "issue-61", target_id: "native-local" }), controller.state.error ?? "proposal").toBe(true);
    expect(await controller.applyChange(), controller.state.error ?? "apply").toBe(true);
    let fromModel = graphEnvelopeSchema.parse(await model("get_factory_graph", { run_id: runId })).graph;
    expect(fromModel.revision).toBeGreaterThan(before ?? 0);
    expect(controller.state.graph).toEqual(fromModel);
    expect(fromModel.nodes.find(node => node.id === "issue-61")?.target_preference).toBe("native-local");
    expect(fromModel.changes.filter(change => change.status === "applied")).toHaveLength(2);
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
    await controller.refresh(); await controller.loadGraph();
    expect(controller.state.graph).toEqual(fromModel);
    expect(controller.state.graph?.nodes.find(node => node.id === "issue-61")?.dependencies).toEqual(["objective"]);
    expect(controller.selected?.control?.revision).toBe(fromModel.revision);
    await vi.waitFor(() => expect(contexts.at(-1)).toMatchObject({ node_id: "issue-61", graph_revision: fromModel.revision }));
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
