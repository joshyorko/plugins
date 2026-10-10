import { allowedFinishes, classifyRun, needsOperatorDecision, finishLabels, runPath, type FollowUpKind, type FollowUpTask, type RunView } from "./domain";
import type { ViewState } from "./controller";
import { renderGraph } from "./graph-view";

export type Editor = "start" | "settings" | "steer" | "stop" | "repositories" | null;
export type WorkbenchSurface = "inline" | "global" | "thread";
export interface WorkbenchOptions {
  surface?: WorkbenchSurface;
  displayMode?: string;
  canSendFollowUps?: boolean;
  messagePending?: boolean;
  canExpand?: boolean;
}
const escape = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
const icon = (name: "moon" | "plus" | "arrow" | "refresh" | "settings" | "close") => {
  const paths = { moon: '<path d="M17.4 13.3A7 7 0 0 1 6.7 2.6 7.5 7.5 0 1 0 17.4 13.3Z"/>', plus: '<path d="M10 4v12M4 10h12"/>', arrow: '<path d="m7 5 5 5-5 5"/>', refresh: '<path d="M16 7a6.3 6.3 0 1 0 .3 5M16 3v4h-4"/>', settings: '<path d="M4 6h12M4 14h12M7 3v6M13 11v6"/>', close: '<path d="m5 5 10 10M15 5 5 15"/>' };
  return `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name]}</svg>`;
};
const stateLabel = (run: RunView): string => {
  if (run.planning_only) return "Planning only";
  if (!run.control || !run.presentation) return "Outcome unverified";
  if (!run.owner_thread && !run.control.attempts.length && run.presentation.result.kind !== "working" && run.presentation.result.kind !== "finished_verified") return "Not started";
  if (run.presentation.result.kind === "stopped_unresolved" && run.presentation.budget.time_remaining_seconds === 0) return "Stopped; time limit reached";
  return run.presentation.result.label;
};
const runTone = (run: RunView): string => run.planning_only ? "planning" : needsOperatorDecision(run) ? "needs" : run.presentation?.result.kind === "finished_verified" ? "finished" : run.presentation?.result.kind === "working" ? "active" : run.presentation?.result.kind === "stopped_unresolved" ? "stopped" : "unverified";
const badge = (run: RunView): string => `<span class="badge ${classifyRun(run)} ${runTone(run)}"><span></span>${escape(stateLabel(run))}</span>`;
const reasonCopy = (reason: string | null): string => {
  if (!reason) return "No additional reason was reported.";
  const messages: Record<string, string> = {
    planning_only: "Reads the saved plan. Execution has not started.", evidence_missing: "Evidence has not been collected.", owned_execution_active: "Owned execution is active.", read_only_status: "Reads current status without inference.",
    pending_owner_decision: "The completed owner decision needs an answer.", current_owned_turn: "Correction is fenced to the current turn.",
    stop_requires_native_cessation_proof: "Ownership stays held until native cessation is observed.", same_goal_remaining_budget: "Continue the same objective within its remaining limits.",
    effect_outcome_unknown: "Reconcile the existing effect before any new work.", owned_liveness_unknown: "Owned execution has not been proved stopped.",
    time_budget_exhausted: "The original time limit is exhausted.", repair_budget_exhausted: "The original repair limit is exhausted.",
    diagnosis_required: "Two attempts made no progress; a diagnosis is required.", read_only_native_reconciliation: "Observe existing native work without starting or stopping it.",
    inspect_native_evidence: "Inspect retained native evidence.",
  };
  return messages[reason] ?? "The server has not established a safe action.";
};
const deliverableLabel = (kind: "local_candidate" | "push" | "pr_ready"): string => ({ local_candidate: "Local candidate", push: "Pushed branch", pr_ready: "PR ready" })[kind];
const disabled = (value: boolean): string => value ? " disabled" : "";
const button = (action: string, label: string, options: { primary?: boolean; disabled?: boolean; icon?: "plus" | "refresh" | "settings" | "close" } = {}): string => `<button type="button" id="action-${action}" data-action="${action}" class="button${options.primary ? " primary" : ""}"${disabled(options.disabled ?? false)}>${options.icon ? icon(options.icon) : ""}${escape(label)}</button>`;
type PresentedAction = NonNullable<RunView["presentation"]>["actions"][number];
const actionButton = (action: PresentedAction, primary = false, busy = false): string => `<button type="button" class="button${primary ? " primary" : ""}" id="control-${escape(action.kind)}" data-action="control" data-kind="${escape(action.kind)}" data-tool="${escape(action.tool ?? "")}"${disabled(busy || !action.allowed || !action.tool)} aria-describedby="action-reason">${escape(action.label)}</button>`;

