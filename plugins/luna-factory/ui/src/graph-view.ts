import type { ViewState } from "./controller";
import { providerLabel, type FactoryGraph, type GraphChange, type RunView } from "./domain";
import { roster, livenessLabel, type Agent } from "./agents";
import { canvasSize, edgePath, fitGeometry, layoutWaves, nodePosition, type MapGeometry, type MapNode, type WaveLayout } from "./campaign-map";
import { coordinatorToken, icon, taskGlyph, taskTone, taskToneLabel, workerToken } from "./lunar";
import { renderLanes } from "./lanes";
import { blockerCopy } from "./narrative";
import { deliveryChip, deliverySection } from "./delivery-view";

/** Last rendered state per run task. A difference between two server snapshots is the only motion trigger. */
const seenStates = new Map<string, string>();
const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const textareaValue = (value: string): string => {
  const escaped = esc(value).replace(/\r\n?/g, "\n");
  return escaped.startsWith("\n") ? `&#10;${escaped.slice(1)}` : escaped;
};
const disabled = (condition: boolean) => condition ? " disabled" : "";

type MapTask = MapNode & { criterion_ids: string[]; attempt_ids: string[]; admission: string; reason: string | null; source?: FactoryGraph["nodes"][number]["source"]; target_preference?: string | null; editor_title?: string };
/** The persisted graph when loaded for this run, else the run's own task ledger. */
export function missionTasks(state: ViewState, run: RunView | undefined): { tasks: MapTask[]; graph: FactoryGraph | null } {
  const graph = state.graph && run && state.graph.run_id === run.id ? state.graph : null;
  return { tasks: graph?.nodes ?? run?.control?.tasks ?? [], graph };
}

export function agentToken(agent: Agent, size?: number): string {
  return agent.role === "coordinator" ? coordinatorToken(agent.liveness, size) : workerToken(agent.ordinal, agent.liveness, size);
}

