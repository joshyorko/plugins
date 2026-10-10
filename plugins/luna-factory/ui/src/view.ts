import { allowedFinishes, finishLabels, needsOperatorDecision, runPath, type FollowUpKind, type FollowUpTask, type RunView } from "./domain";
import type { ViewState } from "./controller";
import { renderMission, renderProposalReview, renderTargets, missionTasks } from "./graph-view";
import { layoutWaves } from "./campaign-map";
import { icon, lunaMark, phaseGlyph, taskTone } from "./lunar";
import { activityAt, blockerCopy, freshness, nowSentence, runTier, statusLabel, clock, type SinceTracker, type Tier } from "./narrative";
import { roster } from "./agents";
import { receiptAgent } from "./lanes";

export type Editor = "start" | "settings" | "steer" | "stop" | "repositories" | null;
export type WorkbenchSurface = "inline" | "global" | "thread";
export interface WorkbenchOptions {
  surface?: WorkbenchSurface;
  displayMode?: string;
  canSendFollowUps?: boolean;
  messagePending?: boolean;
  canExpand?: boolean;
  /** Whether the host itself supports messages; set from the incoming option before view gating. */
  hostCanMessage?: boolean;
  /** Session-scoped "since you opened Luna Factory" baselines. Absent in tests and fixtures. */
  since?: SinceTracker;
  now?: number;
  /** Measured map pane width, so wave columns fit before the canvas has to pan. */
  canvasWidth?: number;
}
const escape = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
const reasonCopy = (reason: string | null): string => {
  if (!reason) return "No additional reason was reported.";
  const messages: Record<string, string> = {
    planning_only: "Reads the saved plan. Execution has not started.", evidence_missing: "Evidence has not been collected.", owned_execution_active: "Owned execution is active.", read_only_status: "Reads current status without inference.",
    pending_owner_decision: "The completed owner decision needs an answer.", current_owned_turn: "Correction is fenced to the current turn.",
    stop_requires_native_cessation_proof: "Ownership stays held until native cessation is observed.", same_goal_remaining_budget: "Continue the same objective within its remaining limits.",
    effect_outcome_unknown: "Reconcile the existing effect before any new work.", owned_liveness_unknown: "Owned execution has not been proved stopped.",
    time_budget_exhausted: "The original time limit is exhausted.", repair_budget_exhausted: "The original repair limit is exhausted.",
    diagnosis_required: "Two attempts made no progress; a diagnosis is required.", read_only_native_reconciliation: "Observe existing native work without starting or stopping it.",
    inspect_native_evidence: "Inspect retained native evidence.", native_approval_requires_native_ui: "This approval can only be answered in native Codex.",
  };
  // Unmapped codes are never shown raw; prose reasons from the server are already plain language.
  return messages[reason] ?? (/\s/.test(reason) ? `${reason.replace(/[.!?]?$/, ".")}` : "The server reported this action without further detail.");
};
const deliverableLabel = (kind: "local_candidate" | "push" | "pr_ready"): string => ({ local_candidate: "Local candidate", push: "Pushed branch", pr_ready: "PR ready" })[kind];
const disabled = (value: boolean): string => value ? " disabled" : "";
const button = (action: string, label: string, options: { primary?: boolean; disabled?: boolean; icon?: "plus" | "refresh" | "settings" | "close"; quiet?: boolean; iconOnly?: boolean } = {}): string => `<button type="button" id="action-${action}" data-action="${action}" class="button${options.primary ? " primary" : ""}${options.quiet ? " quiet" : ""}${options.iconOnly ? " icon-only" : ""}"${options.iconOnly ? ` aria-label="${escape(label)}" title="${escape(label)}"` : ""}${disabled(options.disabled ?? false)}>${options.icon ? icon(options.icon) : ""}${options.iconOnly ? "" : escape(label)}</button>`;
type PresentedAction = NonNullable<RunView["presentation"]>["actions"][number];
const actionButton = (action: PresentedAction, primary = false, busy = false): string => `<button type="button" class="button${primary ? " primary" : ""}" id="control-${escape(action.kind)}" data-action="control" data-kind="${escape(action.kind)}" data-tool="${escape(action.tool ?? "")}"${disabled(busy || !action.allowed || !action.tool)} aria-describedby="action-reason">${escape(action.label)}</button>`;
const statusMark = (tier: Tier, label: string): string => `<span class="status tier-${tier}"><span class="status-shape" aria-hidden="true"></span>${escape(label)}</span>`;
function proofLine(run: RunView): string {
  const counts = run.control ? run.presentation?.criteria : undefined;
  if (!counts) return `<span class="proof unverified">${phaseGlyph(0, 0)}Proof unverified</span>`;
  return `<span class="proof">${phaseGlyph(counts.proven, counts.mandatory)}${counts.proven} of ${counts.mandatory} ${counts.mandatory === 1 ? "criterion" : "criteria"} proven${counts.failed ? ` · ${counts.failed} failed` : ""}</span>`;
}