export function renderWorkbench(root: HTMLElement, state: ViewState, editor: Editor, preview: boolean, options: WorkbenchOptions = {}): void {
  options = { ...options, canSendFollowUps: state.connected && options.canSendFollowUps === true && !state.pending && !state.refreshing && !state.graphLoading && !state.graphStale };
  const confirmed = root.querySelector<HTMLInputElement>("#confirm-graph-change");
  const confirmationIdentity = confirmed?.checked ? confirmed.dataset.confirmIdentity : undefined;
  const oldForm = root.querySelector<HTMLFormElement>("form");
  const oldKind = oldForm?.dataset.form;
  const draft = oldForm ? new FormData(oldForm) : null;
  const active = document.activeElement;
  const focusId = active instanceof HTMLElement && root.contains(active) ? active.id : null;
  const selection = active instanceof HTMLTextAreaElement ? [active.selectionStart, active.selectionEnd] : null;
  const expanded = Array.from(root.querySelectorAll<HTMLDetailsElement>("details[open]")).map(element => element.id);
  const run = state.runs.find(item => item.id === state.selectedId);
  const surface = options.surface ?? "global";
  if (surface === "inline") {
    const inlineRun = run ?? state.runs[0];
    root.innerHTML = `<div class="workbench inline-surface" data-surface="inline" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic host data · No live integration</div>' : ""}${state.connectionStatus === "disconnected" && !state.error ? '<p class="connection-note" role="status">Disconnected · Reopen Luna Factory to refresh current state.</p>' : ""}${state.error ? `<p class="inline-feedback" role="alert">${escape(state.error)}</p>` : ""}${state.notice ? `<p class="inline-feedback" role="status">${escape(state.notice)}</p>` : ""}${inlineRun ? `<article class="inline-card" aria-labelledby="inline-title"><header><div>${badge(inlineRun)}<span class="inline-progress">${inlineRun.active_workers} active · ${inlineRun.presentation?.criteria.proven ?? 0}/${inlineRun.presentation?.criteria.mandatory ?? 0} proven</span></div><h2 id="inline-title">${escape(inlineRun.objective.slice(0, 300))}</h2></header><p class="inline-blocker">${escape(inlineRun.blocker || inlineRun.remaining_gap || inlineRun.delta || "No new blocker reported.")}</p>${renderFollowUpActions(inlineRun, options)}<div class="inline-actions"><button class="button primary" type="button" data-action="expand-mode"${disabled(!options.canExpand || !state.connected)}>Review in Luna Factory</button></div></article>` : `<article class="inline-card"><h2>No Factory result yet</h2><p>Open the Luna Factory sidebar to choose a run.</p></article>`}</div>`;
    if (focusId) findElementById(root, focusId)?.focus({ preventScroll: true });
    return;
  }
  const canStart = Boolean(state.connected && state.capabilities?.repositories.length && state.capabilities.profiles.length && !state.pending);
  const needs = state.runs.filter(item => classifyRun(item) === "needs").length;
  const activeCount = state.runs.filter(item => classifyRun(item) === "active").length;
  root.innerHTML = `<div class="workbench" data-surface="${surface}" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">
    ${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic run data · No live integration</div>' : ""}
    <header class="topbar"><a class="brand" href="/" data-action="overview"><span class="brand-mark">${icon("moon")}</span><span>Luna Factory<span class="brand-sub">WORKBENCH</span></span></a><div class="top-actions"><span class="connection"><i class="${state.connected ? "online" : ""}"></i>${state.connectionStatus === "connected" ? "Connected" : state.connectionStatus === "disconnected" ? "Disconnected" : "Connecting"}</span>${button("settings", "Settings", { icon: "settings", disabled: !state.capabilities || Boolean(state.pending) })}${button("start", "New run", { primary: true, icon: "plus", disabled: !canStart })}</div></header>
    <div class="layout"><aside class="sidebar" aria-label="Factory runs"><div class="sidebar-heading"><button class="overview-button" type="button" data-action="overview">All runs <span>${state.runs.length}</span></button><button class="icon-button" type="button" data-action="refresh" aria-label="Refresh runs"${disabled(!state.connected || state.refreshing || Boolean(state.pending))}>${icon("refresh")}</button></div>
      ${([ ["needs", "Needs me"], ["active", "Active"], ["recent", "Plans and history"] ] as const).map(([group, label]) => {
        const runs = state.runs.filter(item => classifyRun(item) === group);
        return `<section class="run-group"><h2>${label}<span>${runs.length}</span></h2>${runs.length ? runs.map(item => `<a id="run-nav-${escape(item.id)}" class="run-nav ${state.selectedId === item.id ? "selected" : ""}" href="${runPath(item.id)}" data-run-id="${escape(item.id)}"${state.selectedId === item.id ? ' aria-current="page"' : ""}><div class="nav-repository">${escape(item.repository)}<span class="state-dot ${group} ${runTone(item)}" title="${escape(stateLabel(item))}"></span></div><div class="nav-objective">${escape(item.objective)}</div><p>${escape(group === "needs" ? item.blocker || item.remaining_gap || stateLabel(item) : item.delta)}</p></a>`).join("") : `<p class="group-empty">${group === "needs" ? "Nothing needs your attention" : group === "active" ? "No work in progress" : "Completed runs appear here"}</p>`}</section>`;
      }).join("")}
      <div class="sidebar-note"><span class="tiny-moon">${icon("moon")}</span><p>One owner. Bounded work.<br>Evidence before completion.</p></div>
    </aside><main id="main" class="main">
      <div class="status-area" aria-live="polite" aria-atomic="true">${state.pending ? `<div class="notice pending"><span class="spinner"></span>${state.pending.tool === "cancel_factory_run" ? "Stopping owned execution. Waiting for verified status…" : "Waiting for the server…"}</div>` : state.refreshing ? '<div class="quiet-status">Refreshing persisted state…</div>' : ""}${state.notice ? `<div class="notice success">${escape(state.notice)}</div>` : ""}${state.error ? `<div class="notice error" role="alert">${escape(state.error)}${button("refresh", "Refresh", { disabled: !state.connected || Boolean(state.pending) })}</div>` : ""}${state.contextError ? `<div class="notice">${escape(state.contextError)}</div>` : ""}</div>
      ${surface === "thread" ? '<div class="thread-inspector-label">Thread inspector</div>' : ""}${editor ? renderEditor(editor, state, run) : !state.initialized ? '<section class="empty welcome"><span class="large-moon">' + icon("moon") + '</span><h1>Opening your factory</h1><p>Waiting for the host’s initial run data.</p><p class="muted">Open Luna Factory from your MCP Apps host to connect.</p></section>' : state.selectedId ? run ? renderRun(run, state, options) : state.graph ? renderGraph(state) : '<section class="empty"><h1>Opening this run</h1><p>The exact run is being read from the server.</p></section>' : renderOverview(state, needs, activeCount, canStart)}
      <footer class="footer">${state.refreshing ? "Reading current state" : "Status reads use no inference"}<span>Native Codex execution</span></footer>
    </main></div></div>`;
  const newForm = root.querySelector<HTMLFormElement>("form");
  if (draft && newForm && oldKind === newForm.dataset.form && oldForm?.dataset.inputIdentity === newForm.dataset.inputIdentity) {
    for (const field of newForm.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input[name],textarea[name],select[name]")) {
      const value = draft.get(field.name);
      if (field instanceof HTMLSelectElement && field.multiple) {
        const values = draft.getAll(field.name);
        for (const option of field.options) option.selected = values.includes(option.value);
      } else if (typeof value === "string") field.value = value;
    }
  }
  const nextConfirmation = root.querySelector<HTMLInputElement>("#confirm-graph-change");
  if (confirmationIdentity && nextConfirmation?.dataset.confirmIdentity === confirmationIdentity) {
    nextConfirmation.checked = true;
    const apply = root.querySelector<HTMLButtonElement>('[data-action="apply-change"]');
    if (apply) apply.disabled = !state.connected || !!state.pending || state.graphStale || state.graphLoading;
  }
  for (const id of expanded) { const element = document.getElementById(id); if (element instanceof HTMLDetailsElement && root.contains(element)) element.open = true; }
  if (focusId) {
    const element = findElementById(root, focusId);
    element?.focus({ preventScroll: true });
    if (element instanceof HTMLTextAreaElement && selection?.[0] !== undefined && selection[1] !== undefined) element.setSelectionRange(selection[0], selection[1]);
  }
}

