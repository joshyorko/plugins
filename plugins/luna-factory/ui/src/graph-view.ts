import type { ViewState } from "./controller";
import type { FactoryGraph, GraphChange, RunView } from "./domain";
import { roster, livenessLabel, type Agent } from "./agents";
import { canvasSize, edgePath, fitGeometry, layoutWaves, nodePosition, type MapGeometry, type MapNode, type WaveLayout } from "./campaign-map";
import { coordinatorToken, icon, taskGlyph, taskTone, taskToneLabel, workerToken } from "./lunar";
import { renderLanes } from "./lanes";

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
  const caption = graph ? (graph.planning_only ? "Planned tasks · execution has not started" : "Persisted tasks and attempt history") : run.control ? "Tasks from the run ledger" : "Task detail unavailable";
  const met = layout.prerequisitesMet.size;
  return `<section class="mission" aria-label="Campaign">
    <div class="mission-main">
      <div class="viewbar">
        <div class="segmented" role="group" aria-label="View">
          <button type="button" id="view-map" data-action="view-mode" data-mode="map" aria-pressed="${!lanes}">${icon("map")}Map</button>
          <button type="button" id="view-lanes" data-action="view-mode" data-mode="lanes" aria-pressed="${lanes}">${icon("lanes")}Lanes</button>
        </div>
        <p class="viewbar-caption">${esc(caption)}${tasks.length ? ` · ${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}${met ? ` · ${met} with prerequisites met` : ""}${layout.remainingChain.length ? ` · longest remaining chain ${layout.remainingChain.length}` : ""}` : ""}</p>
        <button class="button quiet small" type="button" id="action-graph" data-action="graph"${disabled(!state.connected || !!state.pending || state.graphLoading)}>${icon("refresh")}${state.graphLoading ? "Reading…" : graph ? "Refresh plan" : "Read plan"}</button>
      </div>
      ${state.graphStale ? '<div class="notice error" role="status">This plan may be stale. Its last valid view is kept; refresh before changing it.</div>' : ""}
      ${renderCrew(agents, state, run)}
      ${lanes ? renderLanes(run, agents, state) : loading ? '<div class="canvas-empty" role="status">Reading the plan…</div>' : tasks.length ? renderMap(tasks, layout, agents, state, fitGeometry(layout.waves.length, canvasWidth)) : '<div class="canvas-empty"><h3>No tasks yet</h3><p>Ask ChatGPT to import tasks into this plan. Changes are proposed first and applied only after you confirm.</p></div>'}
    </div>
    ${renderInspector(state, run, tasks, graph, layout, agents, followUps)}
  </section>`;
}

