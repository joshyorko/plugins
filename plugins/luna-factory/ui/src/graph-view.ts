import type { ViewState } from "./controller";
import { parentProvenanceLabel, providerLabel, type FactoryGraph, type GraphChange, type RunView } from "./domain";
import { roster, livenessLabel, type Agent } from "./agents";
import { canvasSize, edgePath, fitGeometry, layoutWaves, nodePosition, type MapGeometry, type MapNode, type WaveLayout } from "./campaign-map";
import { coordinatorToken, icon, taskGlyph, taskTone, taskToneLabel, workerToken } from "./lunar";
import { receiptAgent, renderLanes } from "./lanes";
import { blockerCopy } from "./narrative";
import { deliveryChip, deliverySection } from "./delivery-view";

/** Last rendered state per run task. A difference between two server snapshots is the only motion trigger. */
const seenStates = new Map<string, string>();
const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const disabled = (condition: boolean) => condition ? " disabled" : "";

type MapTask = MapNode & { criterion_ids: string[]; attempt_ids: string[]; admission: string; reason: string | null; source?: FactoryGraph["nodes"][number]["source"]; target_preference?: string | null };
/** The persisted graph when loaded for this run, else the run's own task ledger. */
export function missionTasks(state: ViewState, run: RunView | undefined): { tasks: MapTask[]; graph: FactoryGraph | null } {
  const graph = state.graph && run && state.graph.run_id === run.id ? state.graph : null;
  return { tasks: graph?.nodes ?? run?.control?.tasks ?? [], graph };
}

export function agentToken(agent: Agent, size?: number): string {
  return agent.role === "coordinator" ? coordinatorToken(agent.liveness, size) : workerToken(agent.ordinal, agent.liveness, size);
}

/** Map | Lanes, the crew, and the shared inspector. */
export function renderMission(state: ViewState, run: RunView, followUps: (task?: { id: string; title: string }) => string, canvasWidth?: number): string {
  const { tasks, graph } = missionTasks(state, run);
  const agents = roster(run, graph);
  const layout = layoutWaves(tasks);
  const lanes = state.viewMode === "lanes";
  const loading = !graph && state.graphLoading;
  const met = layout.prerequisitesMet.size;
  const planned = graph?.planning_only === true;
  const caption = tasks.length
    ? [`${tasks.length} ${planned ? "planned " : ""}${tasks.length === 1 ? "task" : "tasks"}`, met ? `${met} ready to start` : "", layout.remainingChain.length > 1 ? `longest path ${layout.remainingChain.length} tasks` : ""].filter(Boolean).join(" · ")
    : graph || run.control ? "No tasks yet" : "Task detail unavailable";
  return `<section class="mission" aria-label="Campaign">
    <div class="mission-main">
    <div class="viewbar">
        <div class="segmented" role="group" aria-label="View">
          <button type="button" id="view-map" data-action="view-mode" data-mode="map" aria-pressed="${!lanes}">${icon("map")}Map</button>
          <button type="button" id="view-lanes" data-action="view-mode" data-mode="lanes" aria-pressed="${lanes}">${icon("lanes")}Lanes</button>
        </div>
        <p class="viewbar-caption">${esc(caption)}</p>
      </div>
      ${state.graphStale ? '<div class="notice error" role="status">This plan may be out of date. Refresh it before making changes.</div>' : ""}
      ${renderCrew(agents, state, run)}
      ${lanes ? renderLanes(run, agents, state) : loading ? '<div class="canvas-empty" role="status">Reading the plan…</div>' : tasks.length ? renderMap(tasks, layout, agents, state, fitGeometry(layout.waves.length, canvasWidth)) : '<div class="canvas-empty"><h3>No tasks yet</h3><p>Ask ChatGPT to add tasks to this plan. You review every change before it applies.</p></div>'}
    </div>
    ${renderInspector(state, run, tasks, graph, layout, agents, followUps)}
  </section>`;
}