function findElementById(root: HTMLElement, id: string): HTMLElement | undefined {
  return Array.from(root.querySelectorAll<HTMLElement>("[id]")).find(element => element.id === id);
}

function renderOverview(state: ViewState, needs: number, active: number, canStart: boolean): string {
  const featured = state.runs.filter(run => classifyRun(run) === "needs");
  const others = state.runs.filter(run => classifyRun(run) !== "needs");
  return `<section class="page-heading"><span class="eyebrow">YOUR WORK, IN MOTION</span><h1>A little attention.<br>A lot of progress.</h1><p>See what changed, what remains, and where you’re needed.</p></section>
    <div class="summary-grid"><div class="summary-card attention"><span>Needs me</span><strong>${needs.toString().padStart(2, "0")}</strong><small>${needs ? "An answer or approval is available" : "You’re clear for now"}</small></div><div class="summary-card"><span>Active runs</span><strong>${active.toString().padStart(2, "0")}</strong><small>Owned, bounded work in progress</small></div><div class="summary-card"><span>Plans and history</span><strong>${state.runs.filter(run => classifyRun(run) === "recent").length.toString().padStart(2, "0")}</strong><small>History stays with each run</small></div></div>
    ${!state.capabilities?.repositories.length ? `<div class="notice repository-empty"><div><strong>No repositories configured</strong><p>Choose a local repository offered by your operator, then request access.</p></div>${button("repositories", "Add repository", { icon: "plus", disabled: !state.connected || Boolean(state.pending) })}</div>` : ""}
    ${state.runs.length ? `<section class="activity-section"><div class="section-heading"><h2>${featured.length ? "Your next decision" : "Latest changes"}</h2><span>Delta first</span></div>${[...featured, ...others].slice(0, 8).map(run => `<a id="activity-${escape(run.id)}" class="activity-card" href="${runPath(run.id)}" data-run-id="${escape(run.id)}"><div class="activity-top"><span class="repo-label">${escape(run.repository)}</span>${badge(run)}</div><h3>${escape(run.objective)}</h3><p class="delta">${escape(run.delta)}</p><div class="activity-bottom"><span>${escape(run.blocker || run.remaining_gap || "No remaining gap reported")}</span>${icon("arrow")}</div></a>`).join("")}</section>` : `<section class="empty empty-card"><span class="large-moon">${icon("moon")}</span><h2>No runs yet</h2><p>Give Luna one objective and a clear finish line.<br>Its progress and evidence will stay here.</p>${button("start", "Start your first run", { primary: true, icon: "plus", disabled: !canStart })}</section>`}`;
}

