// Activity is a display of server-recorded events and Codex observations. It never changes plan state.
import type { RunView } from "./domain";
import type { ViewState } from "./controller";
import { livenessLabel, type Agent } from "./agents";
import { coordinatorToken, workerToken } from "./lunar";
import { clock } from "./narrative";

const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
type Receipt = RunView["receipts"][number];
type ActivityItem = { at: number; actor: string; summary: string };

/** The listed agent named by a server-recorded event, if the server supplied its thread. */
export function receiptAgent(receipt: Receipt, agents: Agent[]): Agent | undefined {
  return receipt.thread_id ? agents.find(agent => agent.thread === receipt.thread_id) : undefined;
}

export function renderLanes(run: RunView, agents: Agent[], state: ViewState, now = Date.now() / 1000): string {
  const tasks = state.graph?.run_id === run.id ? state.graph.nodes : run.control?.tasks ?? [];
  const taskTitles = new Map(tasks.map(task => [task.id, task.title]));
  const timelines = agents.map(agent => {
    const view = state.timelineRunId === run.id ? state.timelines[agent.thread] : undefined;
    return { agent, events: view?.status === "observed" ? view.events : [] };
  });
  const agentRows = agents.length ? `<section class="activity-agents" aria-labelledby="activity-agents-title"><h2 id="activity-agents-title">Agents</h2><ul>${timelines.map(({ agent, events }) => {
    const receipts = run.receipts.filter(receipt => receiptAgent(receipt, agents) === agent).map(receipt => receipt.created_at);
    const latest = Math.max(...events.map(event => event.at), ...receipts);
    const task = agent.taskId ? taskTitles.get(agent.taskId) : undefined;
    const role = agent.role === "coordinator" ? "Lead" : `Agent ${agent.ordinal}`;
    const activity = task ? `Working on ${task}` : agent.role === "coordinator" ? "Leading this plan" : "No task reported";
    return `<li class="activity-agent"><button type="button" data-action="select-agent" data-thread-id="${esc(agent.thread)}" aria-pressed="${state.selectedAgent === agent.thread}">${agent.role === "coordinator" ? coordinatorToken(agent.liveness, 24) : workerToken(agent.ordinal, agent.liveness, 24)}<span><strong>${esc(agent.label)}</strong><small>${role} · ${esc(livenessLabel[agent.liveness])}</small></span><span class="activity-agent-task">${esc(activity)}</span><time>${Number.isFinite(latest) ? esc(relative(latest, now)) : "No recent event"}</time></button></li>`;
  }).join("")}</ul></section>` : "";
  const events: ActivityItem[] = run.receipts.map(receipt => ({ at: receipt.created_at, actor: receipt.thread_id ? receiptAgent(receipt, agents)?.label ?? "Agent" : "Plan", summary: plainSummary(receipt.summary) }));
  for (const { agent, events: observed } of timelines) for (const event of observed) events.push({ at: event.at, actor: agent.label, summary: plainSummary(event.summary) });
  events.sort((a, b) => b.at - a.at);
  const feed = events.length ? `<ol class="activity-feed" aria-label="Recent activity">${events.map(event => `<li class="activity-event"><time datetime="${new Date(event.at * 1000).toISOString()}">${esc(clock(event.at, now))}</time><div><strong>${esc(event.actor)}</strong><span>${esc(event.summary)}</span></div></li>`).join("")}</ol>` : '<p class="activity-empty">No recent activity has been reported.</p>';
  const observedEvents = timelines.flatMap(({ agent, events: items }) => items.map(event => ({ agent, event }))).sort((a, b) => b.event.at - a.event.at);
  const timeline = observedEvents.length ? `<details class="activity-timeline"><summary>Timeline</summary><ol>${observedEvents.map(({ agent, event }) => `<li><time datetime="${new Date(event.at * 1000).toISOString()}">${esc(clock(event.at, now))}</time><strong>${esc(agent.label)}</strong><span>${esc(plainSummary(event.summary))}</span></li>`).join("")}</ol></details>` : "";
  return `<section class="activity-view" aria-label="Activity">${agentRows}<section class="activity-history"><h2>Recent activity</h2>${feed}</section>${timeline}</section>`;
}

function relative(at: number, now: number): string {
  const seconds = Math.max(0, Math.round(now - at));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  if (seconds < 48 * 3600) return `${Math.round(seconds / 3600)}h`;
  return new Date(at * 1000).toLocaleDateString([], { month: "short", day: "numeric" });
}

export function plainSummary(value: string): string {
  return value.replace(/native child thread/gi, "Codex task")
    .replace(/native child/gi, "Codex task")
    .replace(/native Codex/gi, "Codex")
    .replace(/\bnative\b/gi, "Codex")
    .replace(/owner turn/gi, "Luna's turn")
    .replace(/\bowner\b/gi, "Luna")
    .replace(/candidate subject/gi, "current plan")
    .replace(/\bcandidate\b/gi, "planned task")
    .replace(/\bcampaigns?\b/gi, match => match.toLowerCase().endsWith("s") ? "plans" : "plan")
    .replace(/\bwaves?\b/gi, match => match.toLowerCase().endsWith("s") ? "steps" : "step")
    .replace(/\bcriteria\b|\bcriterion\b/gi, "check")
    .replace(/\badmission\b/gi, "approval")
    .replace(/\brevision\b/gi, "version")
    .replace(/\bsubject\b/gi, "plan")
    .replace(/\breceipts?\b/gi, "activity")
    .replace(/\bliveness\b/gi, "status")
    .replace(/\binference\b/gi, "guess")
    .replace(/\baction boundary\b/gi, "click")
    .replace(/\bsafe action\b/gi, "next step")
    .replace(/\bclaim held\b/gi, "work remains paused")
    .replace(/supplied,? not verified/gi, "linked by you")
    .replace(/plan navigation/gi, "plan menu");
}