function renderCrew(agents: Agent[], state: ViewState, run: RunView): string {
  if (!agents.length) {
    const why = run.planning_only ? "No agents yet." : "No agents are reported for this run.";
    return `<div class="crew empty" aria-label="Agents">${orbit([])}<p>${esc(why)}</p></div>`;
  }
  return `<div class="crew" aria-label="Agents">${orbit(agents)}<ul class="crew-list">${agents.map(agent => `<li><button type="button" class="crew-agent${state.selectedAgent === agent.thread ? " selected" : ""}" id="agent-${esc(agent.ordinal)}" data-action="select-agent" data-thread-id="${esc(agent.thread)}" aria-pressed="${state.selectedAgent === agent.thread}">${agentToken(agent, 22)}<span><strong>${esc(agent.label)}</strong><small>${agent.role === "coordinator" ? "Coordinator" : "Worker"} · ${esc(livenessLabel[agent.liveness])}${agent.taskId ? ` · ${esc(clip(taskTitle(state, run, agent.taskId), 36))}` : ""}</small></span></button></li>`).join("")}</ul></div>`;
}

/** A small orbit diagram: one satellite per reported worker. Liveness shows in the rim only. */
function orbit(agents: Agent[]): string {
  const workers = agents.filter(agent => agent.role === "worker");
  const coordinator = agents.find(agent => agent.role === "coordinator");
  const satellites = workers.map((worker, index) => {
    const angle = (-90 + (360 / Math.max(1, workers.length)) * index) * Math.PI / 180;
    return `<circle cx="${(28 + 21 * Math.cos(angle)).toFixed(2)}" cy="${(28 + 21 * Math.sin(angle)).toFixed(2)}" r="4" class="sat live-${worker.liveness}"/>`;
  }).join("");
  return `<svg class="orbit" width="56" height="56" viewBox="0 0 56 56" aria-hidden="true" focusable="false"><circle cx="28" cy="28" r="21" class="orbit-path"/><path d="M31.5 17.5a11 11 0 1 0 6.1 17.6 8.8 8.8 0 0 1-6.1-17.6Z" class="orbit-moon${coordinator ? ` live-${coordinator.liveness}` : " absent"}"/>${satellites}</svg>`;
}

const clip = (value: string, length: number) => value.length > length ? `${value.slice(0, length - 1)}…` : value;
function taskTitle(state: ViewState, run: RunView, id: string): string {
  return missionTasks(state, run).tasks.find(task => task.id === id)?.title ?? id;
}