function renderCrew(agents: Agent[], state: ViewState, run: RunView): string {
  if (!agents.length) {
    const why = run.planning_only ? "No agents yet. Execution hasn't started on this plan." : "The server reports no agent identities for this run.";
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
  const neighbors = new Set<string>(selected ? [selected, ...(byId.get(selected)?.dependencies ?? []), ...(layout.dependents.get(selected) ?? [])] : []);
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
    return `<li class="wave${done === wave.length ? " settled" : ""}" style="--wave-x:${g.padX + index * (g.nodeWidth + g.columnGap)}px"><h3 class="wave-label">Wave ${index + 1}<span>${wave.length} ${wave.length === 1 ? "task" : "tasks"}${done ? ` · ${done} done` : ""}</span></h3><ol class="wave-nodes">${wave.map(id => {
      const task = byId.get(id)!;
      const position = nodePosition(layout, id, g)!;
      const tone = taskTone(task.state);
      const key = `${state.selectedId}:${id}`;
      const changed = seenStates.has(key) && seenStates.get(key) !== `${task.state}:${task.owner_thread}`;
      seenStates.set(key, `${task.state}:${task.owner_thread}`);
      const owner = agents.find(agent => agent.thread === task.owner_thread);
      const prerequisites = task.dependencies.map(dependency => byId.get(dependency)?.title ?? dependency);
      const missing = layout.missing.get(id)?.length ?? 0;
      return `<li><button type="button" id="node-${esc(id)}" class="map-node tone-${tone}${changed ? " changed" : ""}${id === selected ? " selected" : ""}${chain.has(id) ? " chain" : ""}${layout.prerequisitesMet.has(id) ? " met" : ""}${selected && !neighbors.has(id) ? " dim" : ""}" style="--x:${position.x}px;--y:${position.y + 24}px" data-action="graph-node" data-node-id="${esc(id)}" aria-pressed="${id === selected}" aria-describedby="node-state-${esc(id)}">
        <span class="node-top">${taskGlyph(tone)}<span class="node-title">${esc(task.title || id)}</span></span>
        <span class="node-meta" id="node-state-${esc(id)}">${id === "objective" ? "Objective · " : ""}${esc(taskToneLabel[tone])}${missing ? ` · ${missing} unknown prerequisite${missing === 1 ? "" : "s"}` : ""}</span>
        <span class="node-after">${prerequisites.length ? `After ${esc(prerequisites.join(", "))}` : "No prerequisites"}</span>
        ${owner ? `<span class="dock" title="${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}">${agentToken(owner, 20)}<span class="sr-only">Owned by ${esc(owner.label)}</span></span>` : ""}
      </button></li>`;
    }).join("")}</ol></li>`;
  }).join("");
  return `<div class="map-scroll"><div class="map-canvas" style="--map-w:${size.width}px;--map-h:${size.height + 24}px;--node-w:${g.nodeWidth}px">
    <svg class="map-edges" width="${size.width}" height="${size.height + 24}" viewBox="0 0 ${size.width} ${size.height + 24}" aria-hidden="true" focusable="false">${edges}</svg>
    <ol class="map-waves" aria-label="Tasks by wave">${waves}</ol>
  </div></div>${layout.cyclic ? '<p class="notice">The server reported a dependency loop. Waves are approximate until it is resolved.</p>' : ""}`;
}

const providers: Record<string, string> = { github: "GitHub", local: "Local", fixture: "Fixture" };
/** Provider only; opaque item identities stay in the inspector's source detail. */
export function sourceLabel(source: NonNullable<MapTask["source"]>): string {
  return providers[source.provider] ?? source.provider;
}

