import { z } from "zod";

const text = z.string().max(16_000);
const nullableText = text.nullable();
const timestamp = z.number().nonnegative().max(253_402_300_799);
export const runId = z.string().regex(/^[a-zA-Z0-9_-]{1,64}$/);
export const nodeId = z.string().min(1).max(256).refine(value => !/[\u0000-\u001f\u007f-\u009f]/.test(value) && new TextEncoder().encode(value).length <= 256);
export const finishSchema = z.enum(["local_candidate", "push", "pr"]);
const revision = z.number().int().nonnegative().refine(Number.isSafeInteger, "Expected a safe revision integer");
const actionKind = z.enum(["wait", "refresh", "answer", "steer", "cancel", "resume", "reconcile", "inspect"]);
const actionSchema = z.object({ kind: actionKind, label: z.string().min(1).max(160), reason: z.string().max(300), tool: z.string().max(80).nullable(), allowed: z.boolean() });
const controlSchema = z.object({
  schema_version: z.literal(1), revision, intent_generation: revision, dispatch_generation: revision,
  criteria: z.array(z.object({ id: z.string().max(128), description: text, status: z.enum(["proven", "failed", "unproved"]), reason: nullableText, check_refs: z.array(z.string().max(256)).max(64) })).max(32),
  tasks: z.array(z.object({ id: nodeId, title: z.string().max(1000), criterion_ids: z.array(nodeId).max(32), dependencies: z.array(nodeId).max(128), state: z.string().max(64), admission: z.string().max(64), reason: nullableText, owner_thread: nullableText, attempt_ids: z.array(nodeId).max(64) })).max(128),
  attempts: z.array(z.object({ id: nodeId, task_id: nodeId, intent_generation: revision, dispatch_generation: revision, subject: text, thread_id: nullableText, turn_id: nullableText, status: z.string().max(64) })).max(256),
  effects: z.array(z.unknown()).max(128), child_policy: z.literal("cooperative_unverified"),
});
const presentationSchema = z.object({
  revision,
  primary_action: actionSchema,
  actions: z.array(actionSchema).max(16),
  criteria: z.object({ proven: z.number().int().nonnegative().max(10_000), failed: z.number().int().nonnegative().max(10_000), unproved: z.number().int().nonnegative().max(10_000), mandatory: z.number().int().nonnegative().max(10_000) }),
  result: z.object({ kind: z.enum(["finished_verified", "stopped_unresolved", "working", "needs_input", "unverified"]), label: z.string().min(1).max(160) }),
  owner: z.object({ thread_id: nullableText, turn_id: nullableText, liveness: z.enum(["active", "idle", "unknown"]) }),
  workers: z.array(z.object({ thread_id: z.string().max(256), liveness: z.enum(["active", "idle", "unknown"]) })).max(64),
  budget: z.object({ time_remaining_seconds: z.number().int().nonnegative().max(604_800).nullable(), repair_attempts_remaining: z.number().int().nonnegative().max(10_000), repairs_used: z.number().int().nonnegative().max(10_000) }),
  claim: z.object({ held: z.boolean(), status: z.enum(["owned", "released", "foreign", "unknown"]) }),
  deliverable: z.object({ kind: z.enum(["local_candidate", "push", "pr_ready"]), status: z.enum(["verified", "unproved"]), subject: text, reference: nullableText }),
});
const states = ["STARTING", "RUNNING", "VERIFYING", "NEEDS_INPUT", "BLOCKED", "CONVERGED", "QUIESCENT", "CANCELLING", "CANCELLED", "INTERRUPTED", "FAILED"] as const;
export const runSchema = z.object({
  id: runId, repository: z.string().min(1).max(64), objective: text,
  acceptance: z.array(text).max(32), non_goals: z.array(text).max(32),
  finish: finishSchema, profile: z.string().max(64), capacity: z.number().int().min(1).max(8),
  active_workers: z.number().int().nonnegative().max(64), state: z.enum(states),
  current_subject: text, owner_thread: nullableText, turn_id: nullableText,
  delta: text, remaining_gap: nullableText, blocker: nullableText,
  pending_decision: z.object({ id: z.string().min(1).max(128), question: text }).nullish(),
  deadline_at: timestamp, claim_held: z.boolean(),
  planning_only: z.boolean().optional(),
  updated_at: timestamp.optional(), generation: z.number().int().nonnegative().optional(),
  route: z.object({
    requested_model: nullableText, requested_effort: nullableText,
    configured_model: nullableText, configured_effort: nullableText,
    observed_model: nullableText, observed_effort: nullableText,
    requested_provider: nullableText.optional(), configured_provider: nullableText.optional(),
    observed_provider: nullableText.optional(), observed_model_source: nullableText.optional(),
    reroutes: z.array(z.object({ thread_id: text, turn_id: text, from_model: text, to_model: text, reason: text, source: z.literal("model/rerouted") })).max(100).optional(),
  }),
  receipts: z.array(z.object({ subject: text, kind: z.string().max(64), summary: z.string().max(2000), created_at: timestamp })).max(100),
  control: controlSchema.optional(), presentation: presentationSchema.optional(),
});
export type RunView = z.infer<typeof runSchema>;
export const capabilitiesSchema = z.object({
  repositories: z.array(z.object({ alias: z.string().min(1).max(64), max_finish: finishSchema })).max(1000),
  profiles: z.array(z.object({ alias: z.string().min(1).max(64), effort: z.string().max(64), supported: z.boolean().optional() })).max(100).transform(profiles => profiles.filter(profile => profile.supported !== false)),
  limits: z.object({ capacity: z.number().int().min(1).max(8), repair_attempts: z.number().int().min(0).max(10), wall_seconds: z.number().int().min(30).max(86400) }),
  repository_onboarding: z.object({ enabled: z.boolean(), approval: z.literal("local_operator") }).optional(),
  execution: z.object({ eligible: z.boolean(), reason: z.string().max(2000) }).optional(),
  /** Optional read-only GitHub App. Configuration only; reachability is reported per read. */
  github: z.object({ configured: z.boolean(), reason: z.string().max(128) }).optional(),
});
export type Capabilities = z.infer<typeof capabilitiesSchema>;
const repositoryName = z.string().min(1).max(256).refine(value => !/^[A-Za-z]:/.test(value) && !/[\x00-\x1f\x7f]/.test(value) && value.split(/[\\/]/).every(part => part !== "" && part !== "." && part !== ".."), "Expected a relative repository name");
export const repositoryCandidateSchema = z.object({
  id: z.string().regex(/^[a-f0-9]{64}$/), name: repositoryName,
  root_alias: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/), max_finish: finishSchema,
}).strict();
export const repositoryRegistrationSchema = z.object({
  id: runId, alias: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/),
  root_alias: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/), max_finish: finishSchema,
  name: repositoryName,
  status: z.enum(["pending", "approved"]),
}).strict();
export const repositoryDiscoverySchema = z.object({
  candidates: z.array(repositoryCandidateSchema).max(100),
  requests: z.array(repositoryRegistrationSchema).max(100), approval: z.literal("local_operator"),
}).strict();
export type RepositoryDiscovery = z.infer<typeof repositoryDiscoverySchema>;
const graphSourceSchema = z.object({ provider: nodeId, repository_id: nodeId, item_id: nodeId, revision: nodeId });
const graphNodeSchema = controlSchema.shape.tasks.element.extend({ id: nodeId, dependencies: z.array(nodeId).max(128), source: graphSourceSchema.nullable(), target_preference: nodeId.nullable() });
export const graphChangeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("set_target"), node_id: nodeId, target_id: nodeId }),
  z.object({ kind: z.literal("set_dependencies"), node_id: nodeId, dependencies: z.array(nodeId).max(128) }),
  z.object({ kind: z.literal("import_candidates"), nodes: z.array(z.object({ id: nodeId, title: z.string().min(1).max(4000), criterion_ids: z.array(nodeId).min(1).max(32), dependencies: z.array(nodeId).max(128), source: graphSourceSchema })).min(1).max(32) }),
]);
const proposalSchema = z.object({ id: nodeId, idempotency_key: nodeId, fingerprint: z.string().max(256), actor: z.literal("local_operator"), base_revision: revision, subject: text, change: graphChangeSchema, status: z.enum(["proposed", "applied"]), applied_revision: revision.nullable() });
export const graphEnvelopeSchema = z.object({
  graph: z.object({ run_id: runId, revision, repository: z.object({ alias: z.string().max(64), identity: z.string().max(256), base_head: text, subject: text }), planning_only: z.boolean(), nodes: z.array(graphNodeSchema).max(128), criteria: controlSchema.shape.criteria, attempts: controlSchema.shape.attempts, claim: presentationSchema.shape.claim, changes: z.array(proposalSchema).max(256) }),
  proposal: proposalSchema.nullable(),
});
export type GraphEnvelope = z.infer<typeof graphEnvelopeSchema>;
export type FactoryGraph = GraphEnvelope["graph"];
export type GraphChange = z.infer<typeof graphChangeSchema>;
const count = z.number().int().nonnegative().max(10_000_000);
const shortCode = z.string().max(32).nullable();
const githubText = (max: number) => z.string().max(max);
const deliveryPullSchema = z.object({
  number: z.number().int().positive().max(1_000_000_000), url: githubText(512).refine(value => value.startsWith("https://"), "Expected an HTTPS URL"),
  state: shortCode, draft: z.boolean(), head_sha: z.string().regex(/^[a-fA-F0-9]{40}$/).nullable(), review_decision: shortCode,
  diff: z.object({ files_changed: count.nullable(), additions: count.nullable(), deletions: count.nullable() }),
  checks: z.object({
    rollup: shortCode, total: count, truncated: z.boolean(),
    runs: z.array(z.object({ name: githubText(200), status: shortCode, conclusion: shortCode, started_at: timestamp.nullable(), completed_at: timestamp.nullable(), summary: githubText(280).nullable() })).max(25),
    statuses: z.array(z.object({ context: githubText(200), state: shortCode, created_at: timestamp.nullable() })).max(25),
  }),
  ready_for_review: z.boolean(), merge_authority: z.literal(false), observed_at: timestamp, reported_by: z.literal("github"),
});
const deliveryNodeSchema = z.object({
  node_id: nodeId, item_id: nodeId, reported_by: z.literal("github"),
  status: z.enum(["observed", "unavailable", "deferred"]), freshness: z.enum(["fresh", "cached", "stale"]).nullable(),
  reason: z.string().max(128).nullable(), observed_at: timestamp.nullable(),
  issue: z.object({ number: z.number().int().positive(), url: githubText(512), state: shortCode }).nullable(),
  pull_requests: z.array(deliveryPullSchema).max(5), pull_requests_total: count, pull_requests_truncated: z.boolean(),
});
/** `read_factory_delivery`: GitHub-reported display data. Never proof, attention or an action. */
export const deliverySchema = z.object({
  schema_version: z.literal(1), run_id: runId, graph_revision: revision, reported_by: z.literal("github"),
  proof: z.literal("none"), merge_capability: z.literal("none"), available: z.boolean(),
  reason: z.string().max(128).nullable(), retry_at: timestamp.nullable(), min_interval_seconds: z.literal(60),
  observed_at: timestamp, nodes: z.array(deliveryNodeSchema).max(128),
});
export type Delivery = z.infer<typeof deliverySchema>;
export type DeliveryNode = Delivery["nodes"][number];
export type DeliveryPull = DeliveryNode["pull_requests"][number];
export const UI_VERSION = "0.2.1";
export type FollowUpKind = "summary" | "blocker" | "choose";
export type FollowUpTask = Pick<FactoryGraph["nodes"][number], "id" | "title">;
export function buildFollowUpPrompt(run: RunView, kind: FollowUpKind, task?: FollowUpTask, revision?: number): string {
  const heading = kind === "summary"
    ? "Summarize the current Luna Factory run. Do not start work."
    : kind === "blocker"
      ? "Analyze the current Luna Factory blocker and suggest a safe next step. Do not approve or dispatch work."
      : "Help me choose a bounded next step for this Luna Factory task. Do not approve or dispatch work.";
  const selectedRevision = revision ?? run.control?.revision ?? run.presentation?.revision ?? 0;
  const identity = [
    heading,
    `Run ID: ${run.id}`,
    `Revision: ${selectedRevision}`,
    ...(task ? [`Task ID: ${task.id}`, `Task: ${task.title.slice(0, 360)}`] : []),
  ].join("\n");
  const context = [
    `State: ${boundedContext(run).state.slice(0, 64)}`,
    `Objective: ${run.objective.slice(0, 600)}`,
    ...(run.blocker ? [`Blocker: ${run.blocker.slice(0, 360)}`] : []),
    ...(run.remaining_gap ? [`Remaining gap: ${run.remaining_gap.slice(0, 360)}`] : []),
    ...(run.delta ? [`Latest change: ${run.delta.slice(0, 360)}`] : []),
  ].join("\n");
  return `${identity}\n${context.slice(0, Math.max(0, 1800 - identity.length - 1))}`;
}
const operationSchema = z.object({ advertised: z.boolean(), enabled: z.boolean(), qualified: z.boolean() });
export const backendCatalogSchema = z.object({ schema_version: z.literal(1), discovery: z.literal("configuration_only"), policy: z.literal("subscription_only"), targets: z.array(z.object({ id: z.string().max(128), label: z.string().max(160), kind: z.string().max(128), namespace: z.string().max(256), operator_enabled: z.boolean(), planning_eligible: z.boolean(), execution_eligible: z.boolean(), qualification: z.enum(["unverified", "unsupported", "qualified"]), authentication: z.string().max(128), entitlement: z.string().max(128), reason: z.string().max(2000), operations: z.object({ discover: operationSchema, start: operationSchema, observe: operationSchema, steer: operationSchema, stop: operationSchema, reconcile: operationSchema }), limits: z.array(z.string().max(1000)).max(32) })).max(32) });
export type BackendCatalog = z.infer<typeof backendCatalogSchema>;
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
  if (workbench.success) {
    for (const run of [...workbench.data.runs, ...(workbench.data.selected_run ? [workbench.data.selected_run] : [])]) {
      validateProjection(run);
    }
    return { kind: "workbench", value: workbench.data };
  }
  const run = runSchema.safeParse(structured);
  if (run.success) {
    validateProjection(run.data);
    return { kind: "run", value: run.data };
  }
  throw new Error("The server returned an unrecognized result. Your last valid view is preserved.");
}
function validateProjection(run: RunView): void {
  const { control, presentation } = run;
  if (!control || !presentation) return;
  if (control.revision !== presentation.revision) throw new Error("The server returned mismatched control revisions. Refresh to read a current view.");
  const actual = control.criteria.reduce((counts, criterion) => ({ ...counts, [criterion.status]: counts[criterion.status] + 1 }), { proven: 0, failed: 0, unproved: 0 });
  if (presentation.criteria.mandatory !== control.criteria.length || presentation.criteria.proven !== actual.proven || presentation.criteria.failed !== actual.failed || presentation.criteria.unproved !== actual.unproved) {
    throw new Error("The server returned inconsistent criterion counts. Refresh to read a current view.");
  }
  if (presentation.result.kind === "finished_verified" && (actual.failed > 0 || actual.unproved > 0 || actual.proven !== presentation.criteria.mandatory)) {
    throw new Error("The server marked unresolved criteria as finished. Refresh to read a current view.");
  }
}
export function needsOperatorDecision(run: RunView): boolean {
  const action = run.presentation?.primary_action;
  if (run.planning_only || !run.control || !run.presentation || run.control.revision !== run.presentation.revision || !action?.allowed) return false;
  return Boolean(run.pending_decision && action.kind === "answer" && action.tool === "resume_factory_run")
    || action.kind === "inspect" && action.tool === "get_factory_run" && action.reason === "native_approval_requires_native_ui";
}
export function classifyRun(run: RunView): "needs" | "active" | "recent" {
  if (needsOperatorDecision(run)) return "needs";
  if (!run.planning_only && run.control && run.presentation?.result.kind === "working") return "active";
  // Plans, blockers and uncertain history stay inspectable without inventing a decision.
  return "recent";
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
export type FactoryLink = { runId: string; taskId?: string; revision?: number };
export function parseFactoryLink(value: string): FactoryLink | null {
  if (!value.startsWith("/runs/") || value.includes("#")) return null;
  const [path, query = ""] = value.split("?");
  if (!path) return null;
  const match = /^\/runs\/([^/]+)$/.exec(path);
  if (!match?.[1]) return null;
  let decodedRunId: string;
  try { decodedRunId = decodeURIComponent(match[1]); } catch { return null; }
  const parsedRunId = runId.safeParse(decodedRunId);
  if (!parsedRunId.success) return null;
  if (!query) return { runId: parsedRunId.data };
  const fields = new Map<string, string>();
  for (const item of query.split("&")) {
    const [rawKey, rawValue, extra] = item.split("=");
    if (!rawKey || rawValue === undefined || extra !== undefined) return null;
    let key: string; let value: string;
    try { key = decodeURIComponent(rawKey); value = decodeURIComponent(rawValue); } catch { return null; }
    if ((key !== "task" && key !== "revision") || fields.has(key)) return null;
    fields.set(key, value);
  }
  const task = fields.get("task");
  const revisionText = fields.get("revision");
  if (task === undefined && revisionText === undefined) return null;
  if (task === undefined || revisionText === undefined || !/^(0|[1-9]\d*)$/.test(revisionText)) return null;
  const parsedTask = nodeId.safeParse(task);
  const revision = Number(revisionText);
  if (!parsedTask.success || !Number.isSafeInteger(revision)) return null;
  return { runId: parsedRunId.data, taskId: parsedTask.data, revision };
}
export function runPath(id: string): string { return `/runs/${encodeURIComponent(id)}`; }
const bound = (value: string | null, length: number): string | null => value === null ? null : value.slice(0, length);
export function boundedContext(run: RunView) {
  const projectedState = run.control && run.presentation
    ? ({ finished_verified: "CONVERGED", stopped_unresolved: "STOPPED_UNRESOLVED", working: run.state === "CONVERGED" ? "WORKING" : run.state, needs_input: "NEEDS_INPUT", unverified: "UNVERIFIED" } satisfies Record<NonNullable<RunView["presentation"]>["result"]["kind"], string>)[run.presentation.result.kind]
    : run.state === "CONVERGED" ? "UNVERIFIED" : run.state;
  return {
    run_id: run.id, revision: run.control?.revision ?? run.presentation?.revision ?? null, repository: run.repository, objective: run.objective.slice(0, 1200), state: run.planning_only ? "PLANNING" : projectedState,
    current_subject: run.current_subject.slice(0, 180), remaining_mandatory_gap: bound(run.remaining_gap, 800),
    blocker: bound(run.blocker, 600), finish: run.finish,
  };
}
export const finishLabels = { local_candidate: "Local candidate", push: "Push branch", pr: "Create PR" } satisfies Record<RunView["finish"], string>;
const finishRank = { local_candidate: 0, push: 1, pr: 2 } satisfies Record<RunView["finish"], number>;
export function allowedFinishes(max: RunView["finish"]): RunView["finish"][] {
  return finishSchema.options.filter(finish => finishRank[finish] <= finishRank[max]);
}
const lines = (value: string, field: "acceptance" | "non_goals"): string[] => {
  const heading = field === "acceptance" ? /^(?:mandatory\s+)?acceptance(?:\s+criteria)?\s*:?$/i : /^non[ -]?goals\s*:?$/i;
  return value.split(/\r?\n/).map(line => line.trim())
    .filter(line => !heading.test(line.replace(/^#{1,6}\s+/, "").replace(/^\*\*(.*?)\*\*$/, "$1").trim()))
    .map(line => line.replace(/^(?:[-*+•]|\d+[.)])\s+/, "").trim()).filter(Boolean);
};
const startSchema = z.object({
  repository: z.string().min(1).max(64), objective: z.string().trim().min(1, "Enter an objective").max(8000),
  acceptance: z.array(z.string().min(1).max(1000)).min(1, "Add at least one acceptance criterion").max(32),
  non_goals: z.array(z.string().min(1).max(1000)).max(32), finish: finishSchema, profile: z.string().min(1).max(64),
  capacity: z.coerce.number().int().min(1).max(8), repair_attempts: z.coerce.number().int().min(0).max(10),
  wall_seconds: z.coerce.number().int().min(30).max(86400), idempotency_key: z.string().min(1).max(128),
});
export function startRequest(fields: Record<string, string>, capabilities: Capabilities, key: string) {
  const result = startSchema.safeParse({ ...fields, acceptance: lines(fields.acceptance ?? "", "acceptance"), non_goals: lines(fields.non_goals ?? "", "non_goals"), idempotency_key: key });
  if (!result.success) throw new Error(result.error.issues[0]?.message || "Check the run fields");
  const request = result.data;
  const repository = capabilities.repositories.find(repo => repo.alias === request.repository);
  if (!repository || !allowedFinishes(repository.max_finish).includes(request.finish)) throw new Error("Choose an approved repository and finish authority");
  if (!capabilities.profiles.some(profile => profile.alias === request.profile)) throw new Error("Choose an approved runtime profile");
  const limits = capabilities.limits;
  if (request.capacity > limits.capacity || request.repair_attempts > limits.repair_attempts || request.wall_seconds > limits.wall_seconds) throw new Error("The requested budget exceeds the configured limits");
  return request;
}