function renderMap(tasks: MapTask[], layout: WaveLayout, agents: Agent[], state: ViewState, g: MapGeometry): string {
  const size = canvasSize(layout, g);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const chain = new Set(layout.remainingChain);
  const selected = state.selectedNodeId;
  const edges = tasks.flatMap(task => task.dependencies.filter(id => byId.has(id)).map(id => {
    const from = nodePosition(layout, id, g); const to = nodePosition(layout, task.id, g);
    if (!from || !to) return "";
    const satisfied = taskTone(byId.get(id)?.state ?? "") === "done";
    const onChain = chain.has(id) && chain.has(task.id);
    const focus = selected && (task.id === selected || id === selected);
    return `<path d="${edgePath({ x: from.x, y: from.y + 24 }, { x: to.x, y: to.y + 24 }, g)}" class="edge${satisfied ? " satisfied" : " pending"}${onChain ? " chain" : ""}${focus ? " focus" : ""}"/>`;
  })).join("");
  const waves = layout.waves.map((wave, index) => {
    const done = wave.filter(id => taskTone(byId.get(id)?.state ?? "") === "done").length;
    return `<li class="wave${done === wave.length ? " settled" : ""}" style="--wave-x:${g.padX + index * (g.nodeWidth + g.columnGap)}px"><h3 class="wave-label">Step ${index + 1}<span>${wave.length} ${wave.length === 1 ? "task" : "tasks"}${done ? ` · ${done} done` : ""}</span></h3><ol class="wave-nodes">${wave.map(id => {
      const task = byId.get(id)!;
      const position = nodePosition(layout, id, g)!;
      const tone = taskTone(task.state);
      const key = `${state.selectedId}:${id}`;
      const changed = seenStates.has(key) && seenStates.get(key) !== `${task.state}:${task.owner_thread}`;
      seenStates.set(key, `${task.state}:${task.owner_thread}`);
      const owner = agents.find(agent => agent.thread === task.owner_thread);
      const prerequisites = task.dependencies.map(dependency => byId.get(dependency)?.title ?? dependency);
      const missing = layout.missing.get(id)?.length ?? 0;
      return `<li><button type="button" id="node-${esc(id)}" class="map-node tone-${tone}${changed ? " changed" : ""}${id === selected ? " selected" : ""}${chain.has(id) ? " chain" : ""}${layout.prerequisitesMet.has(id) ? " met" : ""}" style="--x:${position.x}px;--y:${position.y + 24}px" data-action="graph-node" data-node-id="${esc(id)}" aria-pressed="${id === selected}" aria-describedby="node-state-${esc(id)}">
        <span class="node-top">${taskGlyph(tone)}<span class="node-title">${esc(task.title || id)}</span></span>
        <span class="node-meta" id="node-state-${esc(id)}">${task.source && sourceNumber(task.source) ? `<span class="node-ref">${esc(sourceNumber(task.source)!)}</span> · ` : ""}${esc(taskToneLabel[tone])}${missing ? ` · ${missing} missing prerequisite${missing === 1 ? "" : "s"}` : ""}</span>
        ${deliveryChip(state, state.selectedId ?? undefined, id)}
        <span class="node-after">${prerequisites.length ? `After ${esc(prerequisites.join(", "))}` : "No prerequisites"}</span>
        ${owner ? `<span class="dock" title="${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}">${agentToken(owner, 20)}<span class="sr-only">Owned by ${esc(owner.label)}</span></span>` : ""}
      </button></li>`;
    }).join("")}</ol></li>`;
  }).join("");
  return `<div class="map-scroll"><div class="map-canvas" style="--map-w:${size.width}px;--map-h:${size.height + 24}px;--node-w:${g.nodeWidth}px">
    <svg class="map-edges" width="${size.width}" height="${size.height + 24}" viewBox="0 0 ${size.width} ${size.height + 24}" aria-hidden="true" focusable="false">${edges}</svg>
    <ol class="map-waves" aria-label="Tasks by wave">${waves}</ol>
  </div></div>${layout.cyclic ? '<p class="notice">Some tasks wait on each other in a loop, so the waves are approximate.</p>' : ""}`;
}

type Source = NonNullable<MapTask["source"]>;
/** Provider only; opaque item identities stay in the inspector's source detail. */
export function sourceLabel(source: Source): string {
  return providerLabel(source.provider);
}
/** `#N` only when the importer supplied a display number. Display data is never proof. */
export function sourceNumber(source: Source): string | null {
  return source.display?.number !== undefined ? `#${source.display.number}` : null;
}
/**
 * Provider label plus `#N`, linked only when a validated https://github.com/ URL was supplied.
 * Without display metadata this is exactly the provider-only label.
 */
export function sourceReference(source: Source): string {
  const label = sourceLabel(source);
  const number = sourceNumber(source);
  const url = source.display?.url;
  const text = number ?? label;
  const link = url ? `<a class="source-link" href="${esc(url)}" target="_blank" rel="noopener noreferrer" title="Link supplied at import · not Factory proof">${esc(text)}</a>` : esc(text);
  return number || url ? `${number ? `${esc(label)} ` : ""}${link}` : esc(label);
}