function renderInspector(state: ViewState, run: RunView, tasks: MapTask[], graph: FactoryGraph | null, layout: WaveLayout, agents: Agent[], followUps: (task?: { id: string; title: string }) => string): string {
  const selected = tasks.find(task => task.id === state.selectedNodeId);
  const agent = agents.find(item => item.thread === state.selectedAgent);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const criteria = graph?.criteria ?? run.control?.criteria ?? [];
  const attempts = graph?.attempts ?? run.control?.attempts ?? [];
  const agentSection = agent ? `<section class="inspector-agent" aria-label="Selected agent">${agentToken(agent, 30)}<div><div class="eyebrow">${agent.role === "coordinator" ? "Coordinator" : "Worker"}</div><h3>${esc(agent.label)}</h3><p>${esc(livenessLabel[agent.liveness])}${agent.taskId ? ` · owns ${esc(byId.get(agent.taskId)?.title ?? agent.taskId)}` : " · no task binding reported"}</p><p class="small muted">${attempts.filter(attempt => attempt.thread_id === agent.thread).length} recorded attempts. A per-agent timeline isn't available from this server yet.</p></div></section>` : "";
  if (!selected) return `<aside class="graph-inspector inspector" aria-label="Selected task inspector">${agentSection}<p class="muted">Select a task to see why it's in its wave, what it unblocks, and its proof.</p>${followUps()}</aside>`;
  const tone = taskTone(selected.state);
  const owner = agents.find(item => item.thread === selected.owner_thread);
  const wave = (layout.depth.get(selected.id) ?? 0) + 1;
  const unblocks = layout.dependents.get(selected.id) ?? [];
  const list = (ids: string[], empty: string) => ids.length ? `<ul class="task-list">${ids.map(id => { const task = byId.get(id); return `<li>${taskGlyph(taskTone(task?.state ?? ""), 14)}<span>${esc(task?.title ?? id)}<small>${esc(taskToneLabel[taskTone(task?.state ?? "")])}</small></span></li>`; }).join("")}</ul>` : `<p class="small muted">${empty}</p>`;
  const proof = selected.criterion_ids.map(id => { const criterion = criteria.find(item => item.id === id); return `<li class="proof-${esc(criterion?.status ?? "unverified")}"><strong>${esc(criterion ? criterion.status === "unproved" ? "Evidence pending" : criterion.status === "proven" ? "Proven" : "Failed" : "Unverified")}</strong><span>${esc(criterion?.description ?? id)}</span>${criterion?.check_refs.length ? `<small>Checks: ${criterion.check_refs.map(esc).join(", ")}</small>` : ""}</li>`; }).join("");
  const taskAttempts = attempts.filter(attempt => attempt.task_id === selected.id);
  const graphNode = graph?.nodes.find(node => node.id === selected.id);
  return `<aside class="graph-inspector inspector" aria-label="Selected task inspector">
    ${agentSection}
    <div class="eyebrow">Task · wave ${wave}${selected.source ? ` · ${esc(sourceLabel(selected.source))}` : ""}</div>
    <h3>${esc(selected.title || selected.id)}</h3>
    <p class="inspector-state">${taskGlyph(tone)}<span>${esc(taskToneLabel[tone])}${selected.admission ? ` · admission ${esc(selected.admission.toLowerCase())}` : ""}</span></p>
    <dl class="route-list"><dt>Owner</dt><dd>${owner ? `${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}` : selected.owner_thread ? "An agent the server no longer lists" : "No agent assigned"}</dd><dt>Reason</dt><dd>${esc(reasonText(selected.reason))}</dd>${selected.source ? `<dt>Source</dt><dd>${esc(sourceLabel(selected.source))}<small class="mono">${esc(selected.source.item_id)}</small></dd>` : ""}${"target_preference" in selected ? `<dt>Planning note</dt><dd>${esc(selected.target_preference ?? "No target preference")}</dd>` : ""}</dl>
    <h4>Waits on</h4>${list(selected.dependencies, "No prerequisites.")}
    <h4>Unblocks</h4>${list(unblocks, "Nothing in this plan waits on it.")}
    <h4>Proof</h4>${proof ? `<ul class="graph-proof">${proof}</ul>` : '<p class="small muted">No criterion bindings.</p>'}
    ${followUps({ id: selected.id, title: selected.title })}
    ${taskAttempts.length ? `<h4>Attempts</h4><ul class="attempt-list">${taskAttempts.map(attempt => `<li><strong>${esc(attempt.status.replaceAll("_", " "))}</strong><span>${esc(agents.find(item => item.thread === attempt.thread_id)?.label ?? "Unlisted agent")}</span></li>`).join("")}</ul>` : ""}
    ${graphNode ? renderPlanEditor(state, graph!, graphNode) : ""}
  </aside>`;
}

const reasons: Record<string, string> = {
  planning_candidate_no_authority: "Planned. No execution authority has been granted.", evidence_missing: "Evidence has not been collected.",
};
function reasonText(reason: string | null): string {
  if (!reason) return "No blocker reported";
  return reasons[reason] ?? (/^[a-z_]+$/.test(reason) ? "Reported by the server; see Evidence and audit." : reason);
}

