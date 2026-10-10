// Opt-in browser acceptance with a real AppBridge/PostMessageTransport and synthetic data.
import { strict as assert } from "node:assert";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const ui = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const directory = join(ui, "test/dogfood-visual-snapshots");
const cases = [
  { file: "inline-planning-mobile-light.png", width: 320, height: 640, params: { surface: "inline", scenario: "planning", platform: "mobile" } },
  { file: "global-planning-split-light.png", width: 480, height: 760, params: { surface: "global", scenario: "planning", graph: "1" } },
  { file: "fullscreen-planning-dark.png", width: 1200, height: 900, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "planning", graph: "1" } },
  { file: "thread-selected-dark.png", width: 360, height: 760, params: { surface: "thread", theme: "dark", scenario: "planning", graph: "1" } },
  { file: "inline-decision-light.png", width: 640, height: 640, params: { surface: "inline", scenario: "decision" } },
  { file: "thread-blocked-narrow.png", width: 320, height: 700, params: { surface: "thread", scenario: "blocked", graph: "1" } },
  { file: "global-error-light.png", width: 480, height: 640, params: { surface: "global", scenario: "error" } },
  { file: "global-disconnected-mobile.png", width: 360, height: 720, params: { surface: "global", scenario: "planning", platform: "mobile", graph: "1" }, disconnect: true },
  { file: "global-proposal-review.png", width: 480, height: 760, params: { surface: "global", scenario: "planning", graph: "1", proposal: "1" } },
  { file: "global-large-host-font.png", width: 480, height: 760, params: { surface: "global", scenario: "planning", graph: "1", font: "large" } },
  { file: "campaign-map-light.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-map-dark.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-thread-narrow.png", width: 420, height: 900, params: { surface: "thread", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-mobile-dark.png", width: 390, height: 844, params: { surface: "global", theme: "dark", platform: "mobile", scenario: "campaign", graph: "1", node: "issue-113" } },
  { file: "swarm-map-synthetic.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", scenario: "swarm", graph: "1", node: "child:child-1" }, agent: "child-1" },
  { file: "swarm-lanes-synthetic-dark.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "swarm", graph: "1", node: "child:child-1" }, agent: "child-1", lanes: true },
  { file: "home-light.png", width: 1280, height: 860, params: { surface: "global", mode: "fullscreen", scenario: "home" } },
  { file: "inline-campaign-light.png", width: 720, height: 420, params: { surface: "inline", scenario: "campaign" } },
];
const server = await createServer({ root: ui, configFile: join(ui, "vite.config.ts"), server: { host: "127.0.0.1", port: 0, strictPort: true } });
let browser;
try {
  await server.listen();
  const url = server.resolvedUrls?.local[0];
  assert(url, "Missing disposable loopback simulator URL");
  browser = await chromium.launch({ headless: true, ...(process.env.LUNA_CHROMIUM_EXECUTABLE ? { executablePath: process.env.LUNA_CHROMIUM_EXECUTABLE } : {}), args: ["--disable-dev-shm-usage"] });
  await mkdir(directory, { recursive: true });
  const captures = [];
  for (const item of cases) {
    const page = await browser.newPage({ viewport: { width: item.width, height: item.height } });
    try {
      await page.goto(`${url}test/dogfood-host.html?${new URLSearchParams(item.params)}`);
      await page.waitForFunction(() => window.lunaDogfoodHost?.initialized);
      const frame = page.frames().find(frame => frame.url().endsWith("/index.html"));
      assert(frame, "Production App iframe did not mount");
      await frame.locator(".workbench").waitFor();
      if (item.params.graph) {
        const node = frame.locator(`[data-node-id="${item.params.node ?? "task-b"}"]`);
        await frame.locator(`[data-node-id="${item.params.node ?? "task-b"}"][aria-pressed="true"]`).waitFor();
        await node.scrollIntoViewIfNeeded();
        await node.focus();
        await page.keyboard.press("Tab");
        assert(await frame.evaluate(() => document.activeElement !== document.body), "Keyboard focus escaped the inspector");
      }
      if (item.agent) {
        await frame.locator(`.crew [data-thread-id="${item.agent}"]`).click();
        await frame.locator(`.crew [data-thread-id="${item.agent}"][aria-pressed="true"]`).waitFor();
        const context = await page.evaluate(() => window.lunaDogfoodHost.contexts.at(-1));
        assert(JSON.stringify(context).includes("agent_label"), "Agent selection did not reach model context");
        assert(!/"(?:agent_)?thread(?:_id)?"/.test(JSON.stringify(context)), "Model context carried a raw agent thread field");
      }
      if (item.lanes) {
        await frame.locator('[data-action="view-mode"][data-mode="lanes"]').click();
        await frame.locator(`.lanes [data-thread-id="${item.agent}"][aria-pressed="true"]`).waitFor();
      }
      if (item.width >= 1000) await frame.evaluate(() => window.scrollTo(0, 0));
      if (item.disconnect) {
        await page.evaluate(() => window.lunaDogfoodHost.disconnect());
        await frame.locator('[data-connection="disconnected"]').waitFor();
        assert(await frame.locator('[data-action="chat-follow-up"]:enabled').count() === 0, "Disconnected host left message controls enabled");
      }
      if (item.params.proposal) await frame.locator('[aria-label="Review graph change"]').scrollIntoViewIfNeeded();
      if (item.params.font) assert(await frame.locator(".graph-inspector .route-list dd").first().evaluate(element => parseFloat(getComputedStyle(element).fontSize)) >= 18, "Host font size was ignored");
      const host = await page.evaluate(() => ({ calls: window.lunaDogfoodHost.calls, messages: window.lunaDogfoodHost.messages.length }));
      assert(host.calls.every(name => ["get_factory_run", "get_factory_graph", "get_factory_backends", "refresh_factory", "open_factory"].includes(name)), "Visual capture attempted mutation or execution");
      assert.equal(host.messages, 0, "Message sent without a user click");
      const layout = await frame.evaluate(() => ({ width: innerWidth, scrollWidth: document.documentElement.scrollWidth, overflowingControls: Array.from(document.querySelectorAll("button,input,select,textarea")).filter(element => { const bounds = element.getBoundingClientRect(); return bounds.width > 0 && (bounds.left < -1 || bounds.right > innerWidth + 1); }).length }));
      assert(layout.scrollWidth <= layout.width + 1, `Horizontal overflow in ${item.file}: ${JSON.stringify(layout)}`);
      assert.equal(layout.overflowingControls, 0, `Clipped controls in ${item.file}`);
      const composerSafe = await page.evaluate(() => document.querySelector("iframe").getBoundingClientRect().bottom <= document.querySelector("footer").getBoundingClientRect().top + 1);
      assert(composerSafe, "App overlaps the synthetic host composer");
      await page.screenshot({ path: join(directory, item.file) });
      captures.push({ file: item.file, width: item.width, height: item.height, params: item.params, layout, read_only_tool_calls: host.calls, automatic_messages: host.messages });
    } finally { await page.close(); }
  }
  const hash = createHash("sha256");
  for (const name of (await readdir(join(ui, "src"))).sort()) { hash.update(name); hash.update(await readFile(join(ui, "src", name))); }
  await writeFile(join(directory, "manifest.json"), JSON.stringify({ expected_ui_version: "0.2.1", evidence_kind: "Real browser + MCP Apps AppBridge with synthetic fixtures; not authenticated ChatGPT Desktop acceptance", ui_source_sha256: hash.digest("hex"), captures }, null, 2) + "\n");
  console.log(`PASS ${captures.length} real basic-host captures; zero execution/mutation calls or automatic messages`);
} finally { await browser?.close(); await server.close(); }
