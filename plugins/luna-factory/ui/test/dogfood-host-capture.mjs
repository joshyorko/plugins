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
  { file: "campaign-blocked-light.png", width: 1280, height: 860, params: { surface: "global", mode: "fullscreen", scenario: "blocked", graph: "1" } },
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
  { file: "home-campaign-mobile-dark.png", width: 390, height: 844, params: { surface: "global", theme: "dark", platform: "mobile", scenario: "home" }, scroll: ".group-campaigns" },
  { file: "inline-campaign-light.png", width: 720, height: 420, params: { surface: "inline", scenario: "campaign" } },
  { file: "campaign-github-ready-light.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", scenario: "campaign", graph: "1", node: "issue-103" }, inspect: true },
  { file: "campaign-github-unavailable-narrow.png", width: 420, height: 900, params: { surface: "thread", scenario: "campaign", graph: "1", node: "issue-103", github: "unavailable" }, inspect: true },
  { file: "campaign-plan-updated-light.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", scenario: "campaign", graph: "1", node: "issue-109", proposal: "applied" } },
  { file: "campaign-task-edit-narrow.png", width: 420, height: 900, params: { surface: "thread", theme: "dark", scenario: "campaign", graph: "1", node: "issue-109" }, edit: true },
  // The plan form. Connect and create are mutations, so these captures only read and never click them.
  // Opening the repo menu and choosing "+ New repo…" are view-only.
  { file: "plan-light.png", width: 760, height: 900, params: { surface: "global", scenario: "plan" }, editor: "start" },
  { file: "plan-dark.png", width: 1200, height: 900, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "plan" }, editor: "start" },
  { file: "plan-narrow-mobile.png", width: 360, height: 760, params: { surface: "global", platform: "mobile", scenario: "plan" }, editor: "start" },
  { file: "plan-repo-popover-open.png", width: 760, height: 900, params: { surface: "global", scenario: "plan" }, editor: "start", menu: true },
  { file: "plan-repo-popover-narrow-dark.png", width: 360, height: 760, params: { surface: "global", platform: "mobile", theme: "dark", scenario: "plan" }, editor: "start", menu: true },
  { file: "plan-more-options-light.png", width: 760, height: 1000, params: { surface: "global", scenario: "plan" }, editor: "start", open: "#plan-more" },
  { file: "plan-new-repo-narrow.png", width: 360, height: 760, params: { surface: "global", platform: "mobile", scenario: "plan" }, editor: "start", newRepo: "moon-garden" },
  // An invalid name stops at the inline error before any tool call.
  { file: "plan-new-repo-invalid-light.png", width: 760, height: 900, params: { surface: "global", scenario: "plan" }, editor: "start", newRepo: "Moon Garden", submit: true },
  { file: "plan-approved-only-dark.png", width: 480, height: 760, params: { surface: "global", theme: "dark", scenario: "plan-basic" }, editor: "start" },
];
/** Read-only tools a capture may call. GitHub reads are display-only and never mutate. */
const readOnlyTools = ["get_factory_run", "get_factory_graph", "get_factory_backends", "refresh_factory", "open_factory", "list_factory_campaigns", "read_factory_agent_timeline", "read_factory_delivery", "inspect_factory_issue_graph", "list_factory_github_repositories"];
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
      // Campaign grouping is a separate read; wait for it so every capture is deterministic.
      if (item.params.scenario !== "error") await page.waitForFunction(() => window.lunaDogfoodHost.calls.includes("list_factory_campaigns"));
      if (item.params.scenario === "home") await frame.locator(".group-campaigns .campaign-entity .campaign-children .campaign-child").first().waitFor();
      if (item.params.scenario === "campaign" && item.params.surface !== "inline") await frame.locator(".campaign-head .campaign-parent").waitFor();
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
        await page.waitForFunction(() => JSON.stringify(window.lunaDogfoodHost.contexts.at(-1) ?? {}).includes("agent_label"));
        const context = await page.evaluate(() => window.lunaDogfoodHost.contexts.at(-1));
        assert(JSON.stringify(context).includes("agent_label"), "Agent selection did not reach model context");
        assert(!/"(?:agent_)?thread(?:_id)?"/.test(JSON.stringify(context)), "Model context carried a raw agent thread field");
      }
      if (item.lanes) {
        await frame.locator('[data-action="view-mode"][data-mode="lanes"]').click();
        await frame.locator(`.lanes [data-thread-id="${item.agent}"][aria-pressed="true"]`).waitFor();
        // Synthetic "Observed in Codex" fixtures: two observed lanes and one server-reported unavailable lane.
        await frame.locator(".observed-events").waitFor();
        await frame.locator(".lane-track.unavailable").waitFor();
        assert.equal(await frame.locator(".lane-track.observed").count(), 2, "Observed lanes did not render");
        const reads = await page.evaluate(() => window.lunaDogfoodHost.calls.filter(name => name === "read_factory_agent_timeline").length);
        assert(reads <= 3, `Timeline reads were not bounded to the roster: ${reads}`);
      }
      if (item.editor) {
        await frame.locator(`#action-${item.editor}`).click();
        await frame.locator(`form[data-form="${item.editor}"]`).waitFor();
        // The GitHub list is a separate read-only call; settle it so the picker is deterministic.
        if (item.params.scenario === "plan") await frame.locator("#repo-menu [data-github-repo]").first().waitFor({ state: "attached" });
        if (item.params.scenario === "plan-basic") assert.equal(await frame.locator('#repo-menu [data-github-repo], #repo-menu [data-repo-action="new"]').count(), 0, "Approved-only plan form offered connect or create");
        await frame.evaluate(() => { document.activeElement?.blur?.(); window.scrollTo(0, 0); });
        // The header button the click used is gone now; park the pointer so nothing shows a hover.
        await page.mouse.move(1, item.height - 2);
        if (item.menu || item.newRepo) {
          await frame.locator("#repo-chip").click();
          await frame.locator("#repo-popover:not([hidden])").waitFor();
          assert.equal(await frame.locator("#repo-chip").getAttribute("aria-expanded"), "true", "Repo chip did not report the open menu");
        }
        if (item.newRepo) {
          await frame.locator('#repo-menu [data-repo-action="new"]').click();
          await frame.locator("#new_repository").waitFor();
          assert(await frame.locator("#repo-popover").evaluate(element => element.hidden), "Repo menu stayed open after + New repo…");
          await frame.locator("#new_repository").fill(item.newRepo);
          await frame.locator("#objective").fill("Start a tiny CLI that prints tonight's moon phase.");
          if (item.submit) {
            await frame.locator('button[type="submit"][value="plan"]').click();
            await frame.locator("#plan-repo-error:not([hidden])").waitFor();
            assert.equal(await frame.locator("#new_repository").getAttribute("aria-invalid"), "true", "Invalid repo name was not marked");
          }
          await frame.evaluate(() => document.activeElement?.blur?.());
          await page.mouse.move(1, item.height - 2);
          // Let the composer's focus transition settle so the capture shows the resting state.
          await page.waitForTimeout(250);
        }
        // Only the repo popover may scroll; the page body never gets an inner scrollbar.
        const innerScrollers = await frame.evaluate(() => Array.from(document.querySelectorAll("body *")).filter(element => element.tagName !== "TEXTAREA" && !element.closest("#repo-popover") && /(auto|scroll)/.test(getComputedStyle(element).overflowY) && element.scrollHeight > element.clientHeight + 1).map(element => element.className));
        assert.deepEqual(innerScrollers, [], `Inner scroll area in ${item.file}`);
        // Open one disclosure and bring it under the sticky app bar.
        if (item.open) await frame.locator(item.open).evaluate(element => { element.open = true; element.scrollIntoView({ block: "start" }); window.scrollBy(0, -(document.querySelector(".appbar")?.getBoundingClientRect().height ?? 0) - 12); });
      }
      if (item.width >= 1000) await frame.evaluate(() => window.scrollTo(0, 0));
      // Clear the sticky app bar so the section heading stays visible in the capture.
      if (item.scroll) await frame.locator(item.scroll).evaluate(element => { element.scrollIntoView({ block: "start" }); window.scrollBy(0, -(document.querySelector(".appbar")?.getBoundingClientRect().height ?? 0) - 8); });
      if (item.disconnect) {
        await page.evaluate(() => window.lunaDogfoodHost.disconnect());
        await frame.locator('[data-connection="disconnected"]').waitFor();
        assert(await frame.locator('[data-action="chat-follow-up"]:enabled').count() === 0, "Disconnected host left message controls enabled");
      }
      if (item.params.scenario === "campaign" && item.params.graph) {
        // Wait for the GitHub-reported section so the capture shows settled telemetry.
        await frame.locator(".graph-inspector .github-report").waitFor();
        if (item.inspect) await frame.locator(".graph-inspector .github-report").evaluate(element => element.scrollIntoView({ block: "center" }));
      }
      if (item.params.proposal === "applied") {
        await frame.locator(".plan-updated").waitFor();
        const visible = await frame.locator("main").evaluate(element => { const copy = element.cloneNode(true); copy.querySelector("#evidence-audit")?.remove(); return copy.textContent ?? ""; });
        assert(!/75d01e55|revision \d|Base revision/i.test(visible), "The applied-change note exposed an ID or revision");
      } else if (item.params.proposal) await frame.locator('[aria-label="Review graph change"]').scrollIntoViewIfNeeded();
      if (item.edit) {
        const toggle = frame.locator(`#edit-toggle-${item.params.node}`);
        await toggle.click();
        await frame.locator(`#edit-${item.params.node}:not([hidden])`).waitFor();
        await frame.locator(".graph-inspector .inspector-actions").evaluate(element => { element.scrollIntoView({ block: "start" }); window.scrollBy(0, -(document.querySelector(".appbar")?.getBoundingClientRect().height ?? 0) - 8); });
      }
      if (item.params.font) assert(await frame.locator(".graph-inspector .route-list dd").first().evaluate(element => parseFloat(getComputedStyle(element).fontSize)) >= 18, "Host font size was ignored");
      if (item.params.scenario === "plan-basic") assert(!(await page.evaluate(() => window.lunaDogfoodHost.calls)).includes("list_factory_github_repositories"), "GitHub repos were listed without the server capability");
      const host = await page.evaluate(() => ({ calls: window.lunaDogfoodHost.calls, messages: window.lunaDogfoodHost.messages.length }));
      assert(host.calls.every(name => readOnlyTools.includes(name)), "Visual capture attempted mutation or execution");
      if (item.params.scenario === "campaign" && item.params.graph) assert(host.calls.includes("read_factory_delivery"), "GitHub delivery was not read for a GitHub-sourced plan");
      if (item.params.scenario !== "campaign" && item.params.scenario !== "home") assert(!host.calls.includes("read_factory_delivery"), "GitHub delivery was read while the integration was unconfigured");
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
