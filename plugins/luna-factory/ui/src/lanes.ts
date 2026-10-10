// Flight Recorder lanes. Only server-recorded or server-reported timestamps are placed on the time axis.
// Agent ticks are "Observed in Codex": display data only, never proof, attention or plan state.
import type { ViewState } from "./controller";
import type { AgentTimeline, RunView } from "./domain";
import { livenessLabel, type Agent } from "./agents";
import { coordinatorToken, workerToken } from "./lunar";
import { activityAt, clock } from "./narrative";

const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const kindLabel = (kind: string) => kind.replaceAll("_", " ");
type Receipt = RunView["receipts"][number];
const observedLabel: Record<AgentTimeline["events"][number]["kind"], string> = {
  assigned: "Assigned", command: "Command", file_change: "File change", commit: "Commit", pr_opened: "PR opened",
  check_result: "Check result", subagent_spawned: "Helper spawned", message: "Message", waiting: "Waiting", error: "Error",
};
export const OBSERVED = "Observed in Codex";

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

/** The observed timeline for an agent, or the reason the server gave for not having one. */
function observation(run: RunView, agent: Agent, state: ViewState): { timeline: AgentTimeline } | { reason: string } {
  const capability = state.capabilities?.agent_timeline;
  if (!capability) return { reason: reportsAttribution(run) ? "No receipts attributed to this agent" : "Per-agent timeline not reported by this server" };
  if (!capability.enabled) return { reason: capability.detail };
  const view = state.timelineRunId === run.id ? state.timelines[agent.thread] : undefined;
  if (!view) return { reason: "Reading what Codex observed…" };
  if (view.status === "error") return { reason: "Codex observation could not be read for this agent." };
  if (view.status === "unavailable") return { reason: view.detail ?? "Codex observation is unavailable for this agent." };
  return { timeline: view };
}