/** Tasks, Activity and Details with the selected task sheet. */
export function renderMission(state: ViewState, run: RunView, followUps: (task?: { id: string; title: string }) => string, canvasWidth: number | undefined, details: string): string {
  const { tasks, graph } = missionTasks(state, run);
  const agents = roster(run, graph);
  const layout = layoutWaves(tasks);
  const activityAvailable = !run.planning_only && (agents.length > 0 || run.receipts.length > 0 || Object.keys(state.timelines).length > 0);
  const lanes = state.viewMode === "lanes" && activityAvailable;
  const showingDetails = state.viewMode === "details";
  const loading = !graph && state.graphLoading;
  const selectedTask = state.selectedNodeId && tasks.some(task => task.id === state.selectedNodeId);
  const selectedAgent = state.selectedAgent && agents.some(agent => agent.thread === state.selectedAgent);
  const tab = `<nav class="plan-tabs" role="tablist" aria-label="Plan sections"><button type="button" role="tab" id="view-map" data-action="view-mode" data-mode="map" aria-selected="${!lanes && !showingDetails}" aria-pressed="${!lanes && !showingDetails}">Tasks</button>${activityAvailable ? `<button type="button" role="tab" id="view-lanes" data-action="view-mode" data-mode="lanes" aria-selected="${lanes}" aria-pressed="${lanes}">Activity</button>` : ""}<button type="button" role="tab" id="view-details" data-action="view-mode" data-mode="details" aria-selected="${showingDetails}" aria-pressed="${showingDetails}">Details</button></nav>`;
  const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  const fontScale = Number.isFinite(rootFontSize) && rootFontSize > 0 ? rootFontSize / 16 : 1;
  const content = showingDetails ? details : lanes ? renderLanes(run, agents, state) : loading ? '<div class="canvas-empty" role="status">Loading tasks…</div>' : tasks.length ? renderMap(tasks, layout, agents, state, fitGeometry(layout.waves.length, canvasWidth, fontScale)) : '<div class="canvas-empty"><h3>No tasks yet</h3><p>Ask in chat to add tasks to this plan.</p></div>';
  const activeTab = showingDetails ? "view-details" : lanes ? "view-lanes" : "view-map";
  return `<section class="mission${selectedTask || selectedAgent ? " has-inspector" : ""}" aria-label="Plan tasks">
    <div class="mission-main">${tab}${state.graphStale && state.connected ? '<div class="notice error" role="status">This plan changed. Refresh to see the latest version.</div>' : ""}<div class="mission-panel" id="plan-panel" role="tabpanel" aria-labelledby="${activeTab}">${content}</div></div>
    ${!showingDetails && (selectedTask || lanes && selectedAgent) ? renderInspector(state, run, tasks, graph, layout, agents, followUps) : ""}
  </section>`;
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
      const missing = layout.missing.get(id)?.length ?? 0;
      const displayNumber = task.source ? sourceNumber(task.source) : null;
      const reference = displayNumber ?? "";
      const missingLabel = missing ? `${missing} task${missing === 1 ? " isn't" : "s aren't"} in this plan` : "";
      const nodeMeta = [reference, taskToneLabel[tone], missingLabel].filter(Boolean).join(" · ");
      return `<li><button type="button" id="node-${esc(id)}" class="map-node tone-${tone}${changed ? " changed" : ""}${id === selected ? " selected" : ""}${chain.has(id) ? " chain" : ""}${layout.prerequisitesMet.has(id) ? " met" : ""}" style="--x:${position.x}px;--y:${position.y + 24}px" data-action="graph-node" data-node-id="${esc(id)}" aria-pressed="${id === selected}" aria-describedby="node-state-${esc(id)}">
        <span class="node-top">${taskGlyph(tone)}<span class="node-title">${esc(task.title || id)}</span></span>
        <span class="node-meta" id="node-state-${esc(id)}" title="${esc(nodeMeta)}">${reference ? `<span class="node-ref">${esc(reference)}</span> · ` : ""}${esc(taskToneLabel[tone])}${missingLabel ? ` · ${esc(missingLabel)}` : ""}</span>
        ${deliveryChip(state, state.selectedId ?? undefined, id)}
        ${owner ? `<span class="dock" title="${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}">${agentToken(owner, 20)}<span class="sr-only">Worked on by ${esc(owner.label)}</span></span>` : ""}
      </button></li>`;
    }).join("")}</ol></li>`;
  }).join("");
  return `<div class="map-scroll"><div class="map-canvas" style="--map-w:${size.width}px;--map-h:${size.height + 24}px;--node-w:${g.nodeWidth}px;--node-h:${g.nodeHeight}px">
    <svg class="map-edges" width="${size.width}" height="${size.height + 24}" viewBox="0 0 ${size.width} ${size.height + 24}" aria-hidden="true" focusable="false">${edges}</svg>
    <ol class="map-waves" aria-label="Tasks by step">${waves}</ol>
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
  const link = url ? `<a class="source-link" href="${esc(url)}" target="_blank" rel="noopener noreferrer" title="Open issue in GitHub">${esc(text)}</a>` : esc(text);
  return number || url ? `${number ? `${esc(label)} ` : ""}${link}` : esc(label);
}

