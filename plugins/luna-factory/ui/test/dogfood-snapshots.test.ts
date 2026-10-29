import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { UI_VERSION } from "../src/domain";

const directory = new URL("./dogfood-visual-snapshots/", import.meta.url);
const manifestSchema = z.object({ expected_ui_version: z.string(), evidence_kind: z.string(), ui_source_sha256: z.string(), dist_sha256: z.string(), captures: z.array(z.object({ file: z.string(), width: z.number(), height: z.number(), params: z.record(z.string(), z.string()), layout: z.object({ width: z.number(), scrollWidth: z.number(), overflowingControls: z.array(z.object({ tag: z.string(), label: z.string(), left: z.number(), right: z.number(), width: z.number() })) }), read_only_tool_calls: z.array(z.string()), automatic_messages: z.number() })) });
describe("dogfood MCP Apps browser simulator evidence", () => {
  it("preserves simulator provenance across the exact nonvisual promotion parser update", () => {
    const manifest = manifestSchema.parse(JSON.parse(readFileSync(new URL("manifest.json", directory), "utf8")));
    expect(manifest.expected_ui_version).toBe(UI_VERSION);
    expect(manifest.evidence_kind).toContain("not authenticated ChatGPT Desktop acceptance");
    expect(manifest.captures).toHaveLength(35);
    expect(readFileSync(new URL("global-large-host-font.png", directory)).equals(readFileSync(new URL("global-planning-split-light.png", directory)))).toBe(false);
    const source = new URL("../src/", import.meta.url);
    const compatibility = z.object({ evidence_kind: z.literal("Nonvisual promotion parser compatibility; no new browser captures"), ui_source_sha256: z.string(), dist_sha256: z.string() })
      .parse(JSON.parse(readFileSync(new URL("./execution-compatibility.json", import.meta.url), "utf8")));
    const hash = createHash("sha256");
    const captured = createHash("sha256");
    for (const name of readdirSync(source).sort()) {
      const current = readFileSync(new URL(name, source), "utf8");
      const previous = name === "domain.ts" ? current.replace("promotion: z.object({ allowed: z.boolean()", "promotion: z.object({ allowed: z.literal(false)") : current;
      hash.update(name); hash.update(current);
      captured.update(name); captured.update(previous);
    }
    // Every captured render/style input is byte-identical. Only the one parser
    // predicate differs. The historical manifest and PNGs retain their original hashes.
    expect(captured.digest("hex")).toBe(manifest.ui_source_sha256);
    expect(hash.digest("hex")).toBe(compatibility.ui_source_sha256);
    expect(compatibility.dist_sha256).toBe(createHash("sha256").update(readFileSync(new URL("../dist/index.html", import.meta.url))).digest("hex"));
    for (const capture of manifest.captures) {
      const image = readFileSync(new URL(capture.file, directory));
      expect(image.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
      expect(image.readUInt32BE(16)).toBe(capture.width); expect(image.readUInt32BE(20)).toBe(capture.height);
      expect(capture.layout.overflowingControls).toHaveLength(0);
      expect(capture.layout.scrollWidth).toBeLessThanOrEqual(capture.layout.width + 1);
      expect(capture.automatic_messages).toBe(0);
      expect(capture.read_only_tool_calls.every(name => ["get_factory_run", "get_factory_graph", "get_factory_backends", "refresh_factory", "open_factory", "list_factory_campaigns", "read_factory_agent_timeline", "read_factory_delivery", "inspect_factory_issue_graph", "list_factory_github_repositories"].includes(name))).toBe(true);
      if (capture.params.scenario === "home") expect(capture.read_only_tool_calls).toContain("list_factory_campaigns");
      // The default-off GitHub reader is only called where the synthetic host configured it.
      if (!["campaign", "home"].includes(capture.params.scenario ?? "")) expect(capture.read_only_tool_calls).not.toContain("read_factory_delivery");
    }
  });
});
