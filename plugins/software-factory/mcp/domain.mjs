export const OUTCOME_CLASSIFICATIONS = ["ALREADY_SATISFIED", "REUSE_EXISTING", "REFINE_EXISTING", "CREATE_NEW", "INVESTIGATE_UNKNOWN", "OUT_OF_SCOPE"];
export const STATUSES = ["READY", "WAITING", "BLOCKED", "UNKNOWN", "SATISFIED"];
export const CONTROL_ACTIONS = ["PLAN", "APPROVE_PLAN", "MATERIALIZE", "START", "RESUME", "RECONCILE", "STATUS", "DRAIN", "CHECKPOINT", "QUIESCE"];
export const DEFAULT_PRESET = { executor: "Codex CLI", model: "gpt-5.6-luna", effort: "xhigh" };

const words = (value) => String(value ?? "").toLowerCase().match(/[a-z0-9]+/g) ?? [];
const overlap = (a, b) => {
  const left = new Set(words(a));
  const right = new Set(words(b));
  return [...left].filter((word) => word.length > 2 && right.has(word)).length;
};

export function classifyOutcome(intent, existing = []) {
  if (!intent?.goal?.trim()) return { classification: "INVESTIGATE_UNKNOWN", reason: "No durable goal was supplied." };
  const match = existing.find((item) => overlap(intent.goal, `${item.title} ${item.body}`) >= 2);
  if (match?.status === "CLOSED" || match?.status === "MERGED") return { classification: "ALREADY_SATISFIED", reference: match.number, reason: "Existing work is terminal." };
  if (match) return { classification: "REUSE_EXISTING", reference: match.number, reason: "Existing work overlaps the supplied goal." };
  if (intent.architectureUnknown) return { classification: "INVESTIGATE_UNKNOWN", reason: "Architecture evidence is missing; semantic planning is required." };
  return { classification: "CREATE_NEW", reason: "No equivalent observed work was supplied." };
}

export function diffPlanGenerations(previous, next) {
  const prior = new Map((previous?.outcomes ?? []).map((item) => [item.id, item]));
  const current = new Map((next?.outcomes ?? []).map((item) => [item.id, item]));
  const added = [...current.keys()].filter((id) => !prior.has(id));
  const removed = [...prior.keys()].filter((id) => !current.has(id));
  const changed = [...current.keys()].filter((id) => prior.has(id) && JSON.stringify(prior.get(id)) !== JSON.stringify(current.get(id)));
  const approvalInvalidated = previous?.observedRepositoryHead !== next?.observedRepositoryHead;
  return { from: previous?.generation ?? null, to: next?.generation ?? null, added, removed, changed, approvalInvalidated, reason: approvalInvalidated ? "Observed repository head changed; prior approval is stale." : "Plan generation changed without a head change." };
}

export function normalizeControlIntent(input = {}) {
  if (!CONTROL_ACTIONS.includes(input.action)) throw new Error(`Unsupported control action: ${input.action}`);
  if (!input.target?.repository || !input.target?.targetBranch || !input.target?.observedHead) throw new Error("Control intent requires repository, targetBranch, and observedHead.");
  return { action: input.action, target: input.target, planGeneration: input.planGeneration ?? null, orchestrationPreset: { ...DEFAULT_PRESET, ...(input.orchestrationPreset ?? {}) }, constraints: input.constraints ?? {} };
}

export function countStatuses(items = []) {
  return Object.fromEntries(STATUSES.map((status) => [status, items.filter((item) => item.status === status).length]));
}

export function makePrompt(intent, snapshot) {
  const control = normalizeControlIntent(intent);
  return [`DISPATCH TO AGENT`, `Action: ${control.action}`, `Repository: ${control.target.repository}`, `Branch: ${control.target.targetBranch}`, `Observed head: ${control.target.observedHead}`, `Plan generation: ${control.planGeneration ?? "UNKNOWN"}`, `Preset: ${control.orchestrationPreset.executor} / ${control.orchestrationPreset.model} / ${control.orchestrationPreset.effort}`, `Evidence status: ${snapshot?.status ?? "UNKNOWN"}`, `This is a generated prompt preview; it has not been executed.`].join("\n");
}