export function renderWorkbench(root: HTMLElement, state: ViewState, editor: Editor, preview: boolean, options: WorkbenchOptions = {}): void {
  options = { ...options, hostCanMessage: options.canSendFollowUps === true, canSendFollowUps: state.connected && options.canSendFollowUps === true && !state.pending && !state.refreshing && !state.graphLoading && !state.graphStale };
  options.since?.observe(state.runs);
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
    root.innerHTML = renderInline(state, run ?? state.runs[0], options, preview);
    if (focusId) findElementById(root, focusId)?.focus({ preventScroll: true });
    return;
  }
  const canStart = Boolean(state.connected && state.capabilities?.repositories.length && state.capabilities.profiles.length && !state.pending);
  if (options.canvasWidth === undefined && root.clientWidth > 0) {
    // Mirrors the CSS: main max 1360px with 24px padding; the inspector column leaves at 861px+.
    const main = Math.min(root.clientWidth, 1360) - (root.clientWidth <= 520 ? 24 : 48);
    options.canvasWidth = (main > 860 ? main - 340 - 16 : main) - 2;
  }
  root.innerHTML = `<div class="workbench" data-surface="${surface}" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">
    ${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic run data · No live integration</div>' : ""}
    <header class="appbar"><a class="brand" href="/" data-action="overview" aria-label="Luna Factory campaigns">${lunaMark(22)}<span>Luna Factory</span></a>${run && !editor ? `<nav class="breadcrumbs" aria-label="Breadcrumb">${icon("arrow", 12)}<a href="/" data-action="overview">Campaigns</a>${icon("arrow", 12)}<span>${escape(run.repository)}</span></nav>` : ""}
      <div class="top-actions"><span class="connection" role="status"><i class="${state.connected ? "online" : ""}" aria-hidden="true"></i>${state.connectionStatus === "connected" ? "Connected" : state.connectionStatus === "disconnected" ? "Disconnected" : "Connecting"}</span>${button("refresh", "Refresh", { icon: "refresh", quiet: true, iconOnly: true, disabled: !state.connected || state.refreshing || Boolean(state.pending) })}${button("settings", "Settings", { icon: "settings", quiet: true, iconOnly: true, disabled: !state.capabilities || Boolean(state.pending) })}${button("start", "Plan work", { icon: "plus", quiet: true, disabled: !canStart })}</div></header>
    <main id="main" class="main">
      <div class="status-area" aria-live="polite" aria-atomic="true">${state.pending ? `<div class="notice pending"><span class="spinner" aria-hidden="true"></span>${state.pending.tool === "cancel_factory_run" ? "Stopping owned execution. Waiting for verified status…" : "Waiting for the server…"}</div>` : state.refreshing ? '<div class="quiet-status">Reading persisted state…</div>' : ""}${state.notice ? `<div class="notice success">${escape(state.notice)}</div>` : ""}${state.error ? `<div class="notice error" role="alert"><span>${escape(state.error)}</span>${button("refresh", "Refresh", { disabled: !state.connected || Boolean(state.pending) }).replace('id="action-refresh"', 'id="action-refresh-error"')}</div>` : ""}${state.contextError ? `<div class="notice">${escape(state.contextError)}</div>` : ""}</div>
      ${surface === "thread" ? '<div class="thread-inspector-label">This conversation</div>' : ""}${editor ? renderEditor(editor, state, run) : !state.initialized ? `<section class="empty welcome">${lunaMark(40)}<h1>Opening Luna Factory</h1><p>Waiting for the host's first snapshot.</p><p class="muted">Open Luna Factory from your MCP Apps host to connect.</p></section>` : state.selectedId ? run ? renderCampaign(run, state, options) : '<section class="empty"><h1>Opening this run</h1><p>The exact run is being read from the server.</p></section>' : renderHome(state, canStart, options)}
      <footer class="footer"><span>${state.refreshing ? "Reading current state" : "Status reads use no inference"}</span><span>Execution stays unavailable until independently qualified</span></footer>
    </main></div>`;
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

/** A tiny wave thumbnail: one dot per persisted task, by wave and state. */
function miniMap(run: RunView): string {
  const tasks = run.control?.tasks ?? [];
  if (tasks.length < 2) return "";
  const layout = layoutWaves(tasks);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const columns = Math.min(8, layout.waves.length);
  const rows = Math.min(6, Math.max(...layout.waves.map(wave => wave.length)));
  const width = Math.max(28, columns * 12 + 6);
  const height = rows * 9 + 6;
  const dots = layout.waves.slice(0, 8).flatMap((wave, x) => wave.slice(0, 6).map((id, y) => `<circle cx="${6 + x * 12}" cy="${6 + y * 9}" r="3" class="mini tone-${taskTone(byId.get(id)?.state ?? "")}"/>`)).join("");
  return `<svg class="mini-map" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" aria-hidden="true" focusable="false">${dots}</svg>`;
}

