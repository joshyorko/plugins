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
/**
 * Copy and placement category from the server's existing state and reason codes. It never grants
 * an action or creates attention. Kinds this client does not know (or malformed values) read as
 * `unknown`, never as a guessed category.
 */
const blockerKindSchema = z.union([
  z.object({ kind: z.literal("budget_exhausted"), budget: z.enum(["time", "repair"]) }),
  z.object({ kind: z.enum(["diagnosis_required", "native_approval", "effect_outcome_unknown", "liveness_unknown", "planning_only", "none", "unknown"]) }),
]).catch({ kind: "unknown" as const });
export type BlockerKind = z.infer<typeof blockerKindSchema>;
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
  blocker_kind: blockerKindSchema.optional(),
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
  /** Admission time. `activity_at` is the newest of `updated_at` and retained receipts; graph edits carry no time. */
  created_at: timestamp.optional(), activity_at: timestamp.optional(),
  route: z.object({
    requested_model: nullableText, requested_effort: nullableText,
    configured_model: nullableText, configured_effort: nullableText,
    observed_model: nullableText, observed_effort: nullableText,
    requested_provider: nullableText.optional(), configured_provider: nullableText.optional(),
    observed_provider: nullableText.optional(), observed_model_source: nullableText.optional(),
    reroutes: z.array(z.object({ thread_id: text, turn_id: text, from_model: text, to_model: text, reason: text, source: z.literal("model/rerouted") })).max(100).optional(),
  }),
  // `thread_id` is the native thread the server recorded at receipt time. Absent on older servers;
  // null or malformed means unattributed. It is never inferred here.
  receipts: z.array(z.object({ subject: text, kind: z.string().max(64), summary: z.string().max(2000), created_at: timestamp, thread_id: nodeId.nullable().optional().catch(null) })).max(100),
  control: controlSchema.optional(), presentation: presentationSchema.optional(),
});
export type RunView = z.infer<typeof runSchema>;
export const capabilitiesSchema = z.object({
  /** `provenance` is optional and display-only: how an approved alias was added, never authority. */
  repositories: z.array(z.object({ alias: z.string().min(1).max(64), max_finish: finishSchema, provenance: z.enum(["github_app_installation", "local_create"]).optional().catch(undefined) })).max(1000),
  profiles: z.array(z.object({ alias: z.string().min(1).max(64), effort: z.string().max(64), supported: z.boolean().optional() })).max(100).transform(profiles => profiles.filter(profile => profile.supported !== false)),
  limits: z.object({ capacity: z.number().int().min(1).max(8), repair_attempts: z.number().int().min(0).max(10), wall_seconds: z.number().int().min(30).max(86400) }),
  repository_onboarding: z.object({ enabled: z.boolean(), approval: z.literal("local_operator") }).optional(),
  execution: z.object({ eligible: z.boolean(), reason: z.string().max(2000) }).optional(),
  /** Present only on servers that provide read_factory_agent_timeline. */
  agent_timeline: z.object({ source: z.literal("codex-action-server"), enabled: z.boolean(), detail: z.string().max(300) }).optional(),
  /** Optional read-only GitHub App. Configuration only; reachability is reported per read. */
  github: z.object({ configured: z.boolean(), reason: z.string().max(128) }).optional(),
  /**
   * Operator switches for adding repositories from the plan form. Present only on servers with
   * list/connect/create; absent or malformed reads as "not offered", so those tools are never called.
   */
  workspaces: z.object({ connect_github: z.boolean(), create_local: z.boolean() }).optional().catch(undefined),
});
export type Capabilities = z.infer<typeof capabilitiesSchema>;
export const observedKinds = ["assigned", "command", "file_change", "commit", "pr_opened", "check_result", "subagent_spawned", "message", "waiting", "error"] as const;
const threadId = z.string().min(1).max(256);
/** "Observed in Codex" for one agent. Display data only: never proof, attention or plan state. */
export const agentTimelineSchema = z.object({
  schema_version: z.literal(1), run_id: runId, thread_id: threadId,
  observed: z.boolean(), status: z.enum(["observed", "unavailable"]), source: z.literal("codex-action-server"),
  reason: z.string().max(64).nullable(), detail: z.string().max(300).nullable(),
  events: z.array(z.object({ at: timestamp, kind: z.enum(observedKinds), summary: z.string().max(160), thread_id: threadId })).max(100),
  children: z.array(z.object({ parent_thread: threadId, receiver_thread: threadId })).max(64),
  omitted: z.number().int().nonnegative(), next_cursor: z.string().max(1024).nullable(),
  freshness: z.object({ observed_at: timestamp, age_seconds: z.number().int().nonnegative(), cache: z.enum(["miss", "hit", "revalidated"]), native_updated_at: timestamp.nullable(), thread_status: z.enum(["active", "idle", "not_loaded", "system_error", "unknown"]) }).nullable(),
  binding_verified: z.literal(false), execution_eligible: z.literal(false),
}).refine(value => value.observed === (value.status === "observed") && value.events.every(event => event.thread_id === value.thread_id), "Inconsistent agent timeline");
export type AgentTimeline = z.infer<typeof agentTimelineSchema>;
/** A timeline the UI could not read; rendered as unavailable, never as an empty observation. */
export type AgentTimelineView = AgentTimeline | { status: "error" };
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
const repositoryAlias = z.string().regex(/^[A-Za-z0-9._-]{1,64}$/);
const identity = z.string().regex(/^[a-f0-9]{64}$/);
const forgeName = z.string().min(3).max(201).regex(/^[a-z0-9_.-]+\/[a-z0-9_.-]+$/);
/** `list_factory_github_repositories`: what the GitHub App installation can see. Display only, never proof. */
export const githubRepositoryListSchema = z.object({
  schema_version: z.literal(1), reported_by: z.literal("github"), provenance: z.literal("github_app"),
  configured: z.boolean(), available: z.boolean(), reason: z.string().max(128).nullable(),
  repositories: z.array(z.object({
    full_name: forgeName, private: z.boolean(), default_branch: z.string().max(256).nullable(), description: z.string().max(280).nullable(),
    connected_alias: repositoryAlias.nullable(), connectable: z.boolean(), blocked_reason: z.string().max(128).nullable(),
  })).max(100),
  next_cursor: z.string().min(1).max(512).nullable(), observed_at: timestamp,
});
export type GithubRepositoryList = z.infer<typeof githubRepositoryListSchema>;
export type GithubRepository = GithubRepositoryList["repositories"][number];
/** `connect_factory_github_repository`: a read-only clone registered as an approved alias. Nothing runs. */
export const githubConnectSchema = z.object({
  schema_version: z.literal(1), status: z.enum(["connected", "already_connected"]), alias: repositoryAlias,
  repository: z.object({ alias: repositoryAlias, identity }), full_name: forgeName, default_branch: z.string().max(256).nullable(), provenance: z.literal("github_app"),
}).refine(value => value.repository.alias === value.alias, "Inconsistent connect result");
/** `create_factory_local_repository`: an empty local repository registered as an approved alias. */
export const localCreateSchema = z.object({
  schema_version: z.literal(1), status: z.enum(["created", "already_exists"]), alias: repositoryAlias,
  repository: z.object({ alias: repositoryAlias, identity }),
}).refine(value => value.repository.alias === value.alias, "Inconsistent create result");
export const localRepositoryName = /^[a-z0-9][a-z0-9._-]{0,63}$/;
/** Plain copy for the typed reasons the repository tools return. Unknown codes are never shown raw. */
export function repositoryReasonCopy(reason: string | null | undefined): string {
  const messages: Record<string, string> = {
    github_connect_not_enabled: "Connecting GitHub repos isn't switched on for this Luna yet.",
    local_create_not_enabled: "Creating new repos isn't switched on for this Luna yet.",
    workspace_path_conflict: "A different folder with that name already exists in Luna's workspace.",
    not_allowlisted: "Not on the list of repos Luna may connect.",
    github_app_not_configured: "GitHub isn't set up for this Luna.",
    contents_permission_missing: "The GitHub App can't read this repo's code yet.",
    rate_limited: "GitHub is busy. Try again in a few minutes.",
  };
  if (reason && messages[reason]) return messages[reason];
  return reason && /\s/.test(reason) ? reason.slice(0, 300) : "Luna couldn't do that. Refresh and try again.";
}
/** Importer-supplied display hints: never proof or identity. Invalid hints are dropped, not shown. */
const sourceDisplaySchema = z.object({
  number: z.number().int().min(1).max(Number.MAX_SAFE_INTEGER).optional(),
  url: z.string().max(512).regex(/^https:\/\/github\.com\/[A-Za-z0-9._~%/-]+$/).optional(),
});
const graphSourceSchema = z.object({ provider: nodeId, repository_id: nodeId, item_id: nodeId, revision: nodeId, display: sourceDisplaySchema.optional().catch(undefined) });
export type GraphSource = z.infer<typeof graphSourceSchema>;
const graphNodeSchema = controlSchema.shape.tasks.element.extend({ id: nodeId, dependencies: z.array(nodeId).max(128), source: graphSourceSchema.nullable(), target_preference: nodeId.nullable(), editor_title: z.string().min(1).max(8000).nullable().optional() });
export const graphChangeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("update_task"), node_id: nodeId, title: z.string().min(1).max(8000).nullable(), dependencies: z.array(nodeId).max(128).refine(items => new Set(items).size === items.length, "Dependencies must be unique"), target_id: nodeId.nullable() }),
  z.object({ kind: z.literal("set_target"), node_id: nodeId, target_id: nodeId }),
  z.object({ kind: z.literal("set_dependencies"), node_id: nodeId, dependencies: z.array(nodeId).max(128) }),
  z.object({ kind: z.literal("import_candidates"), nodes: z.array(z.object({ id: nodeId, title: z.string().min(1).max(4000), criterion_ids: z.array(nodeId).min(1).max(32), dependencies: z.array(nodeId).max(128), source: graphSourceSchema })).min(1).max(32) }),
]);
/** Textareas normalize line endings; preserve the server's exact title when no edit was made. */
export function preserveCanonicalTaskTitle(entered: string, canonical: string): string {
  const lines = (value: string) => value.replace(/\r\n?/g, "\n");
  return lines(entered) === lines(canonical) ? canonical : entered;
}
/** Null means keep the server's exact title; empty means an invalid attempt to clear a known title. */
export function taskTitleUpdate(entered: string, canonical: string | null): string | null {
  if (canonical === null) return entered.trim() ? entered : null;
  if (preserveCanonicalTaskTitle(entered, canonical) === canonical) return null;
  return entered.trim() ? entered : "";
}
const proposalSchema = z.object({ id: nodeId, idempotency_key: nodeId, fingerprint: z.string().max(256), actor: z.literal("local_operator"), base_revision: revision, subject: text, change: graphChangeSchema, status: z.enum(["proposed", "applied"]), applied_revision: revision.nullable() });
export const graphEnvelopeSchema = z.object({
  graph: z.object({ run_id: runId, revision, repository: z.object({ alias: z.string().max(64), identity: z.string().max(256), base_head: text, subject: text }), planning_only: z.boolean(), nodes: z.array(graphNodeSchema).max(128), criteria: controlSchema.shape.criteria, attempts: controlSchema.shape.attempts, claim: presentationSchema.shape.claim, changes: z.array(proposalSchema).max(256) }),
  proposal: proposalSchema.nullable(),
});
export type GraphEnvelope = z.infer<typeof graphEnvelopeSchema>;
const count = z.number().int().nonnegative().max(10_000);
/**
 * `list_factory_campaigns`. The parent's forge identity (`provider:owner/repo#number`) is separate
 * from the physical repository (approved alias plus opaque local identity). Parent fields carry
 * explicit provenance: `supplied` is the caller's assertion, never proof and never "reported by GitHub".
 */