function renderInspector(state: ViewState, run: RunView, tasks: MapTask[], graph: FactoryGraph | null, layout: WaveLayout, agents: Agent[], followUps: (task?: { id: string; title: string }) => string): string {
  const selected = tasks.find(task => task.id === state.selectedNodeId);
  const agent = agents.find(item => item.thread === state.selectedAgent);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const criteria = graph?.criteria ?? run.control?.criteria ?? [];
  const agentSection = agent ? `<section class="inspector-agent" aria-label="Selected agent">${agentToken(agent, 30)}<div><h3>${esc(agent.label)}</h3><p>${agent.role === "coordinator" ? "Lead" : `Agent ${agent.ordinal}`} · ${esc(livenessLabel[agent.liveness])}${agent.taskId ? ` · working on ${esc(byId.get(agent.taskId)?.title ?? "a task")}` : ""}</p></div></section>` : "";
  const blocker = blockerCopy(run);
  if (!selected) return `<aside class="graph-inspector inspector" aria-label="Selected agent details">${agentSection}${agent ? "" : followUps()}</aside>`;
  const tone = taskTone(selected.state);
  const owner = agents.find(item => item.thread === selected.owner_thread);
  const selectedEditorTitle = selected.editor_title ?? selected.title;
  const wave = (layout.depth.get(selected.id) ?? 0) + 1;
  const unblocks = layout.dependents.get(selected.id) ?? [];
  const list = (ids: string[]) => `<ul class="task-list">${ids.map(id => { const task = byId.get(id); return `<li>${taskGlyph(taskTone(task?.state ?? ""), 14)}<span>${esc(task?.title ?? "Task unavailable")}<small>${esc(taskToneLabel[taskTone(task?.state ?? "")])}</small></span></li>`; }).join("")}</ul>`;
  const proof = selected.criterion_ids.map(id => { const criterion = criteria.find(item => item.id === id); return `<li class="proof-${esc(criterion?.status ?? "unverified")}"><span>${esc(criterion?.description ?? "This check was not described")}</span><strong>${esc(criterion ? criterion.status === "unproved" ? "Not checked yet" : criterion.status === "proven" ? "Done" : "Needs attention" : "Not checked yet")}</strong>${criterion?.check_refs.length ? `<small>Checks: ${criterion.check_refs.map(esc).join(", ")}</small>` : ""}</li>`; }).join("");
  const graphNode = graph?.nodes.find(node => node.id === selected.id);
  const reason = reasonText(selected.reason);
  const agentRow = owner ? `${esc(owner.label)} · ${esc(livenessLabel[owner.liveness])}` : selected.owner_thread ? "An agent is no longer listed" : "";
  const drawer = `<aside class="graph-inspector inspector drawer task-sheet" aria-label="Task details">
    ${agentSection}
    <header class="drawer-heading"><div><h3>${esc(selected.title || "Task")}</h3><p>${taskGlyph(tone)} ${esc(taskToneLabel[tone])} · Step ${wave}${selected.source ? ` · ${sourceReference(selected.source)}` : ""}</p></div><button class="button icon-only" type="button" data-action="close-task" aria-label="Close task">${icon("close")}</button></header>
    ${state.connectionStatus === "disconnected" ? '<div class="sheet-offline" role="status"><span>Offline. Showing saved details.</span><button class="button quiet" type="button" data-action="reconnect">Reconnect</button></div>' : ""}
    ${blocker ? `<p class="blocker-kind">${esc(blocker)}</p>` : ""}${agentRow ? `<p class="task-agent">${agentRow}</p>` : ""}${reason ? `<p class="inspector-note">${esc(reason)}</p>` : ""}
    ${selected.dependencies.length ? `<section class="drawer-section"><h4>Waits on</h4>${list(selected.dependencies)}</section>` : ""}
    ${unblocks.length ? `<section class="drawer-section"><h4>Unblocks</h4>${list(unblocks)}</section>` : ""}
    ${proof ? `<section class="drawer-section"><h4>Done when</h4><ul class="graph-proof">${proof}</ul></section>` : ""}
    ${selected.source?.provider === "github" ? deliverySection(state, run.id, selected.id) : ""}
    <div class="inspector-actions">${followUps({ id: selected.id, title: selectedEditorTitle })}${graphNode ? `<button type="button" class="button edit-toggle" id="edit-toggle-${esc(selected.id)}" data-action="toggle-disclosure" aria-controls="edit-${esc(selected.id)}" aria-expanded="false">Edit</button>` : ""}</div>
  </aside>`;
  return `${drawer}${graphNode ? renderPlanEditor(state, graph!, graphNode) : ""}`;
}

const reasons: Record<string, string> = {
  evidence_missing: "Not checked yet.",
};
/** Plain reason, or nothing. */
function reasonText(reason: string | null): string {
  if (!reason || reason === "planning_candidate_no_authority") return "";
  return reasons[reason] ?? "";
}

