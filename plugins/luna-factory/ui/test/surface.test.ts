import { describe, expect, it } from "vitest";
import { surfaceFromHostContext } from "../src/bridge";

describe("ChatGPT surface selection", () => {
  it("uses the declared entrypoint before inline mode and honors host mode changes", () => {
    expect(surfaceFromHostContext({ toolInfo: { tool: { name: "open_factory" } }, displayMode: "fullscreen" })).toBe("global");
    expect(surfaceFromHostContext({ toolInfo: { tool: { name: "open_factory_panel" } }, displayMode: "fullscreen" })).toBe("thread");
    expect(surfaceFromHostContext({ toolInfo: { tool: { name: "get_factory_run" } }, displayMode: "inline" })).toBe("inline");
    expect(surfaceFromHostContext({ displayMode: "fullscreen" })).toBe("global");
  });
});