function renderInspector(state: ViewState, run: RunView, tasks: MapTask[], graph: FactoryGraph | null, layout: WaveLayout, agents: Agent[], followUps: (task?: { id: string; title: string }) => string): string {
  const selected = tasks.find(task => task.id === state.selectedNodeId);
  const agent = agents.find(item => item.thread === state.selectedAgent);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const criteria = graph?.criteria ?? run.control?.criteria ?? [];
  const attempts = graph?.attempts ?? run.control?.attempts ?? [];
  const attributed = agent ? run.receipts.filter(receipt => receiptAgent(receipt, agents) === agent).length : 0;
  const agentAttempts = agent ? attempts.filter(attempt => attempt.thread_id === agent.thread).length : 0;
  const agentSection = agent ? `<section class="inspector-agent" aria-label="Selected agent">${agentToken(agent, 30)}<div><div class="eyebrow">${agent.role === "coordinator" ? "Coordinator" : "Worker"}</div><h3>${esc(agent.label)}</h3><p>${esc(livenessLabel[agent.liveness])}${agent.taskId ? ` · working on ${esc(byId.get(agent.taskId)?.title ?? agent.taskId)}` : ""}</p><p class="small muted">${esc(`${attributed} ${attributed === 1 ? "receipt" : "receipts"} · ${agentAttempts} ${agentAttempts === 1 ? "attempt" : "attempts"}.`)} ${esc(timelineNote(state, agent.thread))}</p></div></section>` : "";
  const blocker = blockerCopy(run);
  if (!selected) return `<aside class="graph-inspector inspector" aria-label="Selected task inspector">${agentSection}<p class="muted inspector-empty">Select a task to see what it waits on, what it unblocks, and when it's done.</p>${followUps()}</aside>`;
  const tone = taskTone(selected.state);
  const owner = agents.find(item => item.thread === selected.owner_thread);
  const wave = (layout.depth.get(selected.id) ?? 0) + 1;
  const unblocks = layout.dependents.get(selected.id) ?? [];
  const list = (ids: string[], empty: string) => ids.length ? `<ul class="task-list">${ids.map(id => { const task = byId.get(id); return `<li>${taskGlyph(taskTone(task?.state ?? ""), 14)}<span>${esc(task?.title ?? id)}<small>${esc(taskToneLabel[taskTone(task?.state ?? "")])}</small></span></li>`; }).join("")}</ul>` : `<p class="small muted">${empty}</p>`;
  const proof = selected.criterion_ids.map(id => { const criterion = criteria.find(item => item.id === id); return `<li class="proof-${esc(criterion?.status ?? "unverified")}"><span>${esc(criterion?.description ?? "This check was not described")}</span><strong>${esc(criterion ? criterion.status === "unproved" ? "Not done yet" : criterion.status === "proven" ? "Done" : "Needs attention" : "Not checked")}</strong>${criterion?.check_refs.length ? `<small>Checks: ${criterion.check_refs.map(esc).join(", ")}</small>` : ""}</li>`; }).join("");
  const taskAttempts = attempts.filter(attempt => attempt.task_id === selected.id);
  const graphNode = graph?.nodes.find(node => node.id === selected.id);
  const reason = reasonText(selected.reason);
  // An owner row only appears once there is something to say about one.
  const ownerRow = owner ? `${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}` : selected.owner_thread ? "An agent the server no longer lists" : tone === "candidate" ? "" : "No agent assigned";
  return `<aside class="graph-inspector inspector" aria-label="Selected task inspector">
    ${agentSection}
    <div class="eyebrow">Step ${wave}${selected.source ? ` · ${esc(sourceLabel(selected.source))}${sourceNumber(selected.source) ? ` ${esc(sourceNumber(selected.source)!)}` : ""}` : ""}</div>
    <h3>${esc(selected.title || selected.id)}</h3>
    <dl class="route-list"><dt>Status</dt><dd class="inspector-state">${taskGlyph(tone)}<span>${esc(taskToneLabel[tone])}</span></dd>${blocker ? `<dt>Blocked by</dt><dd class="blocker-kind">${esc(blocker)}</dd>` : ""}${ownerRow ? `<dt>Owner</dt><dd>${ownerRow}</dd>` : ""}${reason ? `<dt>Note</dt><dd>${esc(reason)}</dd>` : ""}${selected.source ? `<dt>Source</dt><dd>${sourceReference(selected.source)}</dd>` : ""}</dl>
    ${campaignParentNote(state, run)}
    <h4>What it waits on</h4>${list(selected.dependencies, "Nothing.")}
    <h4>What it unblocks</h4>${list(unblocks, "Nothing else in this plan waits on it.")}
    <h4>Done when</h4>${proof ? `<ul class="graph-proof">${proof}</ul>` : '<p class="small muted">No completion check is linked to this task.</p>'}
    ${selected.source?.provider === "github" ? deliverySection(state, run.id, selected.id) : ""}
    ${taskAttempts.length ? `<h4>Attempts</h4><ul class="attempt-list">${taskAttempts.map(attempt => `<li><strong>${esc(attempt.status.replaceAll("_", " "))}</strong><span>${esc(agents.find(item => item.thread === attempt.thread_id)?.label ?? "Unlisted agent")}</span></li>`).join("")}</ul>` : ""}
    <div class="inspector-actions">${followUps({ id: selected.id, title: selected.title })}${graphNode ? `<button type="button" class="button small edit-toggle" id="edit-toggle-${esc(selected.id)}" data-action="toggle-disclosure" aria-controls="edit-${esc(selected.id)}" aria-expanded="false">Edit</button>` : ""}</div>
    ${graphNode ? renderPlanEditor(state, graph!, graphNode) : ""}
  </aside>`;
}