function renderPlanEditor(state: ViewState, graph: FactoryGraph, selected: FactoryGraph["nodes"][number]): string {
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  const nodeLocked = locked || selected.admission !== "candidate" || !!selected.owner_thread || !!selected.attempt_ids.length;
  const eligible = state.backends?.targets.filter(target => target.operator_enabled && target.planning_eligible && target.qualification !== "unsupported") ?? [];
  const titleWithheld = selected.editor_title === null;
  const title = titleWithheld ? "" : selected.editor_title ?? selected.title;
  const titleLimit = selected.id === "objective" ? 8000 : 4000;
  return `<div id="edit-${esc(selected.id)}" class="planning-editor edit-sheet" data-disclosure hidden><section class="edit-sheet-card" role="dialog" aria-modal="true" aria-labelledby="edit-title-${esc(selected.id)}"><header class="edit-sheet-heading"><div><p class="eyebrow">Edit task</p><h2 id="edit-title-${esc(selected.id)}">Edit task</h2></div><button class="button icon-only" type="button" data-action="close-edit-sheet" aria-label="Close editor">${icon("close")}</button></header>${state.connectionStatus === "disconnected" ? '<div class="sheet-offline" role="status"><span>Offline. Showing saved details.</span><button class="button quiet" type="button" data-action="reconnect">Reconnect</button></div>' : ""}<form data-form="graph-node" data-input-identity="${esc(graph.run_id)}:${esc(selected.id)}:${graph.revision}">
      <div class="field"><label for="task-title">Title</label><textarea id="task-title" name="title" rows="2" maxlength="${titleLimit}" placeholder="${titleWithheld ? "Title hidden for privacy" : ""}"${disabled(nodeLocked)}>${textareaValue(title)}</textarea>${titleWithheld ? '<p class="field-hint">Leave blank to keep the original title, or enter a replacement.</p>' : ""}</div>
      ${nodeLocked && !locked ? '<p class="field-hint">Only planned tasks with no assigned work can be edited.</p>' : ""}
      <fieldset class="field dependency-field" id="dependencies"${disabled(nodeLocked)}><legend>Waits on</legend><div class="check-options">${graph.nodes.filter(node => node.id !== selected.id).map(node => `<label class="check-option"><input type="checkbox" name="dependencies" value="${esc(node.id)}"${selected.dependencies.includes(node.id) ? " checked" : ""}><span>${esc(node.title)}</span></label>`).join("")}</div></fieldset>
      <div class="field target-field"><label for="target_id">Where it runs</label><select id="target_id" name="target_id"${disabled(nodeLocked)}>${targetOptions(selected.target_preference ?? null, eligible)}</select></div>
      <p id="task-save-error" class="field-error" role="alert" hidden></p>
      <div class="editor-row"><button class="button primary" type="submit" value="save"${disabled(nodeLocked)}>Save changes</button></div>
    </form></section></div>`;
}

/** Keeps the current location when available; choosing No preference explicitly clears it. */
export function targetOptions(current: string | null, eligible: Array<{ id: string; label: string; planning_eligible: boolean }>): string {
  const offered = eligible.map(target => `<option value="${esc(target.id)}"${current === target.id ? " selected" : ""}${disabled(!target.planning_eligible)}>${esc(plainTargetLabel(target.label))}</option>`).join("");
  const noPreference = `<option value=""${current === null ? " selected" : ""}>No preference</option>`;
  const unavailable = current !== null && !eligible.some(target => target.id === current) ? `<option value="${esc(current)}" selected disabled>Current location unavailable</option>` : "";
  return noPreference + unavailable + offered;
}

function plainTargetLabel(value: string): string {
  const label = value.toLowerCase();
  return label === "native local" ? "Local machine" : label === "native codex" ? "Codex" : value;
}

/**
 * A proposed change stays prominent: nothing applies without an explicit, revision-fenced confirmation.
 * An applied change is only a quiet "Plan updated" note that the user can dismiss and that clears on the next read.
 */
