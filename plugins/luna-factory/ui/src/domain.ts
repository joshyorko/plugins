import { z } from "zod";

const text = z.string().max(16_000);
const nullableText = text.nullable();
const timestamp = z.number().nonnegative().max(253_402_300_799);
export const runId = z.string().regex(/^[a-zA-Z0-9_-]{1,64}$/);
export const finishSchema = z.enum(["local_candidate", "push", "pr"]);
const states = ["STARTING", "RUNNING", "VERIFYING", "NEEDS_INPUT", "BLOCKED", "CONVERGED", "QUIESCENT", "CANCELLING", "CANCELLED", "INTERRUPTED", "FAILED"] as const;
export const runSchema = z.object({
  id: runId, repository: z.string().min(1).max(64), objective: text,
  acceptance: z.array(text).max(32), non_goals: z.array(text).max(32),
  finish: finishSchema, profile: z.string().max(64), capacity: z.number().int().min(1).max(8),
  active_workers: z.number().int().nonnegative().max(64), state: z.enum(states),
  current_subject: text, owner_thread: nullableText, turn_id: nullableText,
  delta: text, remaining_gap: nullableText, blocker: nullableText,
  deadline_at: timestamp, claim_held: z.boolean(),
  updated_at: timestamp.optional(), generation: z.number().int().nonnegative().optional(),
  route: z.object({ requested_model: nullableText, requested_effort: nullableText, configured_model: nullableText, configured_effort: nullableText, observed_model: nullableText, observed_effort: nullableText }),
  receipts: z.array(z.object({ subject: text, kind: z.string().max(64), summary: z.string().max(2000), created_at: timestamp })).max(100),
});
export type RunView = z.infer<typeof runSchema>;
export const capabilitiesSchema = z.object({
  repositories: z.array(z.object({ alias: z.string().min(1).max(64), max_finish: finishSchema })).max(1000),
  profiles: z.array(z.object({ alias: z.string().min(1).max(64), effort: z.string().max(64), supported: z.boolean().optional() })).max(100).transform(profiles => profiles.filter(profile => profile.supported !== false)),
  limits: z.object({ capacity: z.number().int().min(1).max(8), repair_attempts: z.number().int().min(0).max(10), wall_seconds: z.number().int().min(30).max(86400) }),
});
export type Capabilities = z.infer<typeof capabilitiesSchema>;
export const settingsSchema = z.object({ capacity: z.number().int().min(1).max(8).optional(), finish: finishSchema.optional(), profile: z.string().max(64).optional() });
export type Settings = z.infer<typeof settingsSchema>;
const summarySchema = runSchema.extend({ receipts: runSchema.shape.receipts.default([]) });
const workbenchSchema = z.object({ runs: z.array(summarySchema).max(100), capabilities: capabilitiesSchema, selected_run: runSchema.nullish(), settings: settingsSchema });
export type Workbench = z.infer<typeof workbenchSchema>;
export type ToolData = { kind: "workbench"; value: Workbench } | { kind: "run"; value: RunView };

export function structuredResult(value: unknown): unknown {
  const envelope = z.object({ structuredContent: z.unknown().optional(), isError: z.boolean().optional(), content: z.array(z.object({ type: z.string(), text: z.string().optional() }).passthrough()).optional() }).safeParse(value);
  if (!envelope.success) throw new Error("The server returned an unrecognized result. Refresh to try again.");
  if (envelope.data.isError) {
    const message = envelope.data.content?.find(item => item.type === "text")?.text;
    throw new Error(message?.slice(0, 600) || "The server could not complete this request.");
  }
  return envelope.data.structuredContent;
}
export function parseToolResult(value: unknown): ToolData {
  const structured = structuredResult(value);
  const workbench = workbenchSchema.safeParse(structured);
  if (workbench.success) return { kind: "workbench", value: workbench.data };
  const run = runSchema.safeParse(structured);
  if (run.success) return { kind: "run", value: run.data };
  throw new Error("The server returned an unrecognized result. Your last valid view is preserved.");
}
export function classifyRun(run: RunView): "needs" | "active" | "recent" {
  if (["NEEDS_INPUT", "BLOCKED", "QUIESCENT", "INTERRUPTED", "FAILED"].includes(run.state)) return "needs";
  if (["CONVERGED", "CANCELLED"].includes(run.state)) return "recent";
  return "active";
}
export function parseRunLink(value: string): string | null {
  // Validate the raw path before URL normalization can erase traversal segments.
  if (!value.startsWith("/runs/") || value.includes("#")) return null;
  const path = value.split("?")[0];
  if (!path) return null;
  const match = /^\/runs\/([^/]+)$/.exec(path);
  if (!match?.[1]) return null;
  try { const id = runId.safeParse(decodeURIComponent(match[1])); return id.success ? id.data : null; } catch { return null; }
}
export function runPath(id: string): string { return `/runs/${encodeURIComponent(id)}`; }
const bound = (value: string | null, length: number): string | null => value === null ? null : value.slice(0, length);
export function boundedContext(run: RunView) {
  return {
    run_id: run.id, repository: run.repository, objective: run.objective.slice(0, 1200), state: run.state,
    current_subject: run.current_subject.slice(0, 180), remaining_mandatory_gap: bound(run.remaining_gap, 800),
    blocker: bound(run.blocker, 600), finish: run.finish,
  };
}
export const finishLabels = { local_candidate: "Local candidate", push: "Push branch", pr: "Create PR" } satisfies Record<RunView["finish"], string>;
const finishRank = { local_candidate: 0, push: 1, pr: 2 } satisfies Record<RunView["finish"], number>;
export function allowedFinishes(max: RunView["finish"]): RunView["finish"][] {
  return finishSchema.options.filter(finish => finishRank[finish] <= finishRank[max]);
}
const lines = (value: string): string[] => value.split(/\r?\n/).map(line => line.trim()).filter(Boolean);
const startSchema = z.object({
  repository: z.string().min(1).max(64), objective: z.string().trim().min(1, "Enter an objective").max(8000),
  acceptance: z.array(z.string().min(1).max(1000)).min(1, "Add at least one acceptance criterion").max(32),
  non_goals: z.array(z.string().min(1).max(1000)).max(32), finish: finishSchema, profile: z.string().min(1).max(64),
  capacity: z.coerce.number().int().min(1).max(8), repair_attempts: z.coerce.number().int().min(0).max(10),
  wall_seconds: z.coerce.number().int().min(30).max(86400), idempotency_key: z.string().min(1).max(128),
});
export function startRequest(fields: Record<string, string>, capabilities: Capabilities, key: string) {
  const result = startSchema.safeParse({ ...fields, acceptance: lines(fields.acceptance ?? ""), non_goals: lines(fields.non_goals ?? ""), idempotency_key: key });
  if (!result.success) throw new Error(result.error.issues[0]?.message || "Check the run fields");
  const request = result.data;
  const repository = capabilities.repositories.find(repo => repo.alias === request.repository);
  if (!repository || !allowedFinishes(repository.max_finish).includes(request.finish)) throw new Error("Choose an approved repository and finish authority");
  if (!capabilities.profiles.some(profile => profile.alias === request.profile)) throw new Error("Choose an approved runtime profile");
  const limits = capabilities.limits;
  if (request.capacity > limits.capacity || request.repair_attempts > limits.repair_attempts || request.wall_seconds > limits.wall_seconds) throw new Error("The requested budget exceeds the configured limits");
  return request;
}