function renderPlanEditor(state: ViewState, graph: FactoryGraph, selected: FactoryGraph["nodes"][number]): string {
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  const nodeLocked = locked || selected.admission !== "candidate" || !!selected.owner_thread || !!selected.attempt_ids.length;
  const eligible = state.backends?.targets.filter(target => target.operator_enabled && target.planning_eligible && target.qualification !== "unsupported") ?? [];
  return `<details id="edit-${esc(selected.id)}" class="planning-editor"><summary>Edit this planned task</summary><p class="field-hint">Selecting a task only inspects it. Saving records a proposal for review. Only a confirmed application changes the plan, and no work is dispatched.</p><form data-form="graph-node" data-input-identity="${esc(graph.run_id)}:${esc(selected.id)}:${graph.revision}">
      ${nodeLocked && !locked ? '<p class="field-hint">Only planned tasks with no execution history can be edited.</p>' : ""}
      <div class="field"><label for="dependencies">Prerequisite tasks</label><select id="dependencies" name="dependencies" multiple size="${Math.min(5, Math.max(2, graph.nodes.length - 1))}"${disabled(nodeLocked)}>${graph.nodes.filter(node => node.id !== selected.id).map(node => `<option value="${esc(node.id)}"${selected.dependencies.includes(node.id) ? " selected" : ""}>${esc(node.title)}</option>`).join("")}</select><p class="field-hint">Choose the tasks that must finish first. ${selected.dependencies.length ? "Clearing all selections proposes removal of existing prerequisites." : "There are no prerequisites yet. Empty selection leaves the plan unchanged."} The server rejects cycles.</p></div><button class="button" type="submit" value="dependencies"${disabled(nodeLocked)}>Save prerequisite proposal</button>
      <details id="target-${esc(selected.id)}" class="target-editor"><summary>Optional planning target note</summary><div class="field target-field"><label for="target_id">Planning target note</label><select id="target_id" name="target_id"${disabled(nodeLocked || !eligible.length)}><option value=""${selected.target_preference === null ? " selected" : ""}>No preference</option>${eligible.map(target => `<option value="${esc(target.id)}"${selected.target_preference === target.id ? " selected" : ""}${disabled(!target.planning_eligible)}>${esc(target.label)} · execution unverified</option>`).join("") || '<option value="">Read targets with Refresh plan</option>'}</select><p class="field-hint">A planning preference does not authorize execution or verify subscription access.</p></div><button class="button" type="submit" value="target"${disabled(nodeLocked || !eligible.length)}>Save planning note proposal</button></details>
    </form></details>`;
}

/** Proposal review stays prominent: nothing applies without an exact, confirmed revision. */
export function renderProposalReview(state: ViewState): string {
  const { graph, proposal } = state;
  if (!graph || !proposal) return "";
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  return `<section class="graph-review" aria-label="Review graph change"><div class="eyebrow">${proposal.status === "applied" ? "Applied plan change" : "Review before applying"}</div><h3>${esc(changeTitle(proposal.change))}</h3><p>${esc(changeDescription(proposal.change, graph))}</p>${proposal.status === "proposed" ? `<label class="change-confirmation"><input id="confirm-graph-change" type="checkbox" data-confirm-identity="${esc(proposal.id)}:${graph.revision}"${disabled(locked || proposal.base_revision + 1 !== graph.revision)}> I confirm this exact plan change at revision ${graph.revision}.</label><div class="review-actions"><button id="apply-graph-change" class="button primary" type="button" data-action="apply-change" disabled>Apply confirmed plan change</button><span class="small muted">Records this change to the plan. No work is dispatched.</span></div>` : `<p class="small">Applied at revision ${proposal.applied_revision ?? "unknown"}.</p>`}<p class="small muted">Base revision ${proposal.base_revision} · change ${esc(proposal.id)}</p></section>`;
}

export function renderTargets(state: ViewState): string {
  return state.backends?.targets.map(target => `<article class="ledger-row"><strong>${esc(target.label)}</strong><span>${target.execution_eligible ? "Execution eligible" : "Execution unavailable"}</span><p>${esc(target.reason)} · ${esc(target.qualification)} · authentication ${esc(target.authentication)} · entitlement ${esc(target.entitlement)}</p></article>`).join("") || '<p class="muted small">Target catalog is unavailable. Refresh the plan to read it.</p>';
}

function changeTitle(change: GraphChange): string {
  return change.kind === "set_target" ? "Change execution preference" : change.kind === "set_dependencies" ? "Change prerequisite tasks" : "Import candidate tasks";
}
function changeDescription(change: GraphChange, graph: FactoryGraph): string {
  if (change.kind === "set_target") return `${change.node_id}: prefer ${change.target_id}.`;
  if (change.kind === "set_dependencies") return `${change.node_id}: current prerequisites ${graph.nodes.find(node => node.id === change.node_id)?.dependencies.join(", ") || "none"}; proposed prerequisites ${change.dependencies.join(", ") || "none"}.`;
  return `${change.nodes.length} candidates: ${change.nodes.map(node => node.title).join(", ")}.`;
}
