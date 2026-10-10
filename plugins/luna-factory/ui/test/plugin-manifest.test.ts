import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { UI_VERSION } from "../src/domain";

const read = (url: URL): string => readFileSync(url, "utf8");
const onboardingSchema = z.object({ extensions: z.object({ "com.openai": z.object({ onboardingSkill: z.string() }) }) });
const manifestSchema = onboardingSchema.extend({ version: z.string(), extensions: onboardingSchema.shape.extensions.extend({ "com.openai": onboardingSchema.shape.extensions.shape["com.openai"].extend({ interface: z.object({ brandColor: z.string() }) }) }) });
const manifest = manifestSchema.parse(JSON.parse(read(new URL("../../plugin.json", import.meta.url))));
const compatibilityManifest = onboardingSchema.parse(JSON.parse(read(new URL("../../.codex-plugin/plugin.json", import.meta.url))));

describe("ChatGPT plugin package", () => {
  it("keeps the shipped workbench identity on the package version", () => {
    expect(UI_VERSION).toBe(manifest.version);
    expect(manifest.version).toBe("0.2.1");
  });

  it("aligns the plugin brand color with the workbench accent", () => {
    const css = read(new URL("../src/style.css", import.meta.url));
    const accent = css.match(/--accent:\s*(#[0-9a-f]{6})/i)?.[1];
    expect(accent).toBeDefined();
    expect(manifest.extensions["com.openai"].interface.brandColor.toLowerCase()).toBe(accent?.toLowerCase());
  });

  it("maps setup onboarding only to the packaged safe setup skill", () => {
    const skillPath = manifest.extensions["com.openai"].onboardingSkill;
    expect(skillPath).toBe("./skills/setup/SKILL.md");
    expect(compatibilityManifest.extensions["com.openai"].onboardingSkill).toBe(skillPath);
    if (!skillPath) throw new Error("The setup skill is not packaged");
    const skill = read(new URL(`../../${skillPath.slice(2)}`, import.meta.url));
    expect(skill).toContain("Do not print or paste credentials");
    expect(skill).toContain("does not change protected");
  });
});