function renderHome(state: ViewState, canStart: boolean, options: WorkbenchOptions): string {
  const groups: Array<{ key: string; title: string; tiers: Tier[]; empty: string }> = [
    { key: "needs", title: "Needs you", tiers: ["needs"], empty: "Nothing needs your decision." },
    { key: "live", title: "In progress", tiers: ["working"], empty: "No owned work is in progress." },
    { key: "planned", title: "Planned · not started", tiers: ["planned"], empty: "" },
    { key: "history", title: "History", tiers: ["finished", "stopped", "unverified"], empty: "" },
  ];
  const counts = new Map(groups.map(group => [group.key, state.runs.filter(run => group.tiers.includes(runTier(run))).length]));
  const short: Record<string, [string, string]> = { needs: ["needs you", "need you"], live: ["in progress", "in progress"], planned: ["planned", "planned"], history: ["in history", "in history"] };
  const summary = groups.filter(group => counts.get(group.key)).map(group => { const count = counts.get(group.key) ?? 0; return `${count} ${short[group.key]![count === 1 ? 0 : 1]}`; }).join(" · ");
  const row = (run: RunView): string => {
    const tier = runTier(run);
    const changes = options.since?.changes(run);
    return `<li><a id="campaign-${escape(run.id)}" class="campaign-row tier-${tier}" href="${runPath(run.id)}" data-run-id="${escape(run.id)}">
      <span class="row-sky">${miniMap(run) || phaseGlyph(run.presentation?.criteria.proven ?? 0, run.presentation?.criteria.mandatory ?? 0, 22)}</span>
      <span class="row-main"><span class="row-repo">${escape(run.repository)}</span><strong>${escape(run.objective)}</strong><span class="row-now">${escape(tier === "needs" ? nowSentence(run) : changes?.lines[0] ?? nowSentence(run))}</span></span>
      <span class="row-side">${statusMark(tier, statusLabel(run))}${proofLine(run)}<span class="row-fresh">${escape(freshness(activityAt(run), options.now))}</span></span>
    </a></li>`;
  };
  const sections = groups.map(group => {
    const runs = state.runs.filter(run => group.tiers.includes(runTier(run)));
    if (!runs.length && !group.empty) return "";
    return `<section class="home-group group-${group.key}" aria-labelledby="group-${group.key}"><h2 id="group-${group.key}">${group.key === "needs" ? icon("alert", 14) : ""}${group.title}<span>${runs.length}</span></h2>${runs.length ? `<ul class="campaign-list">${runs.map(row).join("")}</ul>` : `<p class="group-empty">${group.empty}</p>`}</section>`;
  }).join("");
  return `<section class="home-head"><h1>Campaigns</h1><p>${escape(summary || "No runs yet")}</p><p class="home-hint">${icon("chat")}Ask ChatGPT to plan work with Luna Factory. Plans appear here before anything runs.</p></section>
    ${!state.capabilities?.repositories.length ? `<div class="notice repository-empty"><div><strong>No repositories configured</strong><p>Choose a local repository offered by your operator, then request access.</p></div>${button("repositories", "Add repository", { icon: "plus", disabled: !state.connected || Boolean(state.pending) })}</div>` : ""}
    ${state.runs.length ? sections : `<section class="empty empty-sky">${lunaMark(40)}<h2>No runs yet</h2><p>Ask ChatGPT to plan an objective with Luna Factory, or plan one manually.<br>Every run keeps its progress and evidence here.</p>${button("start", "Plan work manually", { primary: true, icon: "plus", disabled: !canStart }).replace('id="action-start"', 'id="action-start-empty"')}</section>`}`;
}