function campaignParentNote(state: ViewState, run: RunView): string {
  const campaign = state.campaigns?.find(item => item.planning_run_id === run.id || item.run_ids.includes(run.id));
  return campaign ? `<p class="inspector-note small muted">Parent issue ${esc(providerLabel(campaign.parent.forge.provider))} ${esc(campaign.parent.forge.repository)}#${campaign.parent.forge.number} · ${esc(parentProvenanceLabel(campaign))}</p>` : "";
}

const reasons: Record<string, string> = {
  evidence_missing: "Evidence has not been collected.",
};
/** Plain reason, or nothing. "Planned with no authority" is already said by the status and the not-running line. */
function reasonText(reason: string | null): string {
  if (!reason || reason === "planning_candidate_no_authority") return "";
  return reasons[reason] ?? (/^[a-z_]+$/.test(reason) ? "" : reason);
}

function renderPlanEditor(state: ViewState, graph: FactoryGraph, selected: FactoryGraph["nodes"][number]): string {
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  const nodeLocked = locked || selected.admission !== "candidate" || !!selected.owner_thread || !!selected.attempt_ids.length;
  const eligible = state.backends?.targets.filter(target => target.operator_enabled && target.planning_eligible && target.qualification !== "unsupported") ?? [];
  return `<div id="edit-${esc(selected.id)}" class="planning-editor" data-disclosure hidden><form data-form="graph-node" data-input-identity="${esc(graph.run_id)}:${esc(selected.id)}:${graph.revision}">
      ${nodeLocked && !locked ? '<p class="field-hint">Only planned tasks that have never run can be edited.</p>' : ""}
      <fieldset class="field dependency-field" id="dependencies"${disabled(nodeLocked)}><legend>Waits on</legend><div class="check-options">${graph.nodes.filter(node => node.id !== selected.id).map(node => `<label class="check-option"><input type="checkbox" name="dependencies" value="${esc(node.id)}"${selected.dependencies.includes(node.id) ? " checked" : ""}><span>${esc(node.title)}</span></label>`).join("")}</div><p class="field-hint">Tasks that must finish first. ${selected.dependencies.length ? "Clear every box to remove them." : ""} Loops are rejected.</p></fieldset>
      <div class="editor-row"><button class="button small" type="submit" value="dependencies"${disabled(nodeLocked)}>Propose this order</button></div>
      <div class="field target-field"><label for="target_id">Preferred target</label><select id="target_id" name="target_id"${disabled(nodeLocked || !eligible.length)}>${targetOptions(selected.target_preference ?? null, eligible)}</select><p class="field-hint">A planning preference does not authorize execution or verify subscription access.</p></div>
      <div class="editor-row"><button class="button small" type="submit" value="target"${disabled(nodeLocked || !eligible.length)}>Propose this target</button></div>
      <p class="field-hint">You review each proposal before the plan changes. Nothing runs.</p>
    </form></div>`;
}

/**
 * An empty target is never a submittable value. "No preference" is only a disabled placeholder
 * while no note is set; a saved note cannot be cleared here, because the server has no clear change.
 */