export function renderProposalReview(state: ViewState): string {
  const { graph, proposal } = state;
  if (!graph || !proposal) return "";
  if (proposal.status === "applied") return `<div class="plan-updated" role="status" aria-label="Plan updated">${taskGlyph("done", 14)}<strong>Plan updated</strong><span>${esc(changeDescription(proposal.change, graph, state))}</span><button type="button" class="link-button" id="dismiss-plan-updated" data-action="dismiss-change">Dismiss</button></div>`;
  const locked = !state.connected || !!state.pending || state.graphStale || state.graphLoading || !graph.planning_only && (graph.claim.held || graph.claim.status !== "released");
  const current = proposal.base_revision + 1 === graph.revision;
  const change = proposal.change;
  const node = change.kind === "import_candidates" ? undefined : graph.nodes.find(item => item.id === change.node_id);
  const targetLabel = (id: string | null) => id === null ? "No preference" : plainTargetLabel(state.backends?.targets.find(target => target.id === id)?.label ?? "Current location");
  const canonicalNodeTitle = node?.editor_title === null ? null : node?.editor_title ?? node?.title ?? null;
  const titleField = change.kind === "update_task" && node && change.title !== null && (canonicalNodeTitle === null || change.title !== canonicalNodeTitle)
    ? [{ label: "Title", before: canonicalNodeTitle ?? "Title hidden for privacy", after: change.title }]
    : [];
  const updateFields = change.kind === "update_task" && node ? [
    ...titleField,
    ...(change.dependencies.length === node.dependencies.length && change.dependencies.every(id => node.dependencies.includes(id)) ? [] : [{ label: "Waits on", before: dependencyTitles(node.dependencies, graph), after: dependencyTitles(change.dependencies, graph) }]),
    ...(change.target_id === node.target_preference ? [] : [{ label: "Where it runs", before: targetLabel(node.target_preference), after: targetLabel(change.target_id) }]),
  ] : [];
  const updateList = (side: "before" | "after") => `<ul class="change-fields">${updateFields.map(field => `<li><strong>${esc(field.label)}</strong><span>${esc(field[side])}</span></li>`).join("")}</ul>`;
  const before = change.kind === "update_task" ? updateList("before") : change.kind === "set_target"
    ? plainTargetLabel(state.backends?.targets.find(target => target.id === node?.target_preference)?.label ?? "No location selected")
    : change.kind === "set_dependencies" ? dependencyTitles(node?.dependencies ?? [], graph) : "Nothing";
  const after = change.kind === "update_task" ? updateList("after") : change.kind === "set_target"
    ? plainTargetLabel(state.backends?.targets.find(target => target.id === change.target_id)?.label ?? "Selected location")
    : change.kind === "set_dependencies" ? dependencyTitles(change.dependencies, graph) : change.nodes.map(item => item.title).join(", ");
  const nodeTitle = node?.title;
  const title = change.kind === "update_task" ? `Review changes to ${nodeTitle ?? "this task"}` : change.kind === "set_target" ? `Change where ${nodeTitle ?? "this task"} runs` : change.kind === "set_dependencies" ? `Change what ${nodeTitle ?? "this task"} waits on` : "Add tasks to this plan";
  const content = (value: string) => change.kind === "update_task" ? value : esc(value);
  return `<section class="graph-review" aria-label="Review plan change"><h2>${esc(title)}</h2><dl class="change-diff"><div><dt>Before</dt><dd>${content(before)}</dd></div><div><dt>After</dt><dd>${content(after)}</dd></div></dl>${current ? "" : '<p class="small">This plan changed. Refresh and propose the update again.</p>'}<div class="review-actions"><button id="cancel-graph-change" class="button" type="button" data-action="cancel-review">Cancel</button><button id="apply-graph-change" class="button primary" type="button" data-action="apply-change"${disabled(locked || !current)}>Apply change</button></div></section>`;
}

function dependencyTitles(ids: string[], graph: FactoryGraph): string {
  return ids.length ? ids.map(id => graph.nodes.find(node => node.id === id)?.title ?? "Task unavailable").join(", ") : "Nothing (it can start right away)";
}
/** Task titles and target labels, never node or target identifiers. */
function changeDescription(change: GraphChange, graph: FactoryGraph, state: ViewState): string {
  const brief = (value: string) => value.length > 120 ? `${value.slice(0, 117)}…` : value;
  const title = (id: string) => brief(graph.nodes.find(item => item.id === id)?.title ?? "a task");
  const titles = (ids: string[]) => ids.length ? ids.map(title).join(", ") : "nothing";
  if (change.kind === "update_task") {
    const updatedTitle = change.title ?? title(change.node_id);
    return `Task “${brief(updatedTitle)}” updated.`;
  }
  if (change.kind === "set_target") return `${title(change.node_id)} prefers ${plainTargetLabel(state.backends?.targets.find(target => target.id === change.target_id)?.label ?? "the selected location")}.`;
  if (change.kind === "set_dependencies") return `${title(change.node_id)} will wait on ${titles(change.dependencies)}.`;
  return `${change.nodes.length} ${change.nodes.length === 1 ? "task" : "tasks"}: ${change.nodes.map(node => node.title).join(", ")}.`;
}
