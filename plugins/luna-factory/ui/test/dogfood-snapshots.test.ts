import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { UI_VERSION } from "../src/domain";

const directory = new URL("./dogfood-visual-snapshots/", import.meta.url);
const manifestSchema = z.object({ expected_ui_version: z.string(), evidence_kind: z.string(), ui_source_sha256: z.string(), captures: z.array(z.object({ file: z.string(), width: z.number(), height: z.number(), params: z.record(z.string(), z.string()), layout: z.object({ width: z.number(), scrollWidth: z.number(), overflowingControls: z.number() }), read_only_tool_calls: z.array(z.string()), automatic_messages: z.number() })) });
describe("dogfood MCP Apps browser simulator evidence", () => {
  it("binds the simulator screenshots to the current UI sources and expected version", () => {
    const manifest = manifestSchema.parse(JSON.parse(readFileSync(new URL("manifest.json", directory), "utf8")));
    expect(manifest.expected_ui_version).toBe(UI_VERSION);
    expect(manifest.evidence_kind).toContain("not authenticated ChatGPT Desktop acceptance");
    expect(manifest.captures).toHaveLength(20);
    expect(readFileSync(new URL("global-large-host-font.png", directory)).equals(readFileSync(new URL("global-planning-split-light.png", directory)))).toBe(false);
    const source = new URL("../src/", import.meta.url);
    const hash = createHash("sha256");
    for (const name of readdirSync(source).sort()) { hash.update(name); hash.update(readFileSync(new URL(name, source))); }
    expect(manifest.ui_source_sha256).toBe(hash.digest("hex"));
    for (const capture of manifest.captures) {
      const image = readFileSync(new URL(capture.file, directory));
      expect(image.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
      expect(image.readUInt32BE(16)).toBe(capture.width); expect(image.readUInt32BE(20)).toBe(capture.height);
      expect(capture.layout.overflowingControls).toBe(0);
      expect(capture.layout.scrollWidth).toBeLessThanOrEqual(capture.layout.width + 1);
      expect(capture.automatic_messages).toBe(0);
      expect(capture.read_only_tool_calls.every(name => ["get_factory_run", "get_factory_graph", "get_factory_backends", "refresh_factory", "open_factory", "read_factory_delivery", "inspect_factory_issue_graph"].includes(name))).toBe(true);
      // The default-off GitHub reader is only called where the synthetic host configured it.
      if (!["campaign", "home"].includes(capture.params.scenario ?? "")) expect(capture.read_only_tool_calls).not.toContain("read_factory_delivery");
    }
  });
});
