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
  { file: "global-disconnected-mobile.png", width: 360, height: 720, params: { surface: "global", scenario: "planning", platform: "mobile", graph: "1", noTask: "1" }, noTask: true, disconnect: true },
  { file: "task-sheet-offline-mobile.png", width: 360, height: 720, params: { surface: "global", scenario: "planning", platform: "mobile", graph: "1", node: "task-b" }, disconnect: true, offlineSheet: true },
  { file: "global-proposal-review.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", scenario: "proposal", graph: "1", proposal: "1" } },
  { file: "global-large-host-font.png", width: 480, height: 760, params: { surface: "global", scenario: "planning", graph: "1", font: "large" } },
  { file: "campaign-map-light.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-map-dark.png", width: 1440, height: 1000, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-thread-narrow.png", width: 420, height: 900, params: { surface: "thread", scenario: "campaign", graph: "1", node: "issue-109" } },
  { file: "campaign-mobile-dark.png", width: 390, height: 844, params: { surface: "global", theme: "dark", platform: "mobile", scenario: "campaign", graph: "1", noTask: "1" }, noTask: true },
  { file: "swarm-map-synthetic.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", scenario: "swarm", graph: "1", node: "child:child-1" }, agent: "child-1" },
  { file: "swarm-lanes-synthetic-dark.png", width: 1280, height: 900, params: { surface: "global", mode: "fullscreen", theme: "dark", scenario: "swarm", graph: "1", node: "child:child-1" }, agent: "child-1", lanes: true },
  { file: "home-light.png", width: 1280, height: 860, params: { surface: "global", mode: "fullscreen", scenario: "home" } },
  { file: "home-empty-light.png", width: 1280, height: 860, params: { surface: "global", mode: "fullscreen", scenario: "home-empty" } },
  { file: "home-campaign-mobile-dark.png", width: 390, height: 844, params: { surface: "global", theme: "dark", platform: "mobile", scenario: "home" }, scroll: ".home-plans" },
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
      // Saved-plan grouping is a separate read; wait for it so every capture is deterministic.
      if (item.params.scenario !== "error") await page.waitForFunction(() => window.lunaDogfoodHost.calls.includes("list_factory_campaigns"));
      if (item.params.scenario === "home") await frame.locator(".home-plans .plan-row").first().waitFor();
      if (item.params.scenario === "home-empty") {
        await frame.locator(".home-page h1").waitFor();
        assert.equal(await frame.locator(".plan-composer").count(), 1, "Empty Home did not show its composer");
        assert.equal(await frame.locator(".plan-row, .home-group").count(), 0, "Empty Home showed an empty list section");
      }
      if (item.params.scenario === "campaign" && item.params.surface !== "inline") await frame.locator(".plan-title-row h1").waitFor();
      if (item.params.graph && !item.noTask) {
        const node = frame.locator(`[data-node-id="${item.params.node ?? "task-b"}"]`);
        await frame.locator(`[data-node-id="${item.params.node ?? "task-b"}"][aria-pressed="true"]`).waitFor();
        await node.scrollIntoViewIfNeeded();
        await node.focus();
        await page.keyboard.press("Tab");
        assert(await frame.evaluate(() => document.activeElement !== document.body), "Keyboard focus escaped the inspector");
      }
      if (item.noTask) {
        await frame.locator(".plan-title-row h1").waitFor();
        assert.equal(await frame.locator(".graph-inspector").count(), 0, "Mobile plan opened a task drawer by default");
      }
      if (item.lanes) await frame.locator('[data-action="view-mode"][data-mode="lanes"]').click();
      if (item.agent) {
        if (!item.lanes) await frame.locator('[data-action="view-mode"][data-mode="lanes"]').click();
        await frame.locator(`.activity-agent [data-thread-id="${item.agent}"]`).click();
        await frame.locator(`.activity-agent [data-thread-id="${item.agent}"][aria-pressed="true"]`).waitFor();
        await page.waitForFunction(() => JSON.stringify(window.lunaDogfoodHost.contexts.at(-1) ?? {}).includes("agent_label"));
        const context = await page.evaluate(() => window.lunaDogfoodHost.contexts.at(-1));
        assert(JSON.stringify(context).includes("agent_label"), "Agent selection did not reach model context");
        assert(!/"(?:agent_)?thread(?:_id)?"/.test(JSON.stringify(context)), "Model context carried a raw agent thread field");
        if (!item.lanes) await frame.locator('[data-action="view-mode"][data-mode="map"]').click();
      }
      if (item.lanes) {
        await frame.locator(`.activity-agent [data-thread-id="${item.agent}"][aria-pressed="true"]`).waitFor();
        await frame.locator(".activity-feed").waitFor();
        assert.equal(await frame.locator(".activity-agent").count(), 3, "Reported agents did not render");
        assert.equal(await frame.locator(".activity-event").count() > 0, true, "Recent activity did not render");
        assert.equal(await frame.locator(".lane-track, .run-lane, .lanes-legend").count(), 0, "Activity showed the old timeline chart");
        const reads = await page.evaluate(() => window.lunaDogfoodHost.calls.filter(name => name === "read_factory_agent_timeline").length);
        assert(reads <= 3, `Timeline reads were not bounded to the roster: ${reads}`);
      }
      if (item.editor) {
        if (item.editor === "start") await frame.locator("#objective").waitFor();
        else { await frame.locator(`#action-${item.editor}`).click(); await frame.locator(`form[data-form="${item.editor}"]`).waitFor(); }
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
        assert.equal(await frame.getByText("This plan changed. Refresh to see the latest version.").count(), 0, "Offline snapshot was described as changed");
      }
      if (item.offlineSheet) {
        await frame.locator(".graph-inspector .sheet-offline").waitFor();
        assert(await frame.locator(".graph-inspector [data-action='reconnect']:enabled").count() === 1, "Open task sheet hid its reconnect action");
        assert(await frame.locator(".graph-inspector [data-action='close-task']:visible").count() === 1, "Offline status obscured the task close button");
      }
      if (item.params.scenario === "campaign" && item.params.graph && !item.noTask) {
        // Wait for the GitHub-reported section so the capture shows settled telemetry.
        await frame.locator(".graph-inspector .github-report").waitFor();
        if (item.inspect) await frame.locator(".graph-inspector .github-report").evaluate(element => element.scrollIntoView({ block: "center" }));
      }
      if (item.params.proposal === "applied") {
        await frame.locator(".plan-updated").waitFor();
        const visible = await frame.locator("main").evaluate(element => { const copy = element.cloneNode(true); copy.querySelector(".technical-details")?.remove(); return copy.textContent ?? ""; });
        assert(!/75d01e55|revision \d|Base revision/i.test(visible), "The applied-change note exposed an ID or revision");
      } else if (item.params.proposal) {
        const review = frame.locator('[aria-label="Review plan change"]');
        await review.scrollIntoViewIfNeeded();
        assert.deepEqual(await review.locator(".change-diff dt").allTextContents(), ["Before", "After"]);
        assert.equal(await review.locator("#confirm-graph-change").count(), 0, "Plan review added a second confirmation checkbox");
        assert(await review.locator('[data-action="apply-change"]').isEnabled(), "Apply change was not available for a current proposal");
      }
      if (item.edit) {
        const toggle = frame.locator(`#edit-toggle-${item.params.node}`);
        await toggle.click();
        await frame.locator(`#edit-${item.params.node}:not([hidden])`).waitFor();
        await page.keyboard.press("Escape");
        const closedSheet = frame.locator(`#edit-${item.params.node}[hidden]`);
        await closedSheet.waitFor({ state: "attached" });
        assert.equal(await closedSheet.evaluate(element => element.hidden), true, "Escape did not close the task sheet");
        await toggle.click();
        await frame.locator(`#edit-${item.params.node}:not([hidden])`).waitFor();
        await frame.locator(".graph-inspector .inspector-actions").evaluate(element => { element.scrollIntoView({ block: "start" }); window.scrollBy(0, -(document.querySelector(".appbar")?.getBoundingClientRect().height ?? 0) - 8); });
      }
      if (item.params.font) assert(await frame.locator(".plan-title-row h1").evaluate(element => parseFloat(getComputedStyle(element).fontSize)) >= 18, "Host font size was ignored");
      if (item.params.scenario === "plan-basic") assert(!(await page.evaluate(() => window.lunaDogfoodHost.calls)).includes("list_factory_github_repositories"), "GitHub repos were listed without the server capability");
      if (item.params.scenario === "campaign" && item.params.surface !== "inline" && item.params.graph) {
        assert.equal(await frame.locator(".page-primary").count(), 1, "Plan page did not have its single primary action");
        assert.equal(await frame.locator(".decision-panel").count(), 0, "Planned state showed a decision banner");
        assert.equal(await frame.getByText("Refresh planning graph", { exact: true }).count(), 0, "Planned state showed a refresh action instead of setup guidance");
      }
      if (item.params.scenario === "blocked") {
        assert.equal(await frame.locator(".plan-banner h2").textContent(), "Luna can't confirm the earlier run stopped.");
        assert.equal(await frame.locator(".plan-banner [data-kind='reconcile']").textContent(), "Check running work");
        assert.equal(await frame.locator(".plan-banner [data-kind='cancel']").textContent(), "Stop everything");
      }
      if (item.params.scenario === "error") {
        assert.equal(await frame.locator(".full-page-error").count(), 1, "Initial error was not shown");
        assert.equal(await frame.locator(".skeleton").count(), 0, "Error and loading were shown together");
      }
      if (item.params.surface === "inline") {
        assert(await frame.locator(".inline-card button").count() <= 2, "Inline card has more than two actions");
        if (item.width <= 320) assert.equal(await frame.locator(".inline-sky:visible").count(), 0, "Narrow inline card left a glyph on its own row");
      }
      const host = await page.evaluate(() => ({ calls: window.lunaDogfoodHost.calls, messages: window.lunaDogfoodHost.messages.length }));
      assert(host.calls.every(name => readOnlyTools.includes(name)), "Visual capture attempted mutation or execution");
      if (item.params.scenario === "campaign" && item.params.graph) assert(host.calls.includes("read_factory_delivery"), "GitHub delivery was not read for a GitHub-sourced plan");
      if (item.params.scenario !== "campaign" && item.params.scenario !== "home") assert(!host.calls.includes("read_factory_delivery"), "GitHub delivery was read while the integration was unconfigured");
      assert.equal(host.messages, 0, "Message sent without a user click");
      const layout = await frame.evaluate(() => {
        const overflowingControls = Array.from(document.querySelectorAll("button,input,select,textarea")).flatMap(element => {
          const bounds = element.getBoundingClientRect();
          if (!bounds.width || bounds.left >= -1 && bounds.right <= innerWidth + 1) return [];
          return [{ tag: element.tagName.toLowerCase(), label: element.getAttribute("aria-label") ?? element.textContent?.trim() ?? "", left: Math.round(bounds.left), right: Math.round(bounds.right), width: Math.round(bounds.width) }];
        });
        return { width: innerWidth, scrollWidth: document.documentElement.scrollWidth, overflowingControls };
      });
      assert(layout.scrollWidth <= layout.width + 1, `Horizontal overflow in ${item.file}: ${JSON.stringify(layout)}`);
      assert.deepEqual(layout.overflowingControls, [], `Clipped controls in ${item.file}`);
      const composerSafe = await page.evaluate(() => document.querySelector("iframe").getBoundingClientRect().bottom <= document.querySelector("footer").getBoundingClientRect().top + 1);
      assert(composerSafe, "App overlaps the synthetic host composer");
      await page.screenshot({ path: join(directory, item.file) });
      captures.push({ file: item.file, width: item.width, height: item.height, params: item.params, layout, read_only_tool_calls: host.calls, automatic_messages: host.messages });
    } finally { await page.close(); }
  }
  const hash = createHash("sha256");
  for (const name of (await readdir(join(ui, "src"))).sort()) { hash.update(name); hash.update(await readFile(join(ui, "src", name))); }
  const distHash = createHash("sha256").update(await readFile(join(ui, "dist/index.html"))).digest("hex");
  await writeFile(join(directory, "manifest.json"), JSON.stringify({ expected_ui_version: "0.2.1", evidence_kind: "Real browser + MCP Apps AppBridge with synthetic fixtures; not authenticated ChatGPT Desktop acceptance", ui_source_sha256: hash.digest("hex"), dist_sha256: distHash, captures }, null, 2) + "\n");
  console.log(`PASS ${captures.length} real basic-host captures; zero execution/mutation calls or automatic messages`);
} finally { await browser?.close(); await server.close(); }
