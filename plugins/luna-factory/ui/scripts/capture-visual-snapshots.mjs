#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const uiRoot = fileURLToPath(new URL("..", import.meta.url));
const output = join(uiRoot, "test", "visual-snapshots");
const chrome = process.env.CHROME_BIN || "/usr/bin/google-chrome";
const origin = "http://127.0.0.1:4179";
const cases = [
  { file: "inline-needs-input-light.png", width: 840, height: 900, params: { surface: "inline", scenario: "needs-input", theme: "light", platform: "desktop", message: "1" } },
  { file: "fullscreen-selected-node-light.png", width: 1440, height: 1900, params: { surface: "global", scenario: "selected-node", theme: "light", platform: "desktop", message: "1" } },
  { file: "thread-blocked-dark.png", width: 820, height: 1000, params: { surface: "thread", scenario: "blocked", theme: "dark", platform: "desktop", message: "1" } },
  { file: "mobile-disconnected-dark.png", width: 390, height: 844, params: { surface: "global", scenario: "disconnected", theme: "dark", platform: "mobile", message: "1" } },
  { file: "mobile-inline-disconnected-dark.png", width: 390, height: 844, params: { surface: "inline", scenario: "disconnected", theme: "dark", platform: "mobile", message: "1" } },
  { file: "fullscreen-error-light.png", width: 1024, height: 768, params: { surface: "global", scenario: "error", theme: "light", platform: "desktop", message: "1" } },
];

await mkdir(output, { recursive: true });
const server = spawn("npm", ["run", "dev", "--", "--host", "127.0.0.1", "--port", "4179", "--strictPort"], { cwd: uiRoot, stdio: "inherit" });
try {
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (server.exitCode !== null) throw new Error("Vite fixture server exited before it was ready");
    try {
      const response = await fetch(`${origin}/?preview=fixture`);
      if (response.ok) { ready = true; break; }
    } catch { /* Vite is still starting. */ }
    await delay(100);
  }
  if (!ready) throw new Error("Vite fixture server did not become ready");
  for (const capture of cases) {
    const url = new URL("/", origin);
    url.searchParams.set("preview", "fixture");
    for (const [key, value] of Object.entries(capture.params)) url.searchParams.set(key, value);
    const result = spawnSync(chrome, [
      "--headless", "--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu", "--hide-scrollbars",
      "--run-all-compositor-stages-before-draw", "--virtual-time-budget=2500",
      `--window-size=${capture.width},${capture.height}`,
      `--screenshot=${join(output, capture.file)}`,
      url.toString(),
    ], { cwd: uiRoot, encoding: "utf8", stdio: "pipe" });
    if (result.status !== 0) throw new Error(`${capture.file}: ${result.stderr.slice(-1200)}`);
  }
  const manifest = {
    expected_ui_version: "0.2.1",
    evidence_kind: "synthetic local fixture host; not ChatGPT Desktop acceptance",
    captures: cases.map(({ file, width, height, params }) => ({ file, width, height, params })),
  };
  await writeFile(join(output, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`Captured ${cases.length} synthetic host screenshots for UI ${manifest.expected_ui_version}.\n`);
} finally {
  server.kill("SIGTERM");
}
