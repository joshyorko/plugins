import { describe, expect, it } from "vitest";
import { inlineAssets } from "../tooling/singlefile";

describe("portable single-file resource", () => {
  it("inlines local JS and CSS without external requests", () => {
    const html = '<html><head><script type="module" crossorigin src="/assets/app.js"></script><link rel="stylesheet" crossorigin href="/assets/app.css"></head><body></body></html>';
    const result = inlineAssets(html, new Map([["assets/app.js", "window.test = '</script>';"], ["assets/app.css", "body { color: green; }"]]));
    expect(result).not.toContain('src="');
    expect(result).not.toContain('href="');
    expect(result).toContain("window.test = '<\\/script>';");
    expect(result).toContain("<style>body { color: green; }</style>");
  });
  it("fails the build rather than shipping an unresolved resource", () => {
    expect(() => inlineAssets('<script type="module" src="/missing.js"></script>', new Map())).toThrow("Unresolved build asset");
  });
});
