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

export function clock(seconds: number): string {
  return new Date(seconds * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
export function freshness(seconds: number | undefined, now = Date.now() / 1000): string {
  if (seconds === undefined) return "Update time not reported";
  const age = Math.max(0, Math.round(now - seconds));
  if (age < 60) return "Updated just now";
  if (age < 3600) return `Updated ${Math.round(age / 60)} min ago`;
  return `Updated ${clock(seconds)}`;
}

interface Baseline { at: number; updated: number }
/**
 * "Since you last looked", scoped honestly to this open session.
 * The first snapshot of each run is the baseline; later snapshots report only server-recorded changes.
 */
export class SinceTracker {
  private readonly seen = new Map<string, Baseline>();
  constructor(private readonly now: () => number = () => Date.now() / 1000) {}
  observe(runs: RunView[]): void {
    for (const run of runs) {
      if (!this.seen.has(run.id)) this.seen.set(run.id, { at: this.now(), updated: run.updated_at ?? 0 });
    }
  }
  /** Returns null when nothing changed after the baseline, so no change is implied. */
  changes(run: RunView): { since: number; lines: string[] } | null {
    const base = this.seen.get(run.id);
    if (!base) return null;
    // Server time when first seen; receipts recorded after it are new to this session.
    const threshold = base.updated || base.at;
    const receipts = run.receipts.filter(receipt => receipt.created_at > threshold).sort((a, b) => b.created_at - a.created_at).slice(0, 3).map(receipt => `${clock(receipt.created_at)} · ${receipt.summary}`);
    const updated = (run.updated_at ?? 0) > base.updated;
    if (!receipts.length && !updated) return null;
    return { since: base.at, lines: receipts.length ? receipts : run.delta ? [run.delta] : [] };
  }
  baseline(runId: string): number | null { return this.seen.get(runId)?.at ?? null; }
}
