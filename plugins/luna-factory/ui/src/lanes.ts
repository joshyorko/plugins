// Flight Recorder lanes. Only server-recorded timestamps are placed on the time axis.
import type { ViewState } from "./controller";
import type { RunView } from "./domain";
import { livenessLabel, type Agent } from "./agents";
import { coordinatorToken, workerToken } from "./lunar";
import { activityAt, clock } from "./narrative";

const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const kindLabel = (kind: string) => kind.replaceAll("_", " ");
type Receipt = RunView["receipts"][number];

/**
 * The listed agent whose thread the server recorded on this receipt, if any. A receipt without a
 * thread, or naming a thread outside the current roster, stays unattributed. Nothing is inferred.
 */
export function receiptAgent(receipt: Receipt, agents: Agent[]): Agent | undefined {
  return receipt.thread_id ? agents.find(agent => agent.thread === receipt.thread_id) : undefined;
}
/** True when this server reports receipt attribution at all (older servers omit the field). */
export function reportsAttribution(run: RunView): boolean {
  return run.receipts.some(receipt => receipt.thread_id !== undefined);
}

export function renderLanes(run: RunView, agents: Agent[], state: ViewState, now = Date.now() / 1000): string {
  const receipts = [...run.receipts].sort((a, b) => a.created_at - b.created_at);
  if (!receipts.length && !agents.length) {
    return `<section class="lanes empty" aria-label="Agent lanes"><h3>No activity recorded yet</h3><p>${run.planning_only ? "Execution hasn't started on this plan, so there is nothing to replay." : "This run has no retained receipts and no reported agents."}</p></section>`;
  }
  const start = receipts[0]?.created_at ?? activityAt(run) ?? now;
  const latest = Math.max(activityAt(run) ?? 0, receipts.at(-1)?.created_at ?? 0);
  // Only extend the axis to "now" while the record is recent; an old record ends at its last event.
  const live = now - latest < 6 * 3600;
  const end = live ? Math.max(now, latest) : latest;
  const span = Math.max(60, end - start);
  const at = (seconds: number) => `${(((seconds - start) / span) * 100).toFixed(2)}%`;
  const tick = (receipt: Receipt) => `<span class="tick kind-${esc(receipt.kind)}" style="--at:${at(receipt.created_at)}" title="${esc(`${clock(receipt.created_at)} · ${kindLabel(receipt.kind)}`)}"><span class="sr-only">${esc(clock(receipt.created_at))} ${esc(kindLabel(receipt.kind))}</span></span>`;
  const owner = new Map<string, Receipt[]>();
  const unattributed: Receipt[] = [];
  for (const receipt of receipts) {
    const agent = receiptAgent(receipt, agents);
    if (agent) owner.set(agent.thread, [...(owner.get(agent.thread) ?? []), receipt]);
    else unattributed.push(receipt);
  }
  const attribution = reportsAttribution(run);
  const ticks = unattributed.map(tick).join("");
  const nowLine = live ? '<span class="now-line" style="--at:100%"></span>' : "";
  const agentRows = agents.map(agent => {
    const selected = state.selectedAgent === agent.thread;
    const own = owner.get(agent.thread) ?? [];
    // Agents with no attributed receipts keep the hatched "not reported" track.
    const track = own.length
      ? `<div class="lane-track">${own.map(tick).join("")}${nowLine}</div>`
      : `<div class="lane-track unavailable"><span>${attribution ? "No receipts attributed to this agent" : "Per-agent timeline not reported by this server"}</span></div>`;
    return `<li class="lane${selected ? " selected" : ""}"><button type="button" class="lane-label" id="lane-${esc(agent.ordinal)}" data-action="select-agent" data-thread-id="${esc(agent.thread)}" aria-pressed="${selected}">${agent.role === "coordinator" ? coordinatorToken(agent.liveness, 22) : workerToken(agent.ordinal, agent.liveness, 22)}<span><strong>${esc(agent.label)}</strong><small>${esc(livenessLabel[agent.liveness])}${own.length ? ` · ${own.length} ${own.length === 1 ? "receipt" : "receipts"}` : ""}</small></span></button>${track}</li>`;
  }).join("");
  return `<section class="lanes" aria-label="Agent lanes">
    <div class="lanes-axis" aria-hidden="true"><span>${esc(clock(start, now))}</span><span>${live ? "now " : "last event "}${esc(clock(end, now))}</span></div>
    <ol class="lane-list">
      <li class="lane run-lane"><div class="lane-label static"><span class="run-dot" aria-hidden="true"></span><span><strong>Run receipts</strong><small>No listed agent</small></span></div><div class="lane-track">${ticks || `<span class="track-note">${receipts.length ? "Every retained receipt is on an agent lane" : "No receipts retained"}</span>`}${nowLine}</div></li>
      ${agentRows}
    </ol>
    ${receipts.length ? `<ol class="lane-events" aria-label="Recorded events, newest first">${[...receipts].reverse().map(receipt => { const agent = receiptAgent(receipt, agents); return `<li><time datetime="${new Date(receipt.created_at * 1000).toISOString()}">${esc(clock(receipt.created_at, now))}</time><strong>${esc(kindLabel(receipt.kind))}${agent ? ` <small class="lane-agent">${esc(agent.label)}</small>` : ""}</strong><span>${esc(receipt.summary)}</span></li>`; }).join("")}</ol>` : ""}
    <p class="lanes-note small muted">Showing the latest ${receipts.length} retained receipts. ${attribution ? "Agent lanes show receipts the server recorded on that agent's thread; the rest stay on Run receipts." : "Agent lanes fill in when the server reports receipt attribution."}</p>
  </section>`;
}
