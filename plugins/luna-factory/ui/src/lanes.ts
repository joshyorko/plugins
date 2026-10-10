// Flight Recorder lanes. Only server-recorded timestamps are placed on the time axis.
import type { ViewState } from "./controller";
import type { RunView } from "./domain";
import { livenessLabel, type Agent } from "./agents";
import { coordinatorToken, workerToken } from "./lunar";
import { clock } from "./narrative";

const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const kindLabel = (kind: string) => kind.replaceAll("_", " ");

export function renderLanes(run: RunView, agents: Agent[], state: ViewState, now = Date.now() / 1000): string {
  const receipts = [...run.receipts].sort((a, b) => a.created_at - b.created_at);
  if (!receipts.length && !agents.length) {
    return `<section class="lanes empty" aria-label="Agent lanes"><h3>No activity recorded yet</h3><p>${run.planning_only ? "Execution hasn't started on this plan, so there is nothing to replay." : "This run has no retained receipts and no reported agents."}</p></section>`;
  }
  const start = receipts[0]?.created_at ?? run.updated_at ?? now;
  const end = Math.max(now, run.updated_at ?? 0, receipts.at(-1)?.created_at ?? 0);
  const span = Math.max(60, end - start);
  const at = (seconds: number) => `${(((seconds - start) / span) * 100).toFixed(2)}%`;
  const ticks = receipts.map(receipt => `<span class="tick kind-${esc(receipt.kind)}" style="--at:${at(receipt.created_at)}" title="${esc(`${clock(receipt.created_at)} · ${kindLabel(receipt.kind)}`)}"><span class="sr-only">${esc(clock(receipt.created_at))} ${esc(kindLabel(receipt.kind))}</span></span>`).join("");
  const agentRows = agents.map(agent => {
    const selected = state.selectedAgent === agent.thread;
    return `<li class="lane${selected ? " selected" : ""}"><button type="button" class="lane-label" id="lane-${esc(agent.ordinal)}" data-action="select-agent" data-thread-id="${esc(agent.thread)}" aria-pressed="${selected}">${agent.role === "coordinator" ? coordinatorToken(agent.liveness, 22) : workerToken(agent.ordinal, agent.liveness, 22)}<span><strong>${esc(agent.label)}</strong><small>${esc(livenessLabel[agent.liveness])}</small></span></button><div class="lane-track unavailable"><span>Per-agent timeline not reported by this server</span></div></li>`;
  }).join("");
  return `<section class="lanes" aria-label="Agent lanes">
    <div class="lanes-axis" aria-hidden="true"><span>${esc(clock(start))}</span><span>now ${esc(clock(end))}</span></div>
    <ol class="lane-list">
      <li class="lane run-lane"><div class="lane-label static"><span class="run-dot" aria-hidden="true"></span><span><strong>Run receipts</strong><small>Not attributed to an agent</small></span></div><div class="lane-track">${ticks || '<span class="track-note">No receipts retained</span>'}<span class="now-line" style="--at:100%"></span></div></li>
      ${agentRows}
    </ol>
    ${receipts.length ? `<ol class="lane-events" aria-label="Recorded events, newest first">${[...receipts].reverse().map(receipt => `<li><time datetime="${new Date(receipt.created_at * 1000).toISOString()}">${esc(clock(receipt.created_at))}</time><strong>${esc(kindLabel(receipt.kind))}</strong><span>${esc(receipt.summary)}</span></li>`).join("")}</ol>` : ""}
    <p class="lanes-note small muted">Showing the latest ${receipts.length} retained receipts. Agent lanes fill in when the server exposes per-agent activity.</p>
  </section>`;
}