export function renderLanes(run: RunView, agents: Agent[], state: ViewState, now = Date.now() / 1000): string {
  const receipts = [...run.receipts].sort((a, b) => a.created_at - b.created_at);
  if (!receipts.length && !agents.length) {
    return `<section class="lanes empty" aria-label="Agent lanes"><h3>No activity recorded yet</h3><p>${run.planning_only ? "Execution hasn't started on this plan, so there is nothing to replay." : "This run has no retained receipts and no reported agents."}</p></section>`;
  }
  const observations = agents.map(agent => ({ agent, observed: observation(run, agent, state) }));
  const observedTimes = observations.flatMap(({ observed }) => "timeline" in observed ? observed.timeline.events.map(event => event.at) : []);
  const times = [...receipts.map(receipt => receipt.created_at), ...observedTimes];
  const start = times.length ? Math.min(...times) : activityAt(run) ?? now;
  const latest = Math.max(activityAt(run) ?? 0, ...times);
  // Only extend the axis to "now" while the record is recent; an old record ends at its last event.
  const live = now - latest < 6 * 3600;
  const end = live ? Math.max(now, latest) : latest;
  const span = Math.max(60, end - start);
  const at = (seconds: number) => `${(((seconds - start) / span) * 100).toFixed(2)}%`;
  const tick = (receipt: Receipt) => `<span class="tick kind-${esc(receipt.kind)}" style="--at:${at(receipt.created_at)}" title="${esc(`${clock(receipt.created_at)} · ${kindLabel(receipt.kind)}`)}"><span class="sr-only">${esc(clock(receipt.created_at))} ${esc(kindLabel(receipt.kind))}</span></span>`;
  const observedTick = (event: AgentTimeline["events"][number]) => `<span class="tick observed obs-${esc(event.kind)}" style="--at:${at(event.at)}" title="${esc(`${clock(event.at)} · ${observedLabel[event.kind]} · ${event.summary} · ${OBSERVED}`)}"><span class="sr-only">${esc(clock(event.at))} ${esc(observedLabel[event.kind])}: ${esc(event.summary)}</span></span>`;
  const owner = new Map<string, Receipt[]>();
  const unattributed: Receipt[] = [];
  for (const receipt of receipts) {
    const agent = receiptAgent(receipt, agents);
    if (agent) owner.set(agent.thread, [...(owner.get(agent.thread) ?? []), receipt]);
    else unattributed.push(receipt);
  }
  const attribution = reportsAttribution(run);
  const ticks = unattributed.map(tick).join("");
  // Only a server that reports the timeline capability has an observation reason worth showing beside receipts.
  const observable = state.capabilities?.agent_timeline !== undefined;
  const nowLine = live ? '<span class="now-line" style="--at:100%"></span>' : "";
  const agentRows = observations.map(({ agent, observed }) => {
    const selected = state.selectedAgent === agent.thread;
    const own = owner.get(agent.thread) ?? [];
    // Factory receipts recorded on this thread and ticks observed in Codex share the lane; neither infers the other.
    const track = "timeline" in observed
      ? `<div class="lane-track observed" aria-label="${esc(`${OBSERVED}: ${observed.timeline.events.length} events for ${agent.label}`)}">${own.map(tick).join("")}${observed.timeline.events.map(observedTick).join("") || (own.length ? "" : '<span class="track-note">No timed activity on the latest page</span>')}${nowLine}</div>`
      : own.length
        ? `<div class="lane-track"${observable ? ` title="${esc(observed.reason)}"` : ""}>${own.map(tick).join("")}${nowLine}${observable ? `<span class="sr-only">${esc(observed.reason)}</span>` : ""}</div>`
        : `<div class="lane-track unavailable"><span>${esc(observed.reason)}</span></div>`;
    return `<li class="lane${selected ? " selected" : ""}"><button type="button" class="lane-label" id="lane-${esc(agent.ordinal)}" data-action="select-agent" data-thread-id="${esc(agent.thread)}" aria-pressed="${selected}">${agent.role === "coordinator" ? coordinatorToken(agent.liveness, 22) : workerToken(agent.ordinal, agent.liveness, 22)}<span><strong>${esc(agent.label)}</strong><small>${esc(livenessLabel[agent.liveness])}${own.length ? ` · ${own.length} ${own.length === 1 ? "receipt" : "receipts"}` : ""}</small></span></button>${track}</li>`;
  }).join("");
  const chosen = observations.find(({ agent }) => agent.thread === state.selectedAgent);
  const observing = state.capabilities?.agent_timeline?.enabled === true;
  const notes = [
    attribution ? "Agent lanes show receipts the server recorded on that agent's thread; the rest stay on Run receipts." : "",
    observing ? "Agent ticks observed in Codex are not Factory proof and never change attention or the map." : "",
  ].filter(Boolean);
  return `<section class="lanes" aria-label="Agent lanes">
    ${observing ? `<p class="lanes-legend small muted" aria-hidden="true"><span class="legend-receipt"></span>Factory receipt<span class="legend-observed"></span>${OBSERVED}</p>` : ""}
    <div class="lanes-axis" aria-hidden="true"><span>${esc(clock(start, now))}</span><span>${live ? "now " : "last event "}${esc(clock(end, now))}</span></div>
    <ol class="lane-list">
      <li class="lane run-lane"><div class="lane-label static"><span class="run-dot" aria-hidden="true"></span><span><strong>Run receipts</strong><small>No listed agent</small></span></div><div class="lane-track">${ticks || `<span class="track-note">${receipts.length ? "Every retained receipt is on an agent lane" : "No receipts retained"}</span>`}${nowLine}</div></li>
      ${agentRows}
    </ol>
    ${chosen && "timeline" in chosen.observed ? renderObserved(chosen.agent, chosen.observed.timeline, agents, now) : ""}
    ${receipts.length ? `<ol class="lane-events" aria-label="Recorded events, newest first">${[...receipts].reverse().map(receipt => { const agent = receiptAgent(receipt, agents); return `<li><time datetime="${new Date(receipt.created_at * 1000).toISOString()}">${esc(clock(receipt.created_at, now))}</time><strong>${esc(kindLabel(receipt.kind))}${agent ? ` <small class="lane-agent">${esc(agent.label)}</small>` : ""}</strong><span>${esc(receipt.summary)}</span></li>`; }).join("")}</ol>` : ""}
    <p class="lanes-note small muted">Showing the latest ${receipts.length} retained receipts. ${notes.length ? notes.join(" ") : "Agent lanes fill in when the server reports receipt attribution."}</p>
  </section>`;
}

/** The selected agent's observed events, newest first, with helpers it was seen spawning. */
function renderObserved(agent: Agent, timeline: AgentTimeline, agents: Agent[], now: number): string {
  const events = [...timeline.events].reverse().slice(0, 20);
  const spawned = timeline.children.filter(child => child.parent_thread === agent.thread).map(child => child.receiver_thread);
  const listed = agents.filter(item => spawned.includes(item.thread)).map(item => item.label);
  const unlisted = spawned.length - listed.length;
  const helpers = spawned.length ? `<p class="small muted">Spawned in Codex: ${esc([...listed, ...(unlisted ? [`${unlisted} unlisted helper${unlisted === 1 ? "" : "s"}`] : [])].join(", "))}</p>` : "";
  const freshness = timeline.freshness ? ` Read ${esc(clock(timeline.freshness.observed_at, now))}.` : "";
  return `<section class="observed-events" aria-label="${esc(`${OBSERVED} for ${agent.label}`)}">
    <h4>${OBSERVED} · ${esc(agent.label)}</h4>
    ${events.length ? `<ol class="lane-events">${events.map(event => `<li><time datetime="${new Date(event.at * 1000).toISOString()}">${esc(clock(event.at, now))}</time><strong>${esc(observedLabel[event.kind])}</strong><span>${esc(event.summary)}</span></li>`).join("")}</ol>` : '<p class="small muted">No timed activity on the latest page.</p>'}
    ${helpers}
    <p class="small muted">Not Factory proof.${freshness}</p>
  </section>`;
}