function renderRun(run: RunView, state: ViewState, options: WorkbenchOptions): string {
  const busy = !state.connected || Boolean(state.pending);
  const attention = needsOperatorDecision(run);
  const presentation = run.control ? run.presentation : undefined;
  const primary = presentation?.primary_action;
  const action = primary ?? { kind: "refresh" as const, label: "Refresh current state", reason: "This older response has no authoritative action projection.", tool: "refresh_factory", allowed: true };
  const counts = presentation?.criteria;
  const evidence = run.receipts.map((receipt, index) => `<details class="receipt" id="receipt-${escape(run.id)}-${index}"><summary><span class="receipt-dot"></span><span><strong>${escape(receipt.kind.replaceAll("_", " "))}</strong><span class="receipt-summary">${escape(receipt.summary)}</span></span><time datetime="${new Date(receipt.created_at * 1000).toISOString()}">${new Date(receipt.created_at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time></summary><div class="receipt-body"><p>${escape(receipt.summary)}</p><dl><dt>Exact subject</dt><dd class="mono">${escape(receipt.subject)}</dd><dt>Freshness</dt><dd>${receipt.subject === run.current_subject ? "Matches the current subject" : "Different subject; re-verification may be needed"}</dd></dl></div></details>`).join("");
  const criteria = run.control?.criteria ?? run.acceptance.map((description, index) => ({ id: `A${index + 1}`, description, status: "unproved" as const, reason: null, check_refs: [] }));
  const taskDetails = run.control?.tasks.map(task => `<article class="ledger-row"><strong>${escape(task.title || task.id)}</strong><span>${escape(task.state)} · ${escape(task.admission)}</span><p>${escape(task.reason ? reasonCopy(task.reason) : task.criterion_ids.length ? `Criteria ${task.criterion_ids.join(", ")}` : "No criterion binding reported")} · prerequisites ${escape(task.dependencies.join(", ") || "none reported")} · owner ${escape(task.owner_thread || "unknown")} · attempts ${escape(task.attempt_ids.join(", ") || "none reported")}</p></article>`).join("") ?? "<p class=\"muted\">Task detail is unavailable.</p>";
  const attemptDetails = run.control?.attempts.map(attempt => `<article class="ledger-row"><strong>${escape(attempt.id)} · ${escape(attempt.status)}</strong><span>${escape(attempt.subject)}</span><p>Task ${escape(attempt.task_id)} · intent ${attempt.intent_generation} · dispatch ${attempt.dispatch_generation} · thread ${escape(attempt.thread_id || "unknown")} · turn ${escape(attempt.turn_id || "unknown")}</p></article>`).join("") ?? "<p class=\"muted\">Attempt lineage is unavailable.</p>";
  const secondaryActions = presentation?.actions.filter(item => item.allowed && ["cancel", "steer"].includes(item.kind) && item.kind !== primary?.kind && item.tool) ?? [];
  const otherActions = secondaryActions.map(item => actionButton(item, false, busy)).join("");
  return `<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/" data-action="overview">Factory</a>${icon("arrow")}<span>${escape(run.repository)}</span></nav><section class="run-heading"><div class="run-title-row"><span class="repo-label">${escape(run.repository)}</span>${badge(run)}</div><h1>${escape(run.objective)}</h1><div class="run-meta"><span>${escape(finishLabels[run.finish])}</span><span>${escape(run.profile)} profile</span></div></section>
    <section class="delta-panel"><div class="eyebrow"><span class="live-dot"></span>What changed</div><p>${escape(run.delta || "No new delta reported")}</p><div class="remaining"><span>${run.planning_only ? "Evidence to collect" : "Still needed"}</span><p>${escape(run.remaining_gap || "No mandatory gap reported")}</p></div></section>
    <section class="decision-panel needs-josh"><div class="eyebrow">${attention ? "Needs Josh" : run.planning_only ? "Plan navigation" : run.blocker ? "Execution blocker" : "Status and evidence"}</div><h2>${escape(attention ? run.pending_decision?.question || action.label : run.planning_only ? action.label : run.blocker || action.label)}</h2><p id="action-reason">${escape(reasonCopy(presentation ? action.reason : null))}${presentation ? " The server rechecks its decision at the action boundary." : " Refresh to load the server action and proof view."}</p><div class="run-controls">${actionButton(action, true, busy)}<a id="exact-run-link" class="run-link" href="${runPath(run.id)}" data-run-id="${escape(run.id)}">Exact run ${icon("arrow")}</a></div>${otherActions ? `<details class="other-actions"><summary>Other available actions</summary><div class="run-controls">${otherActions}</div></details>` : ""}</section>
    ${renderFollowUpActions(run, options, state.graph?.nodes.find(node => node.id === state.selectedNodeId))}
    ${presentation ? `<section class="summary-grid criteria-summary" aria-label="Criterion proof summary"><div class="summary-card"><span>Proven</span><strong>${counts?.proven ?? 0}</strong></div><div class="summary-card${counts?.failed ? " attention" : ""}"><span>Failed</span><strong>${counts?.failed ?? 0}</strong></div><div class="summary-card"><span>Evidence pending</span><strong>${counts?.unproved ?? 0}</strong><small>${counts?.mandatory ?? 0} mandatory criteria</small></div></section>` : `<div class="notice">Criterion status is unverified because this server response has no shared proof projection.</div>`}
    ${run.planning_only ? '<p class="plan-state">Execution has not started. This plan has no owner or workers; evidence can be collected later.</p>' : ""}
    <details id="execution-audit" class="detail-card advanced"><summary>Execution and delivery audit</summary><section class="delivery-strip"><div><span>Owner</span><strong>${run.planning_only ? "No owner started" : escape(presentation?.owner.liveness ?? "unknown")}</strong><small>${run.planning_only ? "No native owner is assigned" : escape(presentation?.owner.thread_id ?? "Identity unknown")}${presentation?.owner.turn_id ? ` · turn ${escape(presentation.owner.turn_id)}` : ""}</small></div><div><span>Workers</span><strong>${presentation ? `${presentation.workers.filter(worker => worker.liveness === "active").length} active · ${presentation.workers.filter(worker => worker.liveness === "unknown").length} unknown` : "Unknown"}</strong><small>${presentation?.workers.map(worker => `${escape(worker.thread_id)}: ${escape(worker.liveness)}`).join(", ") || "No worker identities reported"}</small></div><div><span>Time left</span><strong>${run.planning_only ? "Not running" : presentation?.result.kind !== "working" ? "Inactive" : presentation?.budget.time_remaining_seconds === null || presentation?.budget.time_remaining_seconds === undefined ? "Unverified" : `${Math.ceil(presentation.budget.time_remaining_seconds / 60)} min`}</strong><small>${presentation?.budget.repair_attempts_remaining ?? "?"} repairs remaining · ${presentation?.budget.repairs_used ?? "?"} used</small></div><div><span>Repository claim</span><strong>${presentation?.claim.status ?? "unknown"}</strong><small>${presentation?.claim.held ? "Retained" : presentation ? "Released" : "Status unknown"}</small></div><div><span>Deliverable</span><strong>${run.planning_only ? "No deliverable yet" : presentation ? `${presentation.deliverable.status} · ${deliverableLabel(presentation.deliverable.kind)}` : "unverified"}</strong><small class="mono">${escape(presentation?.deliverable.subject ?? "Subject unknown")}${presentation?.deliverable.reference ? ` · ${escape(presentation.deliverable.reference)}` : ""}</small></div></section></details>
    ${renderGraph(state)}
    <details id="criteria-details" class="detail-card advanced"><summary>Criteria and evidence · ${criteria.length} mandatory</summary><ol class="acceptance-list">${criteria.map(criterion => `<li><span class="criterion-state ${escape(criterion.status)}">${escape(criterion.status)}</span><div><p>${escape(criterion.description)}</p>${criterion.reason ? `<small class="muted">${escape(reasonCopy(criterion.reason))}</small>` : ""}${criterion.check_refs.length ? `<small class="muted">Checks: ${criterion.check_refs.map(escape).join(", ")}</small>` : ""}</div></li>`).join("")}</ol><p class="muted small">Child-task policy: ${escape(run.control?.child_policy ?? "unverified")}.</p></details>
    <details id="task-attempt-details" class="detail-card advanced"><summary>Tasks and attempt lineage · ${run.control?.tasks.length ?? "unknown"} tasks · ${run.control?.attempts.length ?? "unknown"} attempts</summary><h2>Tasks</h2>${taskDetails}<h2>Attempts</h2>${attemptDetails}</details>
    <details id="route-details" class="detail-card advanced"><summary>Model route and runtime</summary><dl class="route-list"><dt>Requested</dt><dd>${escape(run.route.requested_model || "Not reported")}<small>${escape(run.route.requested_effort || "Effort not reported")}</small></dd><dt>Configured</dt><dd>${escape(run.route.configured_model || "Not reported")}<small>${escape(run.route.configured_effort || "Effort not reported")}</small></dd><dt>Observed</dt><dd>${escape(run.route.observed_model || "No execution-side model evidence")}<small>${escape(run.route.observed_effort || "Effort telemetry unavailable")}</small></dd><dt>Provider</dt><dd>${escape(run.route.configured_provider || "Configuration not reported")}<small>Downstream execution and billing unverified</small></dd></dl>${run.route.reroutes?.length ? `<div class="notice error">${run.route.reroutes.length} native model mismatch${run.route.reroutes.length === 1 ? "" : "es"} recorded. ${run.route.reroutes.map(route => `${escape(route.from_model)} → ${escape(route.to_model)} (${escape(route.thread_id)}, ${escape(route.turn_id)})`).join("; ")}</div>` : ""}</details>
    <details id="receipt-details" class="detail-card evidence-card advanced"><summary>Execution evidence · ${run.receipts.length} retained receipts</summary>${evidence || '<p class="muted">No execution receipts retained. A state label alone is not proof.</p>'}</details>
    ${run.non_goals.length ? `<details id="non-goals" class="detail-card advanced"><summary>Outside this run</summary><ul>${run.non_goals.map(goal => `<li>${escape(goal)}</li>`).join("")}</ul></details>` : ""}
    <details id="run-identity" class="identity advanced"><summary>Run identity and source subject</summary><dl><dt>Run</dt><dd class="mono">${escape(run.id)}</dd><dt>Current subject</dt><dd class="mono">${escape(run.current_subject)}</dd><dt>Intent generation</dt><dd>${run.control?.intent_generation ?? "unknown"}</dd><dt>Dispatch generation</dt><dd>${run.control?.dispatch_generation ?? "unknown"}</dd><dt>Deadline</dt><dd>${new Date(run.deadline_at * 1000).toLocaleString()}</dd></dl></details>`;
}