const forgeRepository = z.string().min(3).max(201).regex(/^[a-z0-9_-][a-z0-9._-]*\/[a-z0-9_-][a-z0-9._-]*$/);
const campaignSchema = z.object({
  id: runId, repository: z.object({ alias: z.string().min(1).max(64), identity: z.string().regex(/^[a-f0-9]{64}$/) }),
  parent: z.object({
    forge: z.object({ provider: z.enum(["github", "local", "fixture"]), repository: forgeRepository, number: z.number().int().positive().refine(Number.isSafeInteger), node_id: nodeId.nullable() }),
    key: z.string().max(256), revision: nodeId, display: z.object({ url: z.string().max(2048).nullable() }).nullable(),
    provenance: z.enum(["supplied", "github_app"]),
  }),
  title: z.string().min(1).max(1000), finish: finishSchema, status: z.enum(["planned", "promoted", "finished", "stopped"]),
  planning_run_id: runId, run_ids: z.array(runId).max(64), created_at: timestamp, updated_at: timestamp,
  planning: z.object({ run_id: runId, revision, planning_only: z.literal(true), tasks: z.object({ total: count, candidate: count, ready: count, running: count, verify: count, done: count, blocked: count }) }),
  runs: z.array(z.object({ id: runId, state: z.enum(states), updated_at: timestamp })).max(64),
  // A campaign projection never authorizes execution; anything else is rejected, not shown.
  promotion: z.object({ allowed: z.literal(false), reason: z.string().max(160) }),
}).superRefine((campaign, context) => {
  const { tasks } = campaign.planning;
  const { forge } = campaign.parent;
  const linked = new Set(campaign.run_ids);
  if (campaign.planning.run_id !== campaign.planning_run_id || linked.has(campaign.planning_run_id) || linked.size !== campaign.run_ids.length
    || !campaign.runs.every(run => linked.has(run.id)) || tasks.total !== tasks.candidate + tasks.ready + tasks.running + tasks.verify + tasks.done + tasks.blocked
    || campaign.parent.key !== `${forge.provider}:${forge.repository}#${forge.number}`) {
    context.addIssue({ code: "custom", message: "Inconsistent campaign projection" });
  }
});
/** Unreadable records the server skipped instead of failing the list. Counted, never shown as campaigns. */
const skippedCampaignSchema = z.object({ id: z.string().max(64).nullable(), reason: z.string().max(64) });
export const campaignListSchema = z.object({ campaigns: z.array(campaignSchema).max(100), skipped: z.array(skippedCampaignSchema).max(100).default([]) });
export type CampaignView = z.infer<typeof campaignSchema>;
const providerLabels: Record<string, string> = { github: "GitHub", local: "Local", fixture: "Fixture" };
export function providerLabel(provider: string): string { return providerLabels[provider] ?? provider; }
const provenanceLabels: Record<CampaignView["parent"]["provenance"], string> = { supplied: "Linked by you", github_app: "Retrieved by Luna" };
/** Friendly issue reference for the main view; provenance stays in the task inspector. */
export function parentLabel(campaign: Pick<CampaignView, "parent">): string {
  const { forge } = campaign.parent;
  return `${providerLabel(forge.provider)} ${forge.repository}#${forge.number}`;
}
export function parentProvenanceLabel(campaign: Pick<CampaignView, "parent">): string {
  return provenanceLabels[campaign.parent.provenance];
}
export function campaignStatusLabel(status: CampaignView["status"]): string {
  return { planned: "Planned · not started", promoted: "Promoted", finished: "Finished", stopped: "Stopped" }[status];
}
export type FactoryGraph = GraphEnvelope["graph"];
export type GraphChange = z.infer<typeof graphChangeSchema>;
const githubCount = z.number().int().nonnegative().max(10_000_000);
const shortCode = z.string().max(32).nullable();
const githubText = (max: number) => z.string().max(max);
const deliveryPullSchema = z.object({
  number: z.number().int().positive().max(1_000_000_000), url: githubText(512).refine(value => value.startsWith("https://"), "Expected an HTTPS URL"),
  state: shortCode, draft: z.boolean(), head_sha: z.string().regex(/^[a-fA-F0-9]{40}$/).nullable(), review_decision: shortCode,
  diff: z.object({ files_changed: githubCount.nullable(), additions: githubCount.nullable(), deletions: githubCount.nullable() }),
  checks: z.object({
    rollup: shortCode, total: githubCount, truncated: z.boolean(),
    runs: z.array(z.object({ name: githubText(200), status: shortCode, conclusion: shortCode, started_at: timestamp.nullable(), completed_at: timestamp.nullable(), summary: githubText(280).nullable() })).max(25),
    statuses: z.array(z.object({ context: githubText(200), state: shortCode, created_at: timestamp.nullable() })).max(25),
  }),
  ready_for_review: z.boolean(), merge_authority: z.literal(false), observed_at: timestamp, reported_by: z.literal("github"),
  provenance: z.literal("github_app").optional(),
});
const deliveryNodeSchema = z.object({
  node_id: nodeId, item_id: nodeId, reported_by: z.literal("github"), provenance: z.literal("github_app").optional(),
  /** False when GitHub did not return every linked pull request or check context. Optional for older servers. */
  complete: z.boolean().optional(),
  status: z.enum(["observed", "unavailable", "deferred"]), freshness: z.enum(["fresh", "cached", "stale"]).nullable(),
  reason: z.string().max(128).nullable(), observed_at: timestamp.nullable(),
  issue: z.object({ number: z.number().int().positive(), url: githubText(512), state: shortCode }).nullable(),
  pull_requests: z.array(deliveryPullSchema).max(5), pull_requests_total: githubCount, pull_requests_truncated: z.boolean(),
});
/** `read_factory_delivery`: GitHub-reported display data. Never proof, attention or an action. */
export const deliverySchema = z.object({
  schema_version: z.literal(1), run_id: runId, graph_revision: revision, reported_by: z.literal("github"),
  proof: z.literal("none"), merge_capability: z.literal("none"), available: z.boolean(),
  provenance: z.literal("github_app").optional(), complete: z.boolean().optional(),
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
export const finishLabels = { local_candidate: "Changes ready locally", push: "Push a branch", pr: "Open a pull request" } satisfies Record<RunView["finish"], string>;
/** Plan form wording for the same finish options. */
export const planFinishLabels = { local_candidate: "Keep changes local", push: "Push a branch", pr: "Open a pull request" } satisfies Record<RunView["finish"], string>;
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
  repository: z.string().min(1, "Pick a repository").max(64), objective: z.string().trim().min(1, "Say what you want done").max(8000),
  acceptance: z.array(z.string().min(1).max(1000)).min(1, "Add at least one acceptance criterion").max(32),
  non_goals: z.array(z.string().min(1).max(1000)).max(32), finish: finishSchema, profile: z.string().min(1).max(64),
  capacity: z.coerce.number().int().min(1).max(8), repair_attempts: z.coerce.number().int().min(0).max(10),
  wall_seconds: z.coerce.number().int().min(30).max(86400), idempotency_key: z.string().min(1).max(128),
});
/** What "Plan it" sends when More options is left alone. Always inside the server's limits. */
export function planDefaults(capabilities: Capabilities) {
  return {
    finish: "local_candidate" as const, profile: capabilities.profiles[0]?.alias ?? "", capacity: 1,
    wall_seconds: Math.min(1800, capabilities.limits.wall_seconds), repair_attempts: Math.min(2, capabilities.limits.repair_attempts),
  };
}
const filled = (value: string | undefined): string | undefined => value?.trim() ? value.trim() : undefined;
/**
 * The plan form's request. "Done when" is one optional line; when empty the request itself is the
 * single criterion. Anything left out of More options takes `planDefaults`.
 */
