import test from "node:test";
import assert from "node:assert/strict";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

test("stdio MCP surface exposes three read-only tools and the v2 UI resource", async () => {
  const client = new Client({ name: "factory-protocol-test", version: "1.0.0" });
  const transport = new StdioClientTransport({ command: process.execPath, args: ["server.mjs", "--stdio"], cwd: import.meta.dirname });
  await client.connect(transport);
  const tools = await client.listTools();
  assert.deepEqual(tools.tools.map((tool) => tool.name), ["factory_snapshot", "factory_plan", "factory_prompt"]);
  for (const tool of tools.tools) assert.equal(tool.annotations?.destructiveHint, false);
  const snapshot = await client.callTool({ name: "factory_snapshot", arguments: { repository: "owner/repo", targetBranch: "main", observedHead: "abc" } });
  assert.ok(snapshot.structuredContent);
  assert.match(snapshot.content[0].text, /UNKNOWN/);
  const resource = await client.readResource({ uri: "ui://software-factory/control-room-v2.html" });
  assert.equal(resource.contents[0].mimeType, "text/html;profile=mcp-app");
  assert.match(resource.contents[0].text, /Factory Control Room/);
  await client.close();
});