function renderFollowUpActions(run: RunView, options: WorkbenchOptions, task?: FollowUpTask): string {
  const actions: Array<{ kind: FollowUpKind; label: string }> = [
    { kind: "summary", label: "Summarize this run" },
  ];
  if (run.blocker || run.remaining_gap || run.pending_decision) {
    actions.push({ kind: "blocker", label: "Ask ChatGPT about this blocker" });
  }
  if (task || run.pending_decision) {
    actions.push({ kind: "choose", label: task ? "Help me choose for this task" : "Help me choose a safe next step" });
  }
  const messageUnavailable = !options.canSendFollowUps;
  const buttons = actions.map(action => `<button id="follow-up-${action.kind}-${escape(task?.id ?? run.id)}" type="button" class="button" data-action="chat-follow-up" data-kind="${action.kind}"${task ? ` data-task-id="${escape(task.id)}"` : ""}${disabled(messageUnavailable)}${options.messagePending ? ' aria-disabled="true"' : ""}>${escape(action.label)}</button>`).join("");
  return `<section class="chat-followups" aria-label="Ask ChatGPT about the selected Factory state"><h2>Continue in ChatGPT</h2><p>Each button sends this exact run and revision to the active conversation after your click. It does not start Factory work.</p><div class="run-controls">${buttons}</div>${messageUnavailable ? '<p class="muted" role="status">ChatGPT message sending is unavailable in this host.</p>' : ""}${options.messagePending ? '<p class="muted" role="status">Sending your message…</p>' : ""}</section>`;
}