export function targetOptions(current: string | null, eligible: Array<{ id: string; label: string; planning_eligible: boolean }>): string {
  const offered = eligible.map(target => `<option value="${esc(target.id)}"${current === target.id ? " selected" : ""}${disabled(!target.planning_eligible)}>${esc(target.label)} · execution unverified</option>`).join("");
  const placeholder = current === null
    ? '<option value="" selected disabled>No preference</option>'
    : eligible.some(target => target.id === current) ? "" : `<option value="${esc(current)}" selected disabled>${esc(current)} · current note</option>`;
  return placeholder + (offered || '<option value="" disabled>Targets appear after you refresh the plan</option>');
}

/**
 * A proposed change stays prominent: nothing applies without an explicit, revision-fenced confirmation.
 * An applied change is only a quiet "Plan updated" note that the user can dismiss and that clears on the next read.
 */
export function renderProposalReview(state: ViewState): string {
  const { graph, proposal } = state;
  if (!graph || !proposal) return "";
  if (proposal.status === "applied") return `<div class="plan-updated" role="status" aria-label="Plan updated">${taskGlyph("done", 14)}<strong>Plan updated</strong><span>${esc(changeDescription(proposal.change, graph, state, true))}</span><button type="button" class="link-button" id="dismiss-plan-updated" data-action="dismiss-change">Dismiss</button></div>`;
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  const current = proposal.base_revision + 1 === graph.revision;
  return `<section class="graph-review" aria-label="Review graph change"><div class="eyebrow">Review before applying</div><h3>${esc(changeTitle(proposal.change))}</h3><p>${esc(changeDescription(proposal.change, graph, state))}</p>${current ? "" : '<p class="small">The plan changed after this was proposed. Refresh and propose it again.</p>'}<label class="change-confirmation"><input id="confirm-graph-change" type="checkbox" data-confirm-identity="${esc(proposal.id)}:${graph.revision}"${disabled(locked || !current)}> I confirm this plan change.</label><div class="review-actions"><button id="apply-graph-change" class="button primary" type="button" data-action="apply-change" disabled>Apply change</button><span class="small muted">Updates the plan only. Nothing runs.</span></div></section>`;
}

export function renderTargets(state: ViewState): string {
  return state.backends?.targets.map(target => `<article class="ledger-row"><strong>${esc(target.label)}</strong><span>${target.execution_eligible ? "Execution eligible" : "Execution unavailable"}</span><p>${esc(target.reason)} · ${esc(target.qualification)} · authentication ${esc(target.authentication)} · entitlement ${esc(target.entitlement)}</p></article>`).join("") || '<p class="muted small">Target catalog is unavailable. Refresh the plan to read it.</p>';
}

function changeTitle(change: GraphChange): string {
  return change.kind === "set_target" ? "Change the preferred target" : change.kind === "set_dependencies" ? "Change what a task waits on" : "Add tasks to the plan";
}
/** Task titles and target labels, never node or target identifiers. */
function changeDescription(change: GraphChange, graph: FactoryGraph, state: ViewState, applied = false): string {
  const title = (id: string) => graph.nodes.find(node => node.id === id)?.title || "a task";
  const titles = (ids: string[]) => ids.length ? ids.map(title).join(", ") : "nothing";
  if (change.kind === "set_target") return `${title(change.node_id)} prefers ${state.backends?.targets.find(target => target.id === change.target_id)?.label ?? "the chosen target"}.`;
  if (change.kind === "set_dependencies") return `${title(change.node_id)} waits on ${titles(change.dependencies)}${!applied && graph.nodes.find(node => node.id === change.node_id)?.dependencies.length ? ` instead of ${titles(graph.nodes.find(node => node.id === change.node_id)!.dependencies)}` : ""}.`;
  return `${change.nodes.length} ${change.nodes.length === 1 ? "task" : "tasks"}: ${change.nodes.map(node => node.title).join(", ")}.`;
}
/** Where this agent's activity can be seen. Observations are never attempts or proof. */
function timelineNote(state: ViewState, thread: string): string {
  const capability = state.capabilities?.agent_timeline;
  if (!capability) return "";
  if (!capability.enabled) return capability.detail;
  const view = state.timelineRunId === state.selectedId ? state.timelines[thread] : undefined;
  return view?.status === "observed" ? `Lanes shows ${view.events.length} ${view.events.length === 1 ? "event" : "events"} observed in Codex (not Factory proof).` : "Open Lanes to see what Codex observed.";
}
