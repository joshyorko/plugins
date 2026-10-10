// Opt-in evidence: a disposable compiled luna-factoryd + SQLite + MCP HTTP, a recorded GitHub issue graph
// imported through model-visible tools, and the production App in a real browser behind a read-only proxy.
// Requires LUNA_FACTORY_BINARY. Never connects to live Luna services; native execution is impossible here.
import { strict as assert } from "node:assert";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const ui = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = process.env.LUNA_FACTORY_BINARY;
assert(binary, "Set LUNA_FACTORY_BINARY to a disposable luna-factoryd build");
const output = join(ui, "test/live-visual-snapshots");
const snapshotPath = resolve(ui, "../tests/fixtures/github/issue-graph-67.json");
const snapshotBytes = await readFile(snapshotPath, "utf8");
const snapshot = JSON.parse(snapshotBytes);
const readOnly = new Set(["get_factory_run", "get_factory_graph", "get_factory_backends", "refresh_factory", "open_factory", "open_factory_panel", "read_factory_delivery", "inspect_factory_issue_graph"]);

const directory = await mkdtemp(join(tmpdir(), "luna-live-plan-"));
const probe = createNetServer(); probe.listen(0, "127.0.0.1"); await once(probe, "listening");
const port = probe.address().port; await new Promise(done => probe.close(done));
const endpoint = `http://127.0.0.1:${port}/mcp`;
let daemon; let vite; let browser;
try {
  const repo = join(directory, "plugins");
  execFileSync("git", ["clone", "--quiet", "--shared", execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim(), repo]);
  const config = join(directory, "config.json");
  await writeFile(config, JSON.stringify({
    listen: `127.0.0.1:${port}`, database: join(directory, "state", "runs.sqlite"),
    codex_binary: join(directory, "native-must-not-exist"), skill_path: join(directory, "SKILL.md"),
    repositories: { plugins: { root: repo, max_finish: "local_candidate" } },
    profiles: { default: { effort: "high" } }, limits: { capacity: 1, repair_attempts: 0, wall_seconds: 300 },
  }));
  daemon = spawn(resolve(binary), ["serve", "--config", config, "--ui", join(ui, "dist/index.html")], { stdio: ["ignore", "ignore", "pipe"] });
  let stderr = ""; daemon.stderr.on("data", data => { stderr += String(data); });
  const rpc = async (method, params) => {
    const response = await fetch(endpoint, { method: "POST", headers: {
      "content-type": "application/json", accept: "application/json, text/event-stream", "mcp-protocol-version": "2026-07-28", "mcp-method": method,
      ...(typeof params.name === "string" ? { "mcp-name": params.name } : {}),
    }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params: { ...params, _meta: {
      "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": { name: "live-plan-capture", version: "0.2.1" }, "io.modelcontextprotocol/clientCapabilities": {},
    } } }) });
    const data = await response.json();
    if (!response.ok || data.error) throw new Error(JSON.stringify(data).slice(0, 600));
    return data.result;
  };
  for (let attempt = 0; ; attempt += 1) {
    if (daemon.exitCode !== null) throw new Error(`luna-factoryd exited: ${stderr}`);
    try { await rpc("server/discover", {}); break; } catch (error) { if (attempt > 200) throw error; await delay(50); }
  }
  const tool = async (name, args) => {
    const result = await rpc("tools/call", { name, arguments: args });
    if (result.isError) throw new Error(`${name}: ${JSON.stringify(result.content).slice(0, 600)}`);
    return result.structuredContent;
  };
  // The same model-visible commands ChatGPT would use. Nothing here dispatches native work.
  const parent = snapshot.issues.find(issue => issue.number === 67);
  const children = snapshot.issues.filter(issue => issue.number !== 67);
  let envelope = await tool("create_factory_graph", {
    repository: "plugins", objective: `${parent.title} (#67)`, acceptance: ["Every sub-issue is resolved by a verified change"], non_goals: ["No execution from this planning graph"],
    finish: "local_candidate", profile: "default", capacity: 1, repair_attempts: 0, wall_seconds: 300, idempotency_key: "live-issue-graph-67",
  });
  const runId = envelope.graph.run_id;
  const identity = envelope.graph.repository.identity;
  const nodes = children.map(issue => ({
    id: `issue-${issue.number}`, title: issue.title, criterion_ids: ["A1"], dependencies: issue.requires.map(number => `issue-${number}`),
    source: { provider: "github", repository_id: identity, item_id: `${snapshot.repository_id}:${issue.node_id}`, revision: `sha256:${createHash("sha256").update(JSON.stringify(issue)).digest("hex")}` },
  }));
  envelope = await tool("propose_factory_change", { run_id: runId, expected_revision: envelope.graph.revision, idempotency_key: "live-import-67", change: { kind: "import_candidates", nodes } });
  envelope = await tool("apply_factory_change", { run_id: runId, expected_revision: envelope.graph.revision, change_id: envelope.proposal.id });
  const required = new Set(children.flatMap(issue => issue.requires));
  const sinks = children.filter(issue => !required.has(issue.number)).map(issue => `issue-${issue.number}`);
  envelope = await tool("propose_factory_change", { run_id: runId, expected_revision: envelope.graph.revision, idempotency_key: "live-objective-67", change: { kind: "set_dependencies", node_id: "objective", dependencies: sinks } });
  envelope = await tool("apply_factory_change", { run_id: runId, expected_revision: envelope.graph.revision, change_id: envelope.proposal.id });
  const graph = (await tool("get_factory_graph", { run_id: runId })).graph;
  assert.equal(graph.nodes.length, children.length + 1);
  assert.equal(graph.attempts.length, 0); assert.equal(graph.claim.held, false); assert.equal(graph.planning_only, true);

  const cases = [
    { file: "live-campaign-map-light.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", node: "issue-79" } },
    { file: "live-campaign-map-dark.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", theme: "dark", node: "issue-81" } },
    { file: "live-campaign-thread.png", width: 420, height: 900, params: { surface: "thread", node: "issue-77" } },
    { file: "live-campaign-mobile-dark.png", width: 390, height: 844, params: { surface: "global", theme: "dark", platform: "mobile", node: "issue-79" } },
    { file: "live-lanes-planning.png", width: 1280, height: 860, params: { surface: "global", mode: "fullscreen", node: "issue-79" }, lanes: true },
    { file: "live-inline-light.png", width: 720, height: 420, params: { surface: "inline" } },
  ];
  vite = await createServer({ root: ui, configFile: join(ui, "vite.config.ts"), server: { host: "127.0.0.1", port: 0, strictPort: true } });
  await vite.listen();
  const url = vite.resolvedUrls?.local[0];
  browser = await chromium.launch({ headless: true, ...(process.env.LUNA_CHROMIUM_EXECUTABLE ? { executablePath: process.env.LUNA_CHROMIUM_EXECUTABLE } : {}), args: ["--disable-dev-shm-usage"] });
  await mkdir(output, { recursive: true });
  const captures = [];
  for (const item of cases) {
    const page = await browser.newPage({ viewport: { width: item.width, height: item.height } });
    const proxied = [];
    await page.exposeFunction("lunaRpc", async (name, args) => {
      proxied.push(name);
      if (!readOnly.has(name)) return { isError: true, content: [{ type: "text", text: "Live capture proxy is read-only." }] };
      return await rpc("tools/call", { name, arguments: args });
    });
    try {
      await page.goto(`${url}test/live-host.html?${new URLSearchParams({ ...item.params, run: runId, revision: String(graph.revision) })}`);
      await page.waitForFunction(() => window.lunaLiveHost?.initialized);
      const frame = page.frames().find(candidate => candidate.url().endsWith("/index.html"));
      assert(frame, "Production App iframe did not mount");
      await frame.locator(".workbench").waitFor();
      if (item.params.node) await frame.locator(`[data-node-id="${item.params.node}"][aria-pressed="true"]`).waitFor();
      if (item.params.surface === "inline") await frame.locator('[data-action="chat-follow-up"]:enabled').waitFor();
      if (item.lanes) { await frame.locator('[data-action="view-mode"][data-mode="lanes"]').click(); await frame.locator(".lanes").waitFor(); }
      if (item.width >= 1000) await frame.evaluate(() => window.scrollTo(0, 0));
      else if (item.params.node) await frame.locator(`[data-node-id="${item.params.node}"]`).scrollIntoViewIfNeeded();
      const host = await page.evaluate(() => ({ messages: window.lunaLiveHost.messages.length }));
      assert(proxied.every(name => readOnly.has(name)), `Live capture attempted a non-read tool: ${proxied.join(", ")}`);
      assert.equal(host.messages, 0, "Message sent without a user click");
      const layout = await frame.evaluate(() => ({ width: innerWidth, scrollWidth: document.documentElement.scrollWidth, overflowingControls: Array.from(document.querySelectorAll("button,input,select,textarea")).filter(element => { const bounds = element.getBoundingClientRect(); return bounds.width > 0 && (bounds.left < -1 || bounds.right > innerWidth + 1); }).length }));
      assert(layout.scrollWidth <= layout.width + 1, `Horizontal overflow in ${item.file}`);
      assert.equal(layout.overflowingControls, 0, `Clipped controls in ${item.file}`);
      await page.screenshot({ path: join(output, item.file) });
      captures.push({ file: item.file, width: item.width, height: item.height, params: item.params, layout, read_only_tool_calls: proxied });
    } finally { await page.close(); }
  }
  const source = createHash("sha256");
  for (const name of (await readdir(join(ui, "src"))).sort()) { source.update(name); source.update(await readFile(join(ui, "src", name))); }
  await writeFile(join(output, "manifest.json"), JSON.stringify({
    evidence_kind: "Real disposable luna-factoryd + SQLite + MCP HTTP, recorded GitHub issue snapshot imported through model-visible tools, real browser AppBridge; not authenticated ChatGPT Desktop acceptance",
    server_binary_sha256: createHash("sha256").update(await readFile(resolve(binary))).digest("hex"),
    ui_source_sha256: source.digest("hex"),
    github_snapshot_sha256: createHash("sha256").update(snapshotBytes).digest("hex"),
    graph: { revision: graph.revision, nodes: graph.nodes.length, attempts: graph.attempts.length, planning_only: graph.planning_only, claim: graph.claim },
    captures,
  }, null, 2) + "\n");
  console.log(`PASS ${captures.length} live-server captures at graph revision ${graph.revision}; read-only proxy; zero native dispatch`);
} finally {
  await browser?.close(); await vite?.close();
  if (daemon && daemon.exitCode === null) { daemon.kill("SIGTERM"); await once(daemon, "exit").catch(() => undefined); }
  await rm(directory, { recursive: true, force: true });
}