const field = (name: string, label: string, control: string, hint = ""): string => `<div class="field"><label for="${name}">${label}</label>${control}${hint ? `<p class="field-hint" id="${name}-hint">${hint}</p>` : ""}</div>`;
function renderEditor(editor: Exclude<Editor, null>, state: ViewState, run: RunView | undefined): string {
  const title = { start: "Start a bounded run", settings: "Factory settings", repositories: "Add a local repository", steer: run?.pending_decision ? "Answer the owner" : "Steer this run", stop: "Stop this run?" }[editor];
  const locked = !state.connected || Boolean(state.pending);
  const header = `<div class="editor-heading"><div><span class="eyebrow">${editor === "start" ? "A CLEAR FINISH LINE" : "LUNA FACTORY"}</span><h1>${title}</h1></div>${button("close-editor", "Close", { icon: "close", disabled: Boolean(state.pending) })}</div>`;
  if (editor === "repositories") return header + renderRepositories(state);
  if (editor === "steer" || editor === "stop") {
    if (!run) return `${header}<p>Select a run first.</p>`;
    if (editor === "stop") return `${header}<section class="editor-card"><p class="lead">${escape(run.objective)}</p><p>The server interrupts the owned execution and checks its descendants before releasing the repository claim. Unknown survivors leave the run blocked.</p><p>The objective, evidence, and history stay with this run. A later resume keeps its original authority and remaining limits.</p><form data-form="stop"><div class="form-actions"><button class="button danger" type="submit"${disabled(locked)}>Stop owned execution</button>${button("close-editor", "Keep running", { disabled: Boolean(state.pending) })}</div></form></section>`;
    return `${header}<section class="editor-card">${run.pending_decision || run.blocker ? `<div class="decision-quote">${escape(run.pending_decision?.question || run.blocker || "")}</div>` : `<p>Send a correction to the current owner of <strong>${escape(run.repository)}</strong>.</p>`}<form data-form="steer" data-input-identity="${escape(run.pending_decision?.id || run.turn_id || run.id)}">${field("message", "Your answer or correction", '<textarea id="message" name="message" rows="6" required maxlength="4000" placeholder="What should the owner do differently?" aria-describedby="message-hint"></textarea>', run.pending_decision ? "Continues the same run and owner thread with your answer. Existing limits stay in place. Security approvals must be handled in native Codex." : "Sent to the current turn only. This cannot expand permissions or approve a security prompt.")}<div class="form-actions"><button class="button primary" type="submit"${disabled(locked || !run.pending_decision && !run.turn_id)}>Send to owner</button><span>Existing scope and finish authority stay in place</span></div></form></section>`;
  }
  const cap = state.capabilities;
  const repositories = editor === "settings" ? `<section class="editor-card"><div class="section-heading"><h2>Repositories</h2>${button("repositories", "Add repository", { icon: "plus", disabled: locked })}</div>${cap?.repositories.length ? `<ul class="repository-list">${cap.repositories.map(repo => `<li><strong>${escape(repo.alias)}</strong><span>${finishLabels[repo.max_finish]}</span></li>`).join("")}</ul>` : '<p class="muted">No approved aliases yet.</p>'}</section>` : "";
  if (!cap || !cap.repositories.length || !cap.profiles.length) return `${header}${repositories}<div class="notice">No approved repository and profile combination is available. Check the trusted server configuration.</div>`;
  const repository = cap.repositories[0];
  if (!repository) return header;
  const profileOptions = cap.profiles.map(profile => `<option value="${escape(profile.alias)}"${profile.alias === state.settings.profile ? " selected" : ""}>${escape(profile.alias)} · ${escape(profile.effort)}</option>`).join("");
  const finishes = editor === "settings" ? allowedFinishes("pr") : allowedFinishes(repository.max_finish);
  const finishOptions = finishes.map(finish => `<option value="${finish}"${finish === state.settings.finish ? " selected" : ""}>${finishLabels[finish]}</option>`).join("");
  return `${header}${repositories}<section class="editor-card"><p class="editor-intro">${editor === "start" ? "One objective, one durable owner. Choose the bounds before work begins." : "Safe defaults for new runs. These do not change repository access, provider settings, or trusted limits."}</p><form data-form="${editor}">
    ${editor === "start" ? field("repository", "Approved repository", `<select id="repository" name="repository" required>${cap.repositories.map(repo => `<option value="${escape(repo.alias)}">${escape(repo.alias)}</option>`).join("")}</select>`) + field("objective", "Objective", '<textarea id="objective" name="objective" rows="3" required maxlength="8000" placeholder="What should be true when this run finishes?"></textarea>') + field("acceptance", "Mandatory acceptance", '<textarea id="acceptance" name="acceptance" rows="3" required maxlength="32032" placeholder="One verifiable criterion per line" aria-describedby="acceptance-hint"></textarea>', "One criterion per line, up to 32. Headings and bullet markers are omitted; every remaining line is mandatory.") + field("non_goals", "Non-goals", '<textarea id="non_goals" name="non_goals" rows="2" maxlength="32032" placeholder="What should this run leave alone?" aria-describedby="non_goals-hint"></textarea>', "Optional, one per line. Headings and bullet markers are omitted.") : ""}
    <div class="form-grid">${field("finish", "Finish authority", `<select id="finish" name="finish">${finishOptions}</select>`)}${field("profile", "Approved runtime profile", `<select id="profile" name="profile">${profileOptions}</select>`)}</div>
    ${field("capacity", "Worker capacity", `<input id="capacity" name="capacity" type="number" min="1" max="${cap.limits.capacity}" step="1" value="${Math.min(state.settings.capacity || 1, cap.limits.capacity)}" required aria-describedby="capacity-hint">`, `At most ${cap.limits.capacity} concurrent workers`)}
    ${editor === "start" ? `<details id="run-budgets" class="budget-details"><summary>Time &amp; repair budget</summary><div class="form-grid">${field("wall_seconds", "Time limit, seconds", `<input id="wall_seconds" name="wall_seconds" type="number" min="30" max="${cap.limits.wall_seconds}" step="1" value="${Math.min(1800, cap.limits.wall_seconds)}" required>`)}${field("repair_attempts", "Repair attempts", `<input id="repair_attempts" name="repair_attempts" type="number" min="0" max="${cap.limits.repair_attempts}" step="1" value="${Math.min(2, cap.limits.repair_attempts)}" required>`)}</div></details><div class="authority-note">${icon("moon")}<p>The server validates every action. No merge, release, or deployment is granted by this form.</p></div>` : ""}
    <div class="form-actions">${editor === "start" ? `<button class="button primary" type="submit" value="plan"${disabled(locked)}>Create planning graph</button><button class="button" type="submit" value="start"${disabled(locked || cap.execution?.eligible !== true)}>Start run</button><span>Planning is available. Execution requires independently qualified authentication, entitlement and adapter support.</span>` : `<button class="button primary" type="submit"${disabled(locked)}>Save defaults</button><span>Applies to future runs</span>`}</div></form></section>`;
}

