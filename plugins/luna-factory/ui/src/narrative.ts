// Plain-language status built only from fields the server returned. Nothing here is inferred.
import { needsOperatorDecision, type RunView } from "./domain";

export type Tier = "needs" | "working" | "planned" | "finished" | "stopped" | "unverified";

/** One attention tier per run. "needs" is exactly the server-authorized decision rule. */
export function runTier(run: RunView): Tier {
  if (needsOperatorDecision(run)) return "needs";
  if (run.planning_only) return "planned";
  if (run.state === "BLOCKED") return "stopped";
  if (!run.control || !run.presentation) return "unverified";
  const kind = run.presentation.result.kind;
  return kind === "working" ? "working" : kind === "finished_verified" ? "finished" : kind === "stopped_unresolved" ? "stopped" : "unverified";
}
/** Server labels are authoritative; only the two UI-owned tiers get local copy. */
export function statusLabel(run: RunView): string {
  const tier = runTier(run);
  if (tier === "needs") return "Needs you";
  if (tier === "planned") return "Planned";
  if (tier === "stopped") return run.state === "FAILED" ? "Failed" : run.state === "BLOCKED" ? "Blocked" : "Stopped";
  if (!run.control || !run.presentation) return "Status unknown";
  switch (run.presentation.result.kind) {
    case "working": return "Running";
    case "finished_verified": return "Done";
    case "stopped_unresolved": return "Blocked";
    case "needs_input": return "Needs you";
    case "unverified": return "Status unknown";
  }
}

/** The one sentence that answers "what is happening now". */
export function nowSentence(run: RunView): string {
  const tier = runTier(run);
  if (tier === "needs") return run.pending_decision?.question || "An approval is waiting in Codex.";
  if (tier === "planned") {
    const tasks = run.control?.tasks.length ?? 0;
    return `${tasks} ${tasks === 1 ? "task" : "tasks"} in this plan.`;
  }
  if (tier === "finished") return run.delta || "Every mandatory criterion is proven.";
  return run.blocker || run.delta || run.remaining_gap || "No change has been reported yet.";
}

/**
 * Plain-language copy for the server's typed blocker category. Copy and placement only: tiers,
 * "Needs you" and actions never read it. `none` and `planning_only` add nothing.
 */
export function blockerCopy(run: RunView): string | null {
  const kind = run.control ? run.presentation?.blocker_kind : undefined;
  if (!kind) return null;
  switch (kind.kind) {
    case "budget_exhausted": return kind.budget === "time" ? "Time limit reached" : "Retry limit reached";
    case "diagnosis_required": return "A diagnosis is needed before another repair";
    case "native_approval": return "Waiting for approval in Codex";
    case "effect_outcome_unknown": return "Luna can't confirm whether the change happened";
    case "liveness_unknown": return "Luna can't confirm the earlier run stopped.";
    case "unknown": return "Luna can't check why this work is blocked";
    case "none": case "planning_only": return null;
  }
}

/** Newest server-recorded activity: `activity_at` when reported, else lifecycle `updated_at`. */
export function activityAt(run: RunView): number | undefined {
  return run.activity_at ?? run.updated_at;
}

/** Time of day for today; otherwise the date too, so old events never read as recent. */
export function clock(seconds: number, now = Date.now() / 1000): string {
  const date = new Date(seconds * 1000);
  const sameDay = date.toDateString() === new Date(now * 1000).toDateString();
  return sameDay ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : date.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
export function freshness(seconds: number | undefined, now = Date.now() / 1000): string {
  if (seconds === undefined) return "Time unknown";
  const age = Math.max(0, Math.round(now - seconds));
  if (age < 60) return "now";
  if (age < 3600) return `${Math.round(age / 60)}m`;
  if (age < 86_400) return `${Math.round(age / 3600)}h`;
  return new Date(seconds * 1000).toLocaleDateString([], { month: "short", day: "numeric" });
}

/** `updated` is the run's newest reported activity when first seen (see `activityAt`). */
export interface Baseline { at: number; updated: number }
/**
 * "Since you last looked", scoped honestly to this open session.
 * The first snapshot of each run is the baseline; later snapshots report only server-recorded changes.
 */
export class SinceTracker {
  private readonly seen = new Map<string, Baseline>();
  /**
   * `restored` carries baselines from an earlier session when the host persists widget state;
   * `persist` is told when new runs are first seen. Both are optional and feature-detected by the caller.
   */
  constructor(private readonly now: () => number = () => Date.now() / 1000, restored: Record<string, Baseline> = {}, private readonly persist?: (baselines: Record<string, Baseline>) => void) {
    for (const [id, base] of Object.entries(restored).slice(0, 200)) if (Number.isFinite(base?.at) && Number.isFinite(base?.updated)) this.seen.set(id, { at: base.at, updated: base.updated });
  }
  observe(runs: RunView[]): void {
    let added = false;
    for (const run of runs) {
      if (!this.seen.has(run.id)) { this.seen.set(run.id, { at: this.now(), updated: activityAt(run) ?? 0 }); added = true; }
    }
    if (added) this.persist?.(Object.fromEntries(this.seen));
  }
  /** Moves every baseline to now, so the next visit reports only what changed after this one. */
  markSeen(runs: RunView[]): void {
    const at = this.now();
    for (const run of runs) this.seen.set(run.id, { at, updated: activityAt(run) ?? 0 });
    this.persist?.(Object.fromEntries(this.seen));
  }
  /** Returns null when nothing changed after the baseline, so no change is implied. */
  changes(run: RunView): { since: number; lines: string[] } | null {
    const base = this.seen.get(run.id);
    if (!base) return null;
    // Receipts can be newer than updated_at, so only events after this session began count as new.
    const threshold = Math.max(base.updated, base.at);
    const receipts = run.receipts.filter(receipt => receipt.created_at > threshold).sort((a, b) => b.created_at - a.created_at).slice(0, 3).map(receipt => `${clock(receipt.created_at)} · ${receipt.summary}`);
    const updated = (activityAt(run) ?? 0) > base.updated;
    if (!receipts.length && !updated) return null;
    return { since: base.at, lines: receipts.length ? receipts : run.delta ? [run.delta] : [] };
  }
  baseline(runId: string): number | null { return this.seen.get(runId)?.at ?? null; }
}
