// Plain-language status built only from fields the server returned. Nothing here is inferred.
import { needsOperatorDecision, type RunView } from "./domain";

export type Tier = "needs" | "working" | "planned" | "finished" | "stopped" | "unverified";

/** One attention tier per run. "needs" is exactly the server-authorized decision rule. */
export function runTier(run: RunView): Tier {
  if (needsOperatorDecision(run)) return "needs";
  if (run.planning_only) return "planned";
  if (!run.control || !run.presentation) return "unverified";
  const kind = run.presentation.result.kind;
  return kind === "working" ? "working" : kind === "finished_verified" ? "finished" : kind === "stopped_unresolved" ? "stopped" : "unverified";
}
/** Server labels are authoritative; only the two UI-owned tiers get local copy. */
export function statusLabel(run: RunView): string {
  const tier = runTier(run);
  if (tier === "needs") return "Needs you";
  if (tier === "planned") return "Planned · not started";
  return run.control && run.presentation ? run.presentation.result.label : "Outcome unverified";
}

/** The one sentence that answers "what is happening now". */
export function nowSentence(run: RunView): string {
  const tier = runTier(run);
  if (tier === "needs") return run.pending_decision?.question || "A native approval is waiting in Codex.";
  if (tier === "planned") {
    const tasks = run.control?.tasks.length ?? 0;
    return `Execution hasn't started. ${tasks === 1 ? "1 planned task" : `${tasks} planned tasks`}${run.remaining_gap ? ` · ${run.remaining_gap}` : ""}.`;
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
    case "budget_exhausted": return kind.budget === "time" ? "Time budget used up" : "Repair budget used up";
    case "diagnosis_required": return "A diagnosis is needed before another repair";
    case "native_approval": return "Waiting on an approval in native Codex";
    case "effect_outcome_unknown": return "An effect's outcome is unknown";
    case "liveness_unknown": return "Owned execution isn't proved stopped yet";
    case "unknown": return "Blocker not categorized by the server";
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
  if (seconds === undefined) return "Update time not reported";
  const age = Math.max(0, Math.round(now - seconds));
  if (age < 60) return "Updated just now";
  if (age < 3600) return `Updated ${Math.round(age / 60)} min ago`;
  return `Updated ${clock(seconds, now)}`;
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