function renderRepositories(state: ViewState): string {
  const busy = !state.connected || Boolean(state.pending) || state.discovering;
  const discovery = state.discovery;
  const candidate = discovery?.candidates[0];
  return `<section class="editor-card"><div class="section-heading"><h2>Operator-offered repositories</h2>${button("discover-repositories", "Refresh repositories", { icon: "refresh", disabled: busy })}</div><p class="editor-intro">Request a trusted alias for a local repository. A local operator must approve access before it can be used. Nothing is cloned and no run starts here.</p>
    ${state.discovering ? '<p role="status">Reading the local catalog…</p>' : !state.capabilities?.repository_onboarding?.enabled ? '<div class="notice">Your operator has not enabled local discovery. Ask them to configure a trusted discovery root on the server.</div>' : !discovery ? '<p class="muted">Refresh repositories to read the available catalog.</p>' : !candidate ? '<div class="notice">No new repositories found. Approved aliases are already available in New run. Ask your operator to add a local repository under a trusted root, then refresh.</div>' : `<div class="form-actions native-form-action"><button class="button" type="button" data-action="chat-repository-form"${disabled(busy)}>Choose with a ChatGPT form</button><span>When supported by this host; the accessible form remains available below.</span></div><form data-form="repositories">${field("candidate_id", "Local repository", `<select id="candidate_id" name="candidate_id" required>${discovery.candidates.map(item => `<option value="${escape(item.id)}">${escape(item.name)} · ${escape(item.root_alias)}</option>`).join("")}</select>`, "Only repositories discovered by the server are offered.")}${field("alias", "Trusted alias", '<input id="alias" name="alias" required maxlength="64" pattern="[A-Za-z0-9_-]{1,64}" placeholder="sandbox-test" aria-describedby="alias-hint">', "Use letters, digits, hyphens and underscores. Runs use this alias, never a filesystem path.")}${field("max_finish", "Maximum finish authority", `<select id="max_finish" name="max_finish" required>${allowedFinishes(candidate.max_finish).map(finish => `<option value="${finish}">${finishLabels[finish]}</option>`).join("")}</select>`, "The operator's root policy is the ceiling. Choose the smallest authority needed.")}<div class="form-actions"><button class="button primary" type="submit"${disabled(busy)}>Request access</button><span>Requires local operator approval</span></div></form>`}
    ${discovery?.requests.length ? `<section class="repository-requests"><h3>Access requests</h3>${discovery.requests.map(request => `<article class="repository-request"><div class="section-heading"><strong>${escape(request.alias)}</strong><span>${request.status === "approved" ? "Approval recorded" : "Pending operator approval"}</span></div><p>${escape(request.name)} · ${escape(request.root_alias)} · ${finishLabels[request.max_finish]}</p>${request.status === "pending" ? `<p class="muted">Give the local operator this request ID: <code>${escape(request.id)}</code>. After approval, refresh repositories.</p>` : '<p class="muted">Current approved aliases appear in Settings and New run.</p>'}</article>`).join("")}</section>` : ""}
  </section>`;
}