function renderCampaign(run: RunView, state: ViewState, options: WorkbenchOptions): string {
  const busy = !state.connected || Boolean(state.pending);
  const attention = needsOperatorDecision(run);
  const tier = runTier(run);
  const presentation = run.control ? run.presentation : undefined;
  const primary = presentation?.primary_action;
  const action = primary ?? { kind: "refresh" as const, label: "Refresh current state", reason: "This older response has no authoritative action projection.", tool: "refresh_factory", allowed: true };
  const secondary = presentation?.actions.filter(item => item.allowed && ["cancel", "steer"].includes(item.kind) && item.kind !== primary?.kind && item.tool) ?? [];
  const changes = options.since?.changes(run);
  const baseline = options.since?.baseline(run.id);
  // The typed category only names the blocker in plain language. Attention, tier and actions
  // never read it; an uncategorized blocker keeps the generic label.
  const category = presentation?.blocker_kind?.kind === "unknown" ? null : blockerCopy(run);
  const eyebrow = attention ? "Needs you" : run.planning_only ? "Plan navigation" : category ?? (run.blocker ? "Execution blocker" : "Next safe action");
  const headline = attention ? run.pending_decision?.question || action.label : run.planning_only ? action.label : run.blocker || action.label;
  return `<section class="campaign-head">
      <div class="eyebrow">${escape(run.repository)} · ${escape(finishLabels[run.finish])}</div>
      <h1>${escape(run.objective)}</h1>
      <p class="statusline">${statusMark(tier, statusLabel(run))}${proofLine(run)}<span class="fresh">${escape(freshness(activityAt(run), options.now))}</span>${run.created_at !== undefined ? `<span class="started"><time datetime="${new Date(run.created_at * 1000).toISOString()}">${run.planning_only ? "Created" : "Started"} ${escape(clock(run.created_at, options.now))}</time></span>` : ""}</p>
      <div class="story">${changes?.lines.length ? `<p class="story-kicker">Since you last looked at ${escape(clock(changes.since))}</p><ul>${changes.lines.map(line => `<li>${escape(line)}</li>`).join("")}</ul>` : `<p class="story-now">${escape(attention ? run.delta || "No new change was reported." : nowSentence(run))}</p>${baseline ? `<p class="story-kicker">No new server changes since ${escape(clock(baseline))}.</p>` : ""}`}${run.remaining_gap && tier !== "planned" ? `<p class="story-gap"><span>Still needed</span>${escape(run.remaining_gap)}</p>` : ""}</div>
    </section>
    <section class="decision-panel tier-${attention ? "needs" : tier}${attention ? " needs-you" : " compact"}" aria-labelledby="decision-title"><div class="eyebrow"${category && !attention && !run.planning_only ? ` data-blocker-kind="${escape(presentation?.blocker_kind?.kind ?? "")}"` : ""}>${escape(eyebrow)}</div><h2 id="decision-title">${escape(headline)}</h2><p id="action-reason">${escape(reasonCopy(presentation ? action.reason : null))}${presentation ? " The server rechecks this at the action boundary." : " Refresh to load the server's action and proof view."}</p><div class="run-controls">${actionButton(action, true, busy)}${secondary.length ? `<details id="other-actions" class="other-actions"><summary>Other available actions</summary><div class="run-controls">${secondary.map(item => actionButton(item, false, busy)).join("")}</div></details>` : ""}</div></section>
    ${renderProposalReview(state)}
    ${renderMission(state, run, task => renderFollowUpActions(run, options, task), options.canvasWidth)}
    ${renderEvidence(run, state)}`;
}

