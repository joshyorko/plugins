import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { z } from "zod";

const manifestSchema = z.object({
  expected_ui_version: z.string(),
  evidence_kind: z.string(),
  captures: z.array(z.object({ file: z.string(), width: z.number(), height: z.number(), params: z.record(z.string(), z.string()) })),
});
const directory = new URL("./visual-snapshots/", import.meta.url);

describe("synthetic host visual snapshots", () => {
  it("records the expected UI version and distinguishes simulator evidence from ChatGPT", () => {
    const manifest = manifestSchema.parse(JSON.parse(readFileSync(new URL("manifest.json", directory), "utf8")));
    expect(manifest.expected_ui_version).toBe("0.2.1");
    expect(manifest.evidence_kind).toContain("not ChatGPT Desktop acceptance");
    expect(manifest.captures.map(capture => capture.file)).toHaveLength(6);
    expect(manifest.captures.some(capture => capture.params.surface === "thread")).toBe(true);
    expect(manifest.captures.some(capture => capture.params.platform === "mobile")).toBe(true);
    expect(manifest.captures.some(capture => capture.params.surface === "inline" && capture.params.platform === "mobile")).toBe(true);
  });

  it("contains complete PNG screenshots for inline, fullscreen, thread, dark, and error states", () => {
    const manifest = manifestSchema.parse(JSON.parse(readFileSync(new URL("manifest.json", directory), "utf8")));
    for (const capture of manifest.captures) {
      const image = readFileSync(new URL(capture.file, directory));
      expect(image.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
      expect(image.length).toBeGreaterThan(10_000);
    }
  });
});