export function startRequest(fields: Record<string, string>, capabilities: Capabilities, key: string, settings: Settings = {}) {
  const defaults = { ...planDefaults(capabilities), ...settings };
  const objective = fields.objective?.trim() ?? "";
  const doneWhen = filled(fields.done_when);
  if (!doneWhen && objective.length > 1000) throw new Error("Keep your request under 1,000 characters.");
  const minutes = filled(fields.wall_minutes);
  const result = startSchema.safeParse({
    repository: fields.repository ?? "", objective,
    acceptance: doneWhen ? [doneWhen] : objective ? [objective] : [],
    non_goals: lines(fields.non_goals ?? "", "non_goals"),
    finish: filled(fields.finish) ?? defaults.finish, profile: filled(fields.profile) ?? defaults.profile,
    capacity: filled(fields.capacity) ?? defaults.capacity, repair_attempts: filled(fields.repair_attempts) ?? defaults.repair_attempts,
    wall_seconds: minutes === undefined ? defaults.wall_seconds : Math.min(Number(minutes) * 60, capabilities.limits.wall_seconds),
    idempotency_key: key,
  });
  if (!result.success) throw new Error(result.error.issues[0]?.message || "Check the plan fields");
  const request = result.data;
  const repository = capabilities.repositories.find(repo => repo.alias === request.repository);
  if (!repository) throw new Error("Pick one of the ready repositories");
  if (!allowedFinishes(repository.max_finish).includes(request.finish)) throw new Error("This repository can't finish that way. Pick another option under More options.");
  if (!capabilities.profiles.some(profile => profile.alias === request.profile)) throw new Error("Pick one of the offered models");
  const limits = capabilities.limits;
  if (request.capacity > limits.capacity || request.repair_attempts > limits.repair_attempts || request.wall_seconds > limits.wall_seconds) throw new Error("That's more than this Luna allows. Lower the numbers under More options.");
  return request;
}