function renderEvidence(run: RunView, state: ViewState): string {
  const presentation = run.control ? run.presentation : undefined;
  const criteria = run.control?.criteria ?? run.acceptance.map((description, index) => ({ id: `A${index + 1}`, description, status: "unproved" as const, reason: null, check_refs: [] }));
  const { tasks, graph } = missionTasks(state, run);
  const agents = roster(run, graph);
  const receipts = run.receipts.map(receipt => `<li class="receipt"><time datetime="${new Date(receipt.created_at * 1000).toISOString()}">${escape(clock(receipt.created_at))}</time><div><strong>${escape(receipt.kind.replaceAll("_", " "))}</strong><span>${escape(receipt.summary)}</span>${receipt.thread_id ? `<small>Recorded on ${escape(receiptAgent(receipt, agents)?.label ?? "an unlisted agent")}'s thread</small>` : ""}<small class="${receipt.subject === run.current_subject ? "" : "warn"}">${receipt.subject === run.current_subject ? "Matches the current subject" : "Different subject; re-verification may be needed"}</small></div></li>`).join("");
  return `<details id="evidence-audit" class="evidence advanced"><summary>Evidence and audit<span>${criteria.length} criteria · ${run.receipts.length} receipts · ${tasks.length} tasks</span></summary>
    <section><h3>Criteria</h3><ol class="acceptance-list">${criteria.map(criterion => `<li><span class="criterion-state ${escape(criterion.status)}">${escape(criterion.status === "unproved" ? "pending" : criterion.status)}</span><div><p>${escape(criterion.description)}</p>${criterion.reason ? `<small class="muted">${escape(reasonCopy(criterion.reason))}</small>` : ""}${criterion.check_refs.length ? `<small class="muted">Checks: ${criterion.check_refs.map(escape).join(", ")}</small>` : ""}</div></li>`).join("")}</ol><p class="muted small">Child-task policy: ${escape(run.control?.child_policy ?? "unverified")}.</p></section>
    <section><h3>Execution and delivery</h3><div class="delivery-strip"><div><span>Owner</span><strong>${run.planning_only ? "No owner started" : escape(presentation?.owner.liveness ?? "unknown")}</strong><small>${run.planning_only ? "No native owner is assigned" : escape(presentation?.owner.thread_id ?? "Identity unknown")}${presentation?.owner.turn_id ? ` · turn ${escape(presentation.owner.turn_id)}` : ""}</small></div><div><span>Workers</span><strong>${presentation ? `${presentation.workers.filter(worker => worker.liveness === "active").length} active · ${presentation.workers.filter(worker => worker.liveness === "unknown").length} unknown` : "Unknown"}</strong><small>${presentation?.workers.map(worker => `${escape(worker.thread_id)}: ${escape(worker.liveness)}`).join(", ") || "No worker identities reported"}</small></div><div><span>Time left</span><strong>${run.planning_only ? "Not running" : presentation?.result.kind !== "working" ? "Inactive" : presentation?.budget.time_remaining_seconds === null || presentation?.budget.time_remaining_seconds === undefined ? "Unverified" : `${Math.ceil(presentation.budget.time_remaining_seconds / 60)} min`}</strong><small>${presentation?.budget.repair_attempts_remaining ?? "?"} repairs remaining · ${presentation?.budget.repairs_used ?? "?"} used</small></div><div><span>Repository claim</span><strong>${presentation?.claim.status ?? "unknown"}</strong><small>${presentation?.claim.held ? "Retained" : presentation ? "Released" : "Status unknown"}</small></div><div><span>Deliverable</span><strong>${run.planning_only ? "No deliverable yet" : presentation ? `${presentation.deliverable.status} · ${deliverableLabel(presentation.deliverable.kind)}` : "unverified"}</strong><small class="mono">${escape(presentation?.deliverable.subject ?? "Subject unknown")}${presentation?.deliverable.reference ? ` · ${escape(presentation.deliverable.reference)}` : ""}</small></div></div></section>
    <section><h3>Model route</h3><dl class="route-list"><dt>Requested</dt><dd>${escape(run.route.requested_model || "Not reported")}<small>${escape(run.route.requested_effort || "Effort not reported")}</small></dd><dt>Configured</dt><dd>${escape(run.route.configured_model || "Not reported")}<small>${escape(run.route.configured_effort || "Effort not reported")}</small></dd><dt>Observed</dt><dd>${escape(run.route.observed_model || "No execution-side model evidence")}<small>${escape(run.route.observed_effort || "Effort telemetry unavailable")}</small></dd><dt>Provider</dt><dd>${escape(run.route.configured_provider || "Configuration not reported")}<small>Downstream execution and billing unverified</small></dd></dl>${run.route.reroutes?.length ? `<div class="notice error">${run.route.reroutes.length} native model mismatch${run.route.reroutes.length === 1 ? "" : "es"} recorded. Execution was stopped for review. ${run.route.reroutes.map(route => `${escape(route.from_model)} → ${escape(route.to_model)} (${escape(route.thread_id)}, ${escape(route.turn_id)})`).join("; ")}</div>` : ""}</section>
    <section><h3>Receipts</h3>${receipts ? `<ol class="receipt-list">${receipts}</ol>` : '<p class="muted small">No execution receipts retained. A state label alone is not proof.</p>'}</section>
    <section><h3>Execution targets</h3>${renderTargets(state)}</section>
    ${run.non_goals.length ? `<section><h3>Outside this run</h3><ul>${run.non_goals.map(goal => `<li>${escape(goal)}</li>`).join("")}</ul></section>` : ""}
    <section><h3>Run identity</h3><dl class="identity-list"><dt>Run</dt><dd class="mono">${escape(run.id)}</dd><dt>Current subject</dt><dd class="mono">${escape(run.current_subject)}</dd><dt>Revision</dt><dd>${run.control?.revision ?? "unknown"}</dd><dt>Intent generation</dt><dd>${run.control?.intent_generation ?? "unknown"}</dd><dt>Dispatch generation</dt><dd>${run.control?.dispatch_generation ?? "unknown"}</dd><dt>Deadline</dt><dd>${run.planning_only ? "Not running" : new Date(run.deadline_at * 1000).toLocaleString()}</dd></dl></section>
  </details>`;
}

function renderInline(state: ViewState, run: RunView | undefined, options: WorkbenchOptions, preview: boolean): string {
  const notes = `${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic host data · No live integration</div>' : ""}${state.connectionStatus === "disconnected" && !state.error ? '<p class="connection-note" role="status">Disconnected · Reopen Luna Factory to refresh current state.</p>' : ""}${state.error ? `<p class="inline-feedback" role="alert">${escape(state.error)}</p>` : ""}${state.notice ? `<p class="inline-feedback" role="status">${escape(state.notice)}</p>` : ""}`;
  const shell = (body: string) => `<div class="workbench inline-surface" data-surface="inline" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">${notes}${body}</div>`;
  if (!run) return shell(`<article class="inline-card"><header class="inline-top">${lunaMark(18)}<span>Luna Factory</span></header><h2>No runs yet</h2><p class="inline-now">Ask ChatGPT to plan work with Luna Factory. Plans appear before anything runs.</p></article>`);
  const tier = runTier(run);
  const followKind: FollowUpKind = run.blocker || run.remaining_gap || run.pending_decision ? "blocker" : "summary";
  const followLabel = followKind === "blocker" ? (tier === "needs" ? "Ask ChatGPT about this decision" : "Ask ChatGPT about this") : "Summarize in chat";
  return shell(`<article class="inline-card tier-${tier}" aria-labelledby="inline-title">
    <header class="inline-top">${lunaMark(18)}<span>${escape(run.repository)}</span>${statusMark(tier, statusLabel(run))}<span class="inline-fresh">${escape(freshness(activityAt(run), options.now))}</span></header>
    <div class="inline-body"><span class="inline-sky">${miniMap(run) || phaseGlyph(run.presentation?.criteria.proven ?? 0, run.presentation?.criteria.mandatory ?? 0, 32)}</span><div><h2 id="inline-title">${escape(run.objective.slice(0, 300))}</h2><p class="inline-now">${escape(nowSentence(run))}</p><p class="inline-proof">${proofLine(run)}</p></div></div>
    ${tier === "needs" ? `<p class="inline-attention">${icon("alert", 14)}<span>${run.pending_decision ? "Answer in Luna Factory or in this chat." : "Answer this approval in native Codex."}</span></p>` : ""}
    <div class="inline-actions"><button id="follow-up-${followKind}-${escape(run.id)}" type="button" class="button quiet" data-action="chat-follow-up" data-kind="${followKind}"${disabled(!options.canSendFollowUps)}${options.messagePending ? ' aria-disabled="true"' : ""}>${icon("chat")}${escape(followLabel)}</button><button class="button primary" type="button" id="action-expand-mode" data-action="expand-mode"${disabled(!options.canExpand || !state.connected)}>Open Luna Factory</button></div>
    ${!options.hostCanMessage ? '<p class="inline-hint" role="status">ChatGPT message sending is unavailable in this host.</p>' : options.messagePending ? '<p class="inline-hint" role="status">Sending your message…</p>' : !options.canSendFollowUps ? '<p class="inline-hint" role="status">Available once the current read finishes.</p>' : ""}
  </article>`);
}

function renderFollowUpActions(run: RunView, options: WorkbenchOptions, task?: FollowUpTask): string {
  const actions: Array<{ kind: FollowUpKind; label: string }> = [{ kind: "summary", label: "Summarize this run" }];
  if (run.blocker || run.remaining_gap || run.pending_decision) actions.push({ kind: "blocker", label: "Ask about the blocker" });
  if (task || run.pending_decision) actions.push({ kind: "choose", label: task ? "Help me choose for this task" : "Help me choose a safe next step" });
  const messageUnavailable = !options.canSendFollowUps;
  const hostUnavailable = !options.hostCanMessage;
  const buttons = actions.map(action => `<button id="follow-up-${action.kind}-${escape(task?.id ?? run.id)}" type="button" class="ask" data-action="chat-follow-up" data-kind="${action.kind}"${task ? ` data-task-id="${escape(task.id)}"` : ""}${disabled(messageUnavailable)}${options.messagePending ? ' aria-disabled="true"' : ""}>${icon("chat", 14)}${escape(action.label)}</button>`).join("");
  return `<section class="chat-followups" aria-label="Ask ChatGPT about the selected Factory state"><h4>Ask ChatGPT</h4><div class="asks">${buttons}</div><p class="small muted">Sends this run${task ? " and task" : ""} at its current revision only after your click. Nothing starts.</p>${hostUnavailable ? '<p class="small muted" role="status">ChatGPT message sending is unavailable in this host.</p>' : messageUnavailable && !options.messagePending ? '<p class="small muted" role="status">Available once the current read finishes.</p>' : ""}${options.messagePending ? '<p class="small muted" role="status">Sending your message…</p>' : ""}</section>`;
}

const field = (name: string, label: string, control: string, hint = ""): string => `<div class="field"><label for="${name}">${label}</label>${control}${hint ? `<p class="field-hint" id="${name}-hint">${hint}</p>` : ""}</div>`;
function renderEditor(editor: Exclude<Editor, null>, state: ViewState, run: RunView | undefined): string {
  const title = { start: "Plan a bounded run", settings: "Factory settings", repositories: "Add a local repository", steer: run?.pending_decision ? "Answer the owner" : "Steer this run", stop: "Stop this run?" }[editor];
  const locked = !state.connected || Boolean(state.pending);
  const header = `<div class="editor-heading"><div><span class="eyebrow">${editor === "start" ? "A clear finish line" : "Luna Factory"}</span><h1>${title}</h1></div>${button("close-editor", "Close", { icon: "close", disabled: Boolean(state.pending) })}</div>`;
  if (editor === "repositories") return header + renderRepositories(state);
  if (editor === "steer" || editor === "stop") {
    if (!run) return `${header}<p>Select a run first.</p>`;
    if (editor === "stop") return `${header}<section class="editor-card"><p class="lead">${escape(run.objective)}</p><p>The server interrupts the owned execution and checks its descendants before releasing the repository claim. Unknown survivors leave the run blocked.</p><p>The objective, evidence, and history stay with this run. A later resume keeps its original authority and remaining limits.</p><form data-form="stop"><div class="form-actions"><button class="button danger" type="submit"${disabled(locked)}>Stop owned execution</button>${button("close-editor", "Keep running", { disabled: Boolean(state.pending) }).replace('id="action-close-editor"', 'id="action-keep-running"')}</div></form></section>`;
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
  return `${header}${repositories}<section class="editor-card"><p class="editor-intro">${editor === "start" ? "Usually ChatGPT drafts this for you. Use the form when you want to set the bounds yourself." : "Safe defaults for new runs. These do not change repository access, provider settings, or trusted limits."}</p><form data-form="${editor}">
    ${editor === "start" ? field("repository", "Approved repository", `<select id="repository" name="repository" required>${cap.repositories.map(repo => `<option value="${escape(repo.alias)}">${escape(repo.alias)}</option>`).join("")}</select>`) + field("objective", "Objective", '<textarea id="objective" name="objective" rows="3" required maxlength="8000" placeholder="What should be true when this run finishes?"></textarea>') + field("acceptance", "Mandatory acceptance", '<textarea id="acceptance" name="acceptance" rows="3" required maxlength="32032" placeholder="One verifiable criterion per line" aria-describedby="acceptance-hint"></textarea>', "One criterion per line, up to 32. Headings and bullet markers are omitted; every remaining line is mandatory.") + field("non_goals", "Non-goals", '<textarea id="non_goals" name="non_goals" rows="2" maxlength="32032" placeholder="What should this run leave alone?" aria-describedby="non_goals-hint"></textarea>', "Optional, one per line. Headings and bullet markers are omitted.") : ""}
    <div class="form-grid">${field("finish", "Finish authority", `<select id="finish" name="finish">${finishOptions}</select>`)}${field("profile", "Approved runtime profile", `<select id="profile" name="profile">${profileOptions}</select>`)}</div>
    ${field("capacity", "Worker capacity", `<input id="capacity" name="capacity" type="number" min="1" max="${cap.limits.capacity}" step="1" value="${Math.min(state.settings.capacity || 1, cap.limits.capacity)}" required aria-describedby="capacity-hint">`, `At most ${cap.limits.capacity} concurrent workers`)}
    ${editor === "start" ? `<details id="run-budgets" class="budget-details"><summary>Time &amp; repair budget</summary><div class="form-grid">${field("wall_seconds", "Time limit, seconds", `<input id="wall_seconds" name="wall_seconds" type="number" min="30" max="${cap.limits.wall_seconds}" step="1" value="${Math.min(1800, cap.limits.wall_seconds)}" required>`)}${field("repair_attempts", "Repair attempts", `<input id="repair_attempts" name="repair_attempts" type="number" min="0" max="${cap.limits.repair_attempts}" step="1" value="${Math.min(2, cap.limits.repair_attempts)}" required>`)}</div></details><div class="authority-note">${lunaMark(16)}<p>The server validates every action. No merge, release, or deployment is granted by this form.</p></div>` : ""}
    <div class="form-actions">${editor === "start" ? `<button class="button primary" type="submit" value="plan"${disabled(locked)}>Create planning graph</button><button class="button" type="submit" value="start"${disabled(locked || cap.execution?.eligible !== true)}>Start run</button><span>Planning is available. Execution requires independently qualified authentication, entitlement and adapter support.</span>` : `<button class="button primary" type="submit"${disabled(locked)}>Save defaults</button><span>Applies to future runs</span>`}</div></form></section>`;
}

function renderRepositories(state: ViewState): string {
  const busy = !state.connected || Boolean(state.pending) || state.discovering;
  const discovery = state.discovery;
  const candidate = discovery?.candidates[0];
  return `<section class="editor-card"><div class="section-heading"><h2>Operator-offered repositories</h2>${button("discover-repositories", "Refresh repositories", { icon: "refresh", disabled: busy })}</div><p class="editor-intro">Request a trusted alias for a local repository. A local operator must approve access before it can be used. Nothing is cloned and no run starts here.</p>
    ${state.discovering ? '<p role="status">Reading the local catalog…</p>' : !state.capabilities?.repository_onboarding?.enabled ? '<div class="notice">Your operator has not enabled local discovery. Ask them to configure a trusted discovery root on the server.</div>' : !discovery ? '<p class="muted">Refresh repositories to read the available catalog.</p>' : !candidate ? '<div class="notice">No new repositories found. Approved aliases are already available when planning. Ask your operator to add a local repository under a trusted root, then refresh.</div>' : `<div class="form-actions native-form-action"><button class="button" type="button" data-action="chat-repository-form"${disabled(busy)}>Choose with a ChatGPT form</button><span>When supported by this host; the accessible form remains available below.</span></div><form data-form="repositories">${field("candidate_id", "Local repository", `<select id="candidate_id" name="candidate_id" required>${discovery.candidates.map(item => `<option value="${escape(item.id)}">${escape(item.name)} · ${escape(item.root_alias)}</option>`).join("")}</select>`, "Only repositories discovered by the server are offered.")}${field("alias", "Trusted alias", '<input id="alias" name="alias" required maxlength="64" pattern="[A-Za-z0-9_-]{1,64}" placeholder="sandbox-test" aria-describedby="alias-hint">', "Use letters, digits, hyphens and underscores. Runs use this alias, never a filesystem path.")}${field("max_finish", "Maximum finish authority", `<select id="max_finish" name="max_finish" required>${allowedFinishes(candidate.max_finish).map(finish => `<option value="${finish}">${finishLabels[finish]}</option>`).join("")}</select>`, "The operator's root policy is the ceiling. Choose the smallest authority needed.")}<div class="form-actions"><button class="button primary" type="submit"${disabled(busy)}>Request access</button><span>Requires local operator approval</span></div></form>`}
    ${discovery?.requests.length ? `<section class="repository-requests"><h3>Access requests</h3>${discovery.requests.map(request => `<article class="repository-request"><div class="section-heading"><strong>${escape(request.alias)}</strong><span>${request.status === "approved" ? "Approval recorded" : "Pending operator approval"}</span></div><p>${escape(request.name)} · ${escape(request.root_alias)} · ${finishLabels[request.max_finish]}</p>${request.status === "pending" ? `<p class="muted">Give the local operator this request ID: <code>${escape(request.id)}</code>. After approval, refresh repositories.</p>` : '<p class="muted">Current approved aliases appear in Settings and when planning.</p>'}</article>`).join("")}</section>` : ""}
  </section>`;
}
