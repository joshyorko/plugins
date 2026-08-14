import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { registerAppResource, registerAppTool, RESOURCE_MIME_TYPE } from "@modelcontextprotocol/ext-apps/server";
import { z } from "zod";
import { classifyOutcome, countStatuses, diffPlanGenerations, makePrompt, normalizeControlIntent, DEFAULT_PRESET } from "./domain.mjs";

const exec = promisify(execFile);
const resourceUri = "ui://software-factory/control-room-v2.html";
const targetSchema = z.object({ repository: z.string(), targetBranch: z.string(), observedHead: z.string() });
const presetSchema = z.object({ executor: z.string(), model: z.string(), effort: z.string() }).default(DEFAULT_PRESET);
const intentSchema = z.object({ action: z.string(), target: targetSchema, planGeneration: z.string().nullable().optional(), orchestrationPreset: presetSchema.optional(), constraints: z.record(z.string(), z.unknown()).optional() });

async function readOnlyCommand(command, args, cwd) {
  if (!new Set(["git", "gh"]).has(command)) return { status: "UNKNOWN", reason: "Command is not on the read-only allowlist." };
  try { const result = await exec(command, args, { cwd, timeout: 5000 }); return { status: "KNOWN", text: result.stdout.trim() }; }
  catch (error) { return { status: "UNKNOWN", reason: `${command} observation unavailable: ${error.message}` }; }
}

export async function observeSnapshot(input = {}) {
  const repository = input.repository ?? "UNKNOWN";
  const branch = input.targetBranch ?? "UNKNOWN";
  const head = input.observedHead ?? "UNKNOWN";
  const git = input.cwd ? await readOnlyCommand("git", ["status", "--short", "--branch"], input.cwd) : { status: "UNKNOWN", reason: "No observation cwd was supplied." };
  const items = Array.isArray(input.items) ? input.items : [];
  return { repository, targetBranch: branch, observedHead: head, status: git.status === "KNOWN" ? "KNOWN" : "UNKNOWN", statuses: countStatuses(items), candidates: items, reasons: [git.reason ?? "Read-only repository observation supplied."] , evidence: git.text ?? null, resumePointer: null };
}

function textResult(value) { return { content: [{ type: "text", text: JSON.stringify(value, null, 2) }], structuredContent: value }; }

export function createServer({ cwd } = {}) {
  const server = new McpServer({ name: "software-factory-control-room", version: "2.0.0" });
  registerAppTool(server, "factory_snapshot", { title: "Factory snapshot", description: "Read-only repository and factory observation. Missing evidence is UNKNOWN.", inputSchema: targetSchema.extend({ cwd: z.string().optional(), items: z.array(z.record(z.string(), z.unknown())).optional() }), annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: true }, _meta: { ui: { resourceUri } } }, async (args) => textResult(await observeSnapshot({ ...args, cwd: args.cwd ?? cwd })));
  registerAppTool(server, "factory_plan", { title: "Factory plan", description: "Deterministically validates supplied planning evidence; semantic decisions remain INVESTIGATE_UNKNOWN.", inputSchema: z.object({ goal: z.string(), existing: z.array(z.record(z.string(), z.unknown())).optional(), previous: z.record(z.string(), z.unknown()).optional(), next: z.record(z.string(), z.unknown()).optional() }), annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: { ui: { resourceUri } } }, async (args) => { const outcome = classifyOutcome({ goal: args.goal, architectureUnknown: !args.existing }, args.existing ?? []); const diff = args.previous && args.next ? diffPlanGenerations(args.previous, args.next) : null; return textResult({ goal: args.goal, outcomes: [{ id: "outcome-1", desiredPredicate: args.goal, classification: outcome.classification, reason: outcome.reason, existingWorkReference: outcome.reference ?? null }], generation: args.next?.generation ?? "draft", observedRepositoryHead: args.next?.observedRepositoryHead ?? "UNKNOWN", diff }); });
  registerAppTool(server, "factory_prompt", { title: "Factory prompt", description: "Generates a read-only supervisor prompt preview. It never executes it.", inputSchema: intentSchema.extend({ snapshot: z.record(z.string(), z.unknown()).optional() }), annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: { ui: { resourceUri } } }, async (args) => { const intent = normalizeControlIntent(args); return textResult({ intent, prompt: makePrompt(intent, args.snapshot), executed: false }); });
  registerAppResource(server, "software-factory-control-room", resourceUri, { mimeType: RESOURCE_MIME_TYPE }, async () => ({ contents: [{ uri: resourceUri, mimeType: RESOURCE_MIME_TYPE, text: await readFile(join(import.meta.dirname, "dist/control-room.html"), "utf8") }] }));
  return server;
}

if (process.argv.includes("--stdio")) await createServer().connect(new StdioServerTransport());
