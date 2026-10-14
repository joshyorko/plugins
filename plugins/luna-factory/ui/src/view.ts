import { allowedFinishes, campaignStatusLabel, finishLabels, needsOperatorDecision, parentLabel, planDefaults, planFinishLabels, repositoryReasonCopy, runPath, type CampaignView, type FollowUpKind, type FollowUpTask, type RunView } from "./domain";
import type { ViewState } from "./controller";
import { renderMission, renderProposalReview, renderTargets, missionTasks } from "./graph-view";
import { layoutWaves } from "./campaign-map";
import { icon, lunaMark, phaseGlyph, taskTone } from "./lunar";
import { activityAt, blockerCopy, freshness, nowSentence, runTier, statusLabel, clock, type SinceTracker, type Tier } from "./narrative";
import { roster } from "./agents";
import { isRepoMenuOpen, openRepoMenu } from "./repo-menu";
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
    planning_only: "The saved plan is ready to review.", evidence_missing: "Evidence has not been collected.", owned_execution_active: "Owned execution is active.", read_only_status: "Reads current status without inference.",
    pending_owner_decision: "The completed owner decision needs an answer.", current_owned_turn: "Correction is fenced to the current turn.",
    stop_requires_native_cessation_proof: "Check current running work before continuing.", same_goal_remaining_budget: "Continue the same objective within its remaining limits.",
    effect_outcome_unknown: "Reconcile the existing effect before any new work.", owned_liveness_unknown: "Luna can't confirm the earlier run stopped.",
    time_budget_exhausted: "The original time limit is exhausted.", repair_budget_exhausted: "The original repair limit is exhausted.",
    diagnosis_required: "Two attempts made no progress; a diagnosis is required.", read_only_native_reconciliation: "Check current running work before continuing.",
    inspect_native_evidence: "Inspect retained native evidence.", native_approval_requires_native_ui: "This approval can only be answered in native Codex.",
  };
  // Unmapped codes are never shown raw; prose reasons from the server are already plain language.
  return messages[reason] ?? (/\s/.test(reason) ? `${reason.replace(/[.!?]?$/, ".")}` : "The server reported this action without further detail.");
};
const deliverableLabel = (kind: "local_candidate" | "push" | "pr_ready"): string => ({ local_candidate: "Local candidate", push: "Pushed branch", pr_ready: "PR ready" })[kind];
const disabled = (value: boolean): string => value ? " disabled" : "";
const button = (action: string, label: string, options: { primary?: boolean; disabled?: boolean; icon?: "plus" | "refresh" | "settings" | "close"; quiet?: boolean; iconOnly?: boolean } = {}): string => `<button type="button" id="action-${action}" data-action="${action}" class="button${options.primary ? " primary" : ""}${options.quiet ? " quiet" : ""}${options.iconOnly ? " icon-only" : ""}"${options.iconOnly ? ` aria-label="${escape(label)}" title="${escape(label)}"` : ""}${disabled(options.disabled ?? false)}>${options.icon ? icon(options.icon) : ""}${options.iconOnly ? "" : escape(label)}</button>`;
type PresentedAction = NonNullable<RunView["presentation"]>["actions"][number];
const actionButton = (action: PresentedAction, primary = false, busy = false): string => {
  const label = action.kind === "reconcile" ? "Check running work" : action.kind === "steer" ? "Reply to Luna" : action.label;
  return `<button type="button" class="button${primary ? " primary" : ""}" id="control-${escape(action.kind)}" data-action="control" data-kind="${escape(action.kind)}" data-tool="${escape(action.tool ?? "")}"${disabled(busy || !action.allowed || !action.tool)} aria-describedby="action-reason">${escape(label)}</button>`;
};
const statusMark = (tier: Tier, label: string): string => `<span class="status tier-${tier}"><span class="status-shape" aria-hidden="true"></span>${escape(label)}</span>`;
function proofLine(run: RunView): string {
  const counts = run.control ? run.presentation?.criteria : undefined;
  if (!counts) return `<span class="proof unverified">${phaseGlyph(0, 0)}Proof unverified</span>`;
  const progress = counts.failed ? `${counts.failed} need another look` : counts.proven === counts.mandatory ? "Done" : counts.proven ? `${counts.proven} of ${counts.mandatory} done` : "Not done yet";
  return `<span class="proof">${phaseGlyph(counts.proven, counts.mandatory)}${progress}</span>`;
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
  const disclosed = Array.from(root.querySelectorAll<HTMLElement>("[data-disclosure]:not([hidden])")).map(element => element.id);
  const repoMenuOpen = isRepoMenuOpen(root);
  const run = state.runs.find(item => item.id === state.selectedId);
  const surface = options.surface ?? "global";
  if (surface === "inline") {
    root.innerHTML = renderInline(state, run ?? state.runs[0], options, preview);
    if (focusId) findElementById(root, focusId)?.focus({ preventScroll: true });
    return;
  }
  const canStart = Boolean(state.connected && state.capabilities?.profiles.length && canAddOrPick(state) && !state.pending);
  if (options.canvasWidth === undefined && root.clientWidth > 0) {
    // Mirrors the CSS: main max 1360px with 24px padding; the inspector column leaves at 861px+.
    const main = Math.min(root.clientWidth, 1360) - (root.clientWidth <= 520 ? 24 : 48);
    options.canvasWidth = (main > 860 ? main - 340 - 16 : main) - 2;
  }
  root.innerHTML = `<div class="workbench" data-surface="${surface}" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">
    ${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic run data · No live integration</div>' : ""}
    <header class="appbar"><a class="brand" href="/" data-action="overview" aria-label="Luna Factory campaigns">${lunaMark(22)}<span>Luna Factory</span></a>${run && !editor ? `<nav class="breadcrumbs" aria-label="Breadcrumb">${icon("arrow", 12)}<a href="/" data-action="overview">Campaigns</a>${icon("arrow", 12)}<span>${escape(run.repository)}</span></nav>` : ""}
      <div class="top-actions"><span class="connection" role="status"><i class="${state.connected ? "online" : ""}" aria-hidden="true"></i>${state.connectionStatus === "connected" ? "Connected" : state.connectionStatus === "disconnected" ? "Disconnected" : "Connecting"}</span>${button("refresh", "Refresh", { icon: "refresh", quiet: true, iconOnly: true, disabled: !state.connected || state.refreshing || Boolean(state.pending) })}${button("settings", "Settings", { icon: "settings", quiet: true, iconOnly: true, disabled: !state.capabilities || Boolean(state.pending) })}${editor === "start" ? "" : button("start", "Plan work", { icon: "plus", quiet: true, disabled: !canStart })}</div></header>
    <main id="main" class="main">
      <div class="status-area" aria-live="polite" aria-atomic="true">${state.pending ? `<div class="notice pending"><span class="spinner" aria-hidden="true"></span>${state.pending.tool === "cancel_factory_run" ? "Stopping owned execution. Waiting for verified status…" : state.pending.tool === "connect_factory_github_repository" ? "Connecting the repo…" : state.pending.tool === "create_factory_local_repository" ? "Creating the repo…" : "Waiting for the server…"}</div>` : state.refreshing ? '<div class="quiet-status">Reading persisted state…</div>' : ""}${state.notice ? `<div class="notice success">${escape(state.notice)}</div>` : ""}${state.error ? `<div class="notice error" role="alert"><span>${escape(state.error)}</span>${button("refresh", "Refresh", { disabled: !state.connected || Boolean(state.pending) }).replace('id="action-refresh"', 'id="action-refresh-error"')}</div>` : ""}${state.contextError ? `<div class="notice">${escape(state.contextError)}</div>` : ""}</div>
      ${surface === "thread" ? '<div class="thread-inspector-label">This conversation</div>' : ""}${editor ? renderEditor(editor, state, run) : !state.initialized ? `<section class="empty welcome">${lunaMark(40)}<h1>Opening Luna Factory</h1><p>Waiting for the host's first snapshot.</p><p class="muted">Open Luna Factory from your MCP Apps host to connect.</p></section>` : state.selectedId ? run ? renderCampaign(run, state, options) : '<section class="empty"><h1>Opening this run</h1><p>The exact run is being read from the server.</p></section>' : renderHome(state, canStart, options)}
    </main></div>`;
  const newForm = root.querySelector<HTMLFormElement>("form");
  if (draft && newForm && oldKind === newForm.dataset.form && oldForm?.dataset.inputIdentity === newForm.dataset.inputIdentity) {
    for (const field of newForm.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input[name],textarea[name],select[name]")) {
      // Hidden fields, such as the plan form's picked repo, render from view state.
      if (field instanceof HTMLInputElement && field.type === "hidden") continue;
      const value = draft.get(field.name);
      if (field instanceof HTMLInputElement && field.type === "checkbox") field.checked = draft.getAll(field.name).includes(field.value);
      else if (field instanceof HTMLSelectElement && field.multiple) {
        const values = draft.getAll(field.name);
        for (const option of field.options) option.selected = values.includes(option.value);
      } else if (typeof value === "string") field.value = value;
    }
  }
  const repoFilter = root.querySelector<HTMLInputElement>("#repo_filter");
  if (repoFilter?.value) filterRepositories(root, repoFilter.value);
  const nextConfirmation = root.querySelector<HTMLInputElement>("#confirm-graph-change");
  if (confirmationIdentity && nextConfirmation?.dataset.confirmIdentity === confirmationIdentity) {
    nextConfirmation.checked = true;
    const apply = root.querySelector<HTMLButtonElement>('[data-action="apply-change"]');
    if (apply) apply.disabled = !state.connected || !!state.pending || state.graphStale || state.graphLoading;
  }
  for (const id of expanded) { const element = document.getElementById(id); if (element instanceof HTMLDetailsElement && root.contains(element)) element.open = true; }
  for (const id of disclosed) { const toggle = Array.from(root.querySelectorAll<HTMLElement>('[data-action="toggle-disclosure"]')).find(element => element.getAttribute("aria-controls") === id); if (toggle) toggleDisclosure(root, toggle, true); }
  if (repoMenuOpen) openRepoMenu(root, "none");
  if (focusId) {
    const element = findElementById(root, focusId);
    element?.focus({ preventScroll: true });
    if (element instanceof HTMLTextAreaElement && selection?.[0] !== undefined && selection[1] !== undefined) element.setSelectionRange(selection[0], selection[1]);
  }
}

/** Shows or hides the panel a plain toggle button controls, such as a task's editor. Display state only. */
export function toggleDisclosure(root: HTMLElement, toggle: HTMLElement, open?: boolean): void {
  const id = toggle.getAttribute("aria-controls");
  const panel = id ? findElementById(root, id) : undefined;
  if (!panel?.hasAttribute("data-disclosure")) return;
  const show = open ?? panel.hidden;
  panel.hidden = !show;
  toggle.setAttribute("aria-expanded", String(show));
  toggle.textContent = show ? "Close editor" : "Edit";
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
  const campaigns = state.campaigns ?? [];
  const linked = new Set(campaigns.flatMap(campaign => [campaign.planning_run_id, ...campaign.run_ids]));
  // Linked runs move under their campaign, except that a real decision is never hidden there.
  const members = (group: (typeof groups)[number]) => state.runs.filter(run => group.tiers.includes(runTier(run)) && (group.key === "needs" || !linked.has(run.id)));
  const counts = new Map(groups.map(group => [group.key, members(group).length]));
  const short: Record<string, [string, string]> = { needs: ["needs you", "need you"], live: ["in progress", "in progress"], planned: ["planned", "planned"], history: ["in history", "in history"] };
  const summary = [...(campaigns.length ? [`${campaigns.length} ${campaigns.length === 1 ? "campaign" : "campaigns"}`] : []), ...groups.filter(group => counts.get(group.key)).map(group => { const count = counts.get(group.key) ?? 0; return `${count} ${short[group.key]![count === 1 ? 0 : 1]}`; })].join(" · ");
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
    const runs = members(group);
    const section = !runs.length && !group.empty ? "" : `<section class="home-group group-${group.key}" aria-labelledby="group-${group.key}"><h2 id="group-${group.key}">${group.key === "needs" ? icon("alert", 14) : ""}${group.title}<span>${runs.length}</span></h2>${runs.length ? `<ul class="campaign-list">${runs.map(row).join("")}</ul>` : `<p class="group-empty">${group.empty}</p>`}</section>`;
    return group.key === "needs" ? section + renderCampaignGroup(state, campaigns, options) : section;
  }).join("");
  return `<section class="home-head"><h1>Campaigns</h1><p>${escape(summary || "No runs yet")}</p><p class="home-hint">${icon("chat")}Ask ChatGPT to plan work with Luna Factory. Plans appear here before anything runs.</p></section>
    ${!state.capabilities?.repositories.length ? `<div class="notice repository-empty"><div><strong>No repositories yet</strong><p>${canAddOrPick(state) ? "Pick one from GitHub or start a new one when you plan work." : "Ask your operator to add a repository for Luna."}</p></div>${canAddOrPick(state) ? button("start", "Pick a repo", { icon: "plus", disabled: !canStart }).replace('id="action-start"', 'id="action-start-repository"') : ""}</div>` : ""}
    ${state.runs.length || campaigns.length ? sections : `<section class="empty empty-sky">${lunaMark(40)}<h2>No runs yet</h2><p>Ask ChatGPT to plan an objective with Luna Factory, or plan one manually.<br>Every run keeps its progress and evidence here.</p>${button("start", "Plan work", { primary: true, icon: "plus", disabled: !canStart }).replace('id="action-start"', 'id="action-start-empty"')}</section>`}`;
}

const campaignTier = (status: CampaignView["status"]): Tier => ({ planned: "planned", promoted: "unverified", finished: "finished", stopped: "stopped" } satisfies Record<CampaignView["status"], Tier>)[status];
const plannedTasks = (count: number): string => count === 1 ? "1 planned task" : `${count} planned tasks`;
/** Campaign row, then its planning plan, then any linked runs. Every value is a named campaign or run field. */
function renderCampaignGroup(state: ViewState, campaigns: CampaignView[], options: WorkbenchOptions): string {
  if (!campaigns.length && !state.campaignsSkipped) return "";
  const child = (id: string, kind: string, run: RunView | undefined, fallback: string): string => {
    const tier = run ? runTier(run) : "unverified";
    return `<li><a id="campaign-child-${escape(id)}" class="campaign-child tier-${tier}" href="${runPath(id)}" data-run-id="${escape(id)}"><span class="child-kind">${escape(kind)}</span><span class="child-main"><strong>${escape(run ? run.objective : fallback)}</strong>${run ? `<span class="row-now">${escape(nowSentence(run))}</span>` : ""}</span><span class="row-side">${run ? `${statusMark(tier, statusLabel(run))}${proofLine(run)}` : '<span class="row-fresh">Open to read its current state</span>'}</span></a></li>`;
  };
  const items = campaigns.map(campaign => {
    const plan = state.runs.find(run => run.id === campaign.planning_run_id);
    const tier = campaignTier(campaign.status);
    const execution = campaign.run_ids.length ? `${campaign.run_ids.length} linked ${campaign.run_ids.length === 1 ? "run" : "runs"}` : campaign.status === "planned" ? "No runs yet" : "No linked runs reported";
    const runs = campaign.run_ids.map(id => child(id, "Run", state.runs.find(run => run.id === id), "Linked run")).join("");
    return `<li class="campaign-entity"><a id="campaign-entity-${escape(campaign.id)}" class="campaign-row campaign-parent-row tier-${tier}" href="${runPath(campaign.planning_run_id)}" data-run-id="${escape(campaign.planning_run_id)}">
      <span class="row-sky">${plan ? miniMap(plan) || phaseGlyph(0, 0, 22) : phaseGlyph(0, 0, 22)}</span>
      <span class="row-main"><span class="row-repo">${escape(campaign.repository.alias)} · ${escape(parentLabel(campaign))}</span><strong>${escape(campaign.title)}</strong><span class="row-now">${escape(`${plannedTasks(campaign.planning.tasks.total)} · ${execution}.`)}</span></span>
      <span class="row-side">${statusMark(tier, campaignStatusLabel(campaign.status))}<span class="row-fresh">${escape(freshness(campaign.updated_at, options.now))}</span></span>
    </a><ul class="campaign-children" aria-label="Plan and runs">${child(campaign.planning_run_id, "Plan", plan, `Planning graph · ${plannedTasks(campaign.planning.tasks.total)}`)}${runs || '<li class="campaign-child-empty">No execution runs yet</li>'}</ul></li>`;
  }).join("");
  return `<section class="home-group group-campaigns" aria-labelledby="group-campaigns"><h2 id="group-campaigns">Parent campaigns<span>${campaigns.length}</span></h2>${state.campaignsStale ? '<p class="group-empty" role="status">Campaign grouping could not be refreshed. Showing the last valid read.</p>' : ""}${state.campaignsSkipped ? `<p class="group-empty" role="status">${state.campaignsSkipped === 1 ? "1 campaign record could not be read and is not shown." : `${state.campaignsSkipped} campaign records could not be read and are not shown.`}</p>` : ""}${items ? `<ul class="campaign-list">${items}</ul>` : ""}</section>`;
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
  // The typed category only names the blocker in plain language. Attention, tier and actions
  // never read it; an uncategorized blocker keeps the generic label.
  const category = presentation?.blocker_kind?.kind === "unknown" ? null : blockerCopy(run);
  const eyebrow = attention ? "Needs you" : run.planning_only ? "Plan" : category ?? (run.blocker ? "Execution blocker" : "Next safe action");
  const headline = attention ? run.pending_decision?.question || action.label : run.planning_only ? "" : presentation?.blocker_kind?.kind === "liveness_unknown" ? "Luna can't confirm the earlier run stopped." : run.blocker || action.label;
  const campaign = state.campaigns?.find(item => item.planning_run_id === run.id || item.run_ids.includes(run.id));
  return `<section class="campaign-head">
      <div class="eyebrow">${escape(run.repository)}</div>
      <h1>${escape(run.objective)}</h1>
      ${campaign ? `<p class="campaign-parent">from ${campaign.parent.display?.url ? `<a class="source-link" href="${escape(campaign.parent.display.url)}" target="_blank" rel="noopener noreferrer">#${campaign.parent.forge.number}</a>` : `#${campaign.parent.forge.number}`}</p>` : ""}
      <p class="statusline">${statusMark(tier, statusLabel(run))}${run.planning_only ? "" : proofLine(run)}<span class="fresh">${escape(freshness(activityAt(run), options.now))}</span>${run.created_at !== undefined ? `<span class="started"><time datetime="${new Date(run.created_at * 1000).toISOString()}">${run.planning_only ? "Created" : "Started"} ${escape(clock(run.created_at, options.now))}</time></span>` : ""}</p>
      <div class="story">${changes?.lines.length ? `<p class="story-kicker">Since you last looked at ${escape(clock(changes.since))}</p><ul>${changes.lines.map(line => `<li>${escape(line)}</li>`).join("")}</ul>` : `<p class="story-now">${escape(attention ? run.delta || "No new change was reported." : nowSentence(run))}</p>`}${run.remaining_gap && tier !== "planned" ? `<p class="story-gap">${escape(run.remaining_gap === "Owner acceptance of the current subject is still required." ? "Waiting for you to accept the result." : `Still needed: ${run.remaining_gap}`)}</p>` : ""}</div>
    </section>
    ${run.planning_only && !attention ? "" : `<section class="decision-panel tier-${attention ? "needs" : tier}${attention ? " needs-you" : " compact"}" aria-labelledby="decision-title"><div class="eyebrow"${category && !attention && !run.planning_only ? ` data-blocker-kind="${escape(presentation?.blocker_kind?.kind ?? "")}"` : ""}>${escape(eyebrow)}</div><h2 id="decision-title">${escape(headline)}</h2><p id="action-reason">${escape(reasonCopy(presentation ? action.reason : null))}${presentation ? "" : " Refresh to load the server's action and proof view."}</p><div class="run-controls">${actionButton(action, !run.planning_only || attention, busy)}${secondary.map(item => actionButton(item, false, busy)).join("")}</div></section>`}
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
  return `<details id="evidence-audit" class="evidence advanced"><summary>Evidence and audit<span>${criteria.length} ${criteria.length === 1 ? "completion check" : "completion checks"} · ${run.receipts.length} ${run.receipts.length === 1 ? "record" : "records"} · ${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}</span></summary>
    <section><h3>Completion checks</h3><ol class="acceptance-list">${criteria.map(criterion => `<li><span class="criterion-state ${escape(criterion.status)}">${escape(criterion.status === "unproved" ? "Not done yet" : criterion.status === "proven" ? "Done" : "Needs attention")}</span><div><p>${escape(criterion.description)}</p>${criterion.reason ? `<small class="muted">${escape(reasonCopy(criterion.reason))}</small>` : ""}${criterion.check_refs.length ? `<small class="muted">Checks: ${criterion.check_refs.map(escape).join(", ")}</small>` : ""}</div></li>`).join("")}</ol><p class="muted small">Child-task policy: ${escape(run.control?.child_policy ?? "unverified")}.</p></section>
    <section><h3>Execution and delivery</h3><div class="delivery-strip"><div><span>Owner</span><strong>${run.planning_only ? "No owner started" : escape(presentation?.owner.liveness ?? "unknown")}</strong><small>${run.planning_only ? "No native owner is assigned" : escape(agents.find(agent => agent.thread === presentation?.owner.thread_id)?.label ?? (presentation?.owner.thread_id ? "Owner thread recorded" : "Identity unknown"))}</small></div><div><span>Workers</span><strong>${presentation ? `${presentation.workers.filter(worker => worker.liveness === "active").length} active · ${presentation.workers.filter(worker => worker.liveness === "unknown").length} unknown` : "Unknown"}</strong><small>${presentation?.workers.map(worker => `${escape(agents.find(agent => agent.thread === worker.thread_id)?.label ?? "Unlisted worker")}: ${escape(worker.liveness)}`).join(", ") || "No worker identities reported"}</small></div><div><span>Time left</span><strong>${run.planning_only ? "Not running" : presentation?.result.kind !== "working" ? "Inactive" : presentation?.budget.time_remaining_seconds === null || presentation?.budget.time_remaining_seconds === undefined ? "Unverified" : `${Math.ceil(presentation.budget.time_remaining_seconds / 60)} min`}</strong><small>${presentation?.budget.repair_attempts_remaining ?? "?"} repairs remaining · ${presentation?.budget.repairs_used ?? "?"} used</small></div><div><span>Repository claim</span><strong>${presentation?.claim.status ?? "unknown"}</strong><small>${presentation?.claim.held ? "Retained" : presentation ? "Released" : "Status unknown"}</small></div><div><span>Deliverable</span><strong>${run.planning_only ? "No deliverable yet" : presentation ? `${presentation.deliverable.status} · ${deliverableLabel(presentation.deliverable.kind)}` : "unverified"}</strong><small class="mono">${escape(presentation?.deliverable.subject ?? "Subject unknown")}${presentation?.deliverable.reference ? ` · ${escape(presentation.deliverable.reference)}` : ""}</small></div></div></section>
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

/** One plain "Ask ChatGPT about this" action. It only sends after a click and never starts work. */
function renderFollowUpActions(run: RunView, options: WorkbenchOptions, task?: FollowUpTask): string {
  const kind: FollowUpKind = task ? "choose" : run.blocker || run.remaining_gap || run.pending_decision ? "blocker" : "summary";
  const note = !options.hostCanMessage ? "ChatGPT message sending is unavailable in this host." : options.messagePending ? "Sending your message…" : !options.canSendFollowUps ? "Available once the current read finishes." : "Sends only after your click. Nothing starts.";
  return `<div class="chat-followups"><button id="follow-up-${kind}-${escape(task?.id ?? run.id)}" type="button" class="ask" data-action="chat-follow-up" data-kind="${kind}"${task ? ` data-task-id="${escape(task.id)}"` : ""}${disabled(!options.canSendFollowUps)}${options.messagePending ? ' aria-disabled="true"' : ""} aria-describedby="follow-up-note">${icon("chat", 14)}Ask ChatGPT about this</button><p class="small muted" id="follow-up-note" role="status">${escape(note)}</p></div>`;
}

const field = (name: string, label: string, control: string, hint = ""): string => `<div class="field"><label for="${name}">${label}</label>${control}${hint ? `<p class="field-hint" id="${name}-hint">${hint}</p>` : ""}</div>`;
function renderEditor(editor: Exclude<Editor, null>, state: ViewState, run: RunView | undefined): string {
  const title = { start: "Plan work", settings: "Factory settings", repositories: "Add from an operator folder", steer: run?.pending_decision ? "Answer the owner" : "Steer this run", stop: "Stop this run?" }[editor];
  const locked = !state.connected || Boolean(state.pending);
  const header = `<div class="editor-heading"><div>${editor === "start" ? "" : '<span class="eyebrow">Luna Factory</span>'}<h1>${title}</h1></div>${button("close-editor", "Close", { icon: "close", disabled: Boolean(state.pending) })}</div>`;
  if (editor === "repositories") return header + renderRepositories(state);
  if (editor === "steer" || editor === "stop") {
    if (!run) return `${header}<p>Select a run first.</p>`;
    if (editor === "stop") return `${header}<section class="editor-card"><p class="lead">${escape(run.objective)}</p><p>The server interrupts the owned execution and checks its descendants before releasing the repository claim. Unknown survivors leave the run blocked.</p><p>The objective, evidence, and history stay with this run. A later resume keeps its original authority and remaining limits.</p><form data-form="stop"><div class="form-actions"><button class="button danger" type="submit"${disabled(locked)}>Stop owned execution</button>${button("close-editor", "Keep running", { disabled: Boolean(state.pending) }).replace('id="action-close-editor"', 'id="action-keep-running"')}</div></form></section>`;
    return `${header}<section class="editor-card">${run.pending_decision || run.blocker ? `<div class="decision-quote">${escape(run.pending_decision?.question || run.blocker || "")}</div>` : `<p>Send a correction to the current owner of <strong>${escape(run.repository)}</strong>.</p>`}<form data-form="steer" data-input-identity="${escape(run.pending_decision?.id || run.turn_id || run.id)}">${field("message", "Your answer or correction", '<textarea id="message" name="message" rows="6" required maxlength="4000" placeholder="What should the owner do differently?" aria-describedby="message-hint"></textarea>', run.pending_decision ? "Continues the same run and owner thread with your answer. Existing limits stay in place. Security approvals must be handled in native Codex." : "Sent to the current turn only. This cannot expand permissions or approve a security prompt.")}<div class="form-actions"><button class="button primary" type="submit"${disabled(locked || !run.pending_decision && !run.turn_id)}>Send to owner</button><span>Existing scope and finish authority stay in place</span></div></form></section>`;
  }
  if (editor === "start") return `<section class="plan-composer">${header}${renderPlan(state)}</section>`;
  const cap = state.capabilities;
  const repositories = `<section class="editor-card"><div class="section-heading"><h2>Repositories</h2></div>${cap?.repositories.length ? `<ul class="repository-list">${cap.repositories.map(repo => `<li><strong>${escape(repo.alias)}</strong><span>${finishLabels[repo.max_finish]}</span></li>`).join("")}</ul>` : '<p class="muted">No repositories yet. Add one when you plan work.</p>'}</section>`;
  if (!cap || !cap.profiles.length) return `${header}${renderRunningSetup(state)}${repositories}<div class="notice">No approved runtime profile is available. Check the trusted server configuration.</div>`;
  const profileOptions = cap.profiles.map(profile => `<option value="${escape(profile.alias)}"${profile.alias === state.settings.profile ? " selected" : ""}>${escape(profile.alias)} · ${escape(profile.effort)}</option>`).join("");
  const finishOptions = allowedFinishes("pr").map(finish => `<option value="${finish}"${finish === state.settings.finish ? " selected" : ""}>${finishLabels[finish]}</option>`).join("");
  return `${header}${renderRunningSetup(state)}${repositories}<section class="editor-card"><p class="editor-intro">Safe defaults for new runs. These do not change repository access, provider settings, or trusted limits.</p><form data-form="settings">
    <div class="form-grid">${field("finish", "Finish authority", `<select id="finish" name="finish">${finishOptions}</select>`)}${field("profile", "Approved runtime profile", `<select id="profile" name="profile">${profileOptions}</select>`)}</div>
    ${field("capacity", "Worker capacity", `<input id="capacity" name="capacity" type="number" min="1" max="${cap.limits.capacity}" step="1" value="${Math.min(state.settings.capacity || 1, cap.limits.capacity)}" required aria-describedby="capacity-hint">`, `At most ${cap.limits.capacity} concurrent workers`)}
    <div class="form-actions"><button class="button primary" type="submit"${disabled(locked)}>Save defaults</button><span>Applies to future runs</span></div></form></section>`;
}

/** Whether the plan form has a repository to pick, or the server offers a way to add one. */
function canAddOrPick(state: ViewState): boolean {
  const cap = state.capabilities;
  return Boolean(cap?.repositories.length || cap?.workspaces?.connect_github || cap?.workspaces?.create_local || cap?.repository_onboarding?.enabled);
}

/** Hide picker rows that don't match the filter text. View-only; nothing is read or sent. */
export function filterRepositories(root: ParentNode, query: string): void {
  const needle = query.trim().toLowerCase();
  for (const row of root.querySelectorAll<HTMLElement>(".repo-item[data-repo-name]")) row.hidden = Boolean(needle) && !(row.dataset.repoName ?? "").includes(needle);
}

const provenanceCopy = { github_app_installation: "Connected from GitHub", local_create: "Created in Luna" } as const;
const itemId = (prefix: string, name: string): string => `${prefix}-${name.toLowerCase().replace(/[^a-z0-9_-]/g, "-")}`;
/**
 * One composer: the request box, then one row with the repo chip and Plan it. The chip opens a
 * menu of your repos, GitHub repos to connect, and "+ New repo…" (created when Plan it is pressed).
 * Every other setting sits in a closed "More options" with the defaults filled in. The server
 * rechecks all of it on submit.
 */
function renderPlan(state: ViewState): string {
  const cap = state.capabilities;
  if (!cap || !cap.profiles.length) return '<section class="plan-card"><p class="plan-note">Luna isn\'t ready to plan yet. Ask your operator to finish setting it up.</p></section>';
  const locked = !state.connected || Boolean(state.pending);
  const defaults = planDefaults(cap);
  const approved = cap.repositories;
  const creating = state.newRepository && cap.workspaces?.create_local === true;
  const picked = creating ? undefined : approved.find(repo => repo.alias === state.pickedRepository) ?? approved[0];
  const github = cap.workspaces?.connect_github ? state.githubRepos : null;
  const githubFor = new Map((github?.repositories ?? []).filter(repo => repo.connected_alias).map(repo => [repo.connected_alias, repo]));
  const text = (name: string, meta: string): string => `<span class="repo-text"><span class="repo-name">${escape(name)}</span>${meta ? `<span class="repo-meta">${escape(meta)}</span>` : ""}</span>`;
  const approvedItems = approved.map(repo => {
    const source = githubFor.get(repo.alias);
    const meta = source ? source.full_name : repo.provenance ? provenanceCopy[repo.provenance] : "";
    const checked = repo === picked;
    return `<button type="button" role="menuitemradio" aria-checked="${checked}" tabindex="-1" class="repo-item" id="${itemId("repo-item", repo.alias)}" data-repo-action="pick" data-alias="${escape(repo.alias)}" data-repo-name="${escape(`${repo.alias} ${source?.full_name ?? ""}`.toLowerCase())}"><span class="repo-check" aria-hidden="true">${checked ? icon("check", 14) : ""}</span>${text(repo.alias, meta)}</button>`;
  }).join("");
  // Already-connected repos appear above as approved aliases; the rest can be connected here.
  const approvedAliases = new Set(approved.map(repo => repo.alias));
  const githubItems = (github?.available ? github.repositories : []).filter(repo => !repo.connected_alias || !approvedAliases.has(repo.connected_alias))
    .sort((a, b) => Number(b.connectable && !b.connected_alias) - Number(a.connectable && !a.connected_alias))
    .map(repo => {
      const meta = [repo.private ? "Private" : "", repo.description ?? ""].filter(Boolean).join(" · ");
      const attrs = `type="button" tabindex="-1" id="${itemId("github-item", repo.full_name)}" data-github-repo="${escape(repo.full_name)}" data-repo-name="${escape(repo.full_name.toLowerCase())}"`;
      if (repo.connectable && !repo.connected_alias) return `<button ${attrs} role="menuitem" class="repo-item github" data-repo-action="connect" data-repository="${escape(repo.full_name)}" aria-label="Connect ${escape(repo.full_name)}"><span class="repo-check" aria-hidden="true"></span>${text(repo.full_name, meta)}<span class="repo-connect" aria-hidden="true">Connect</span></button>`;
      const reason = repo.connected_alias ? "Connected · refresh to use" : "Not available";
      return `<button ${attrs} role="menuitem" class="repo-item github muted-row" aria-disabled="true"${repo.connected_alias ? "" : ` title="${escape(repositoryReasonCopy(repo.blocked_reason))}"`}><span class="repo-check" aria-hidden="true"></span>${text(repo.full_name, meta)}<span class="repo-state">${reason}</span></button>`;
    }).join("");
  const more = github?.next_cursor ? `<button type="button" role="menuitem" tabindex="-1" class="repo-item repo-more" id="action-more-github" data-repo-action="more"${state.githubLoading ? ' aria-disabled="true"' : ""}><span class="repo-check" aria-hidden="true"></span><span class="repo-text"><span class="repo-name">Show more from GitHub</span></span></button>` : "";
  const githubNote = cap.workspaces?.connect_github && !state.githubLoading && github && !github.available ? `<p class="repo-note">${escape(github.configured ? "GitHub repos couldn't be read right now." : repositoryReasonCopy("github_app_not_configured"))}</p>` : "";
  const loading = cap.workspaces?.connect_github && state.githubLoading && !github ? '<p class="repo-note" role="status">Reading your GitHub repos…</p>' : "";
  const canAdd = cap.workspaces?.connect_github === true || cap.workspaces?.create_local === true;
  const newItem = cap.workspaces?.create_local ? `<div role="separator" class="repo-sep"></div><button type="button" role="menuitem" tabindex="-1" class="repo-item repo-new" id="repo-item-new" data-repo-action="new"><span class="repo-check" aria-hidden="true">${icon("plus", 14)}</span><span class="repo-text"><span class="repo-name">New repo…</span></span></button>` : "";
  const rows = approved.length + (github?.available ? github.repositories.length : 0);
  const filter = rows > 6 ? '<input type="search" id="repo_filter" class="repo-filter" placeholder="Find a repo" aria-label="Find a repo" aria-controls="repo-menu" autocomplete="off">' : "";
  const emptyNote = !approved.length && canAdd ? '<p class="repo-note">Connect a repo or start a new one to plan here.</p>' : "";
  const chipLabel = creating ? "New repo" : picked?.alias ?? (cap.workspaces?.connect_github ? "Connect GitHub" : canAdd ? "Choose a repo" : "No repos yet");
  const chip = `<div class="repo-select"><button type="button" id="repo-chip" class="repo-chip" aria-haspopup="menu" aria-expanded="false" aria-controls="repo-menu"${disabled(locked || !approved.length && !canAdd)}><span class="sr-only">Repo: </span><span class="repo-chip-label">${escape(chipLabel)}</span>${icon("chevron", 14)}</button>
      <div id="repo-popover" class="repo-popover" hidden>${filter}<div id="repo-menu" role="menu" aria-label="Choose a repo">${approvedItems ? `<div role="group" aria-label="Your repos" class="repo-group"><p class="repo-group-label" aria-hidden="true">Your repos</p>${approvedItems}</div>` : ""}${githubItems || more ? `<div role="group" aria-label="On GitHub" class="repo-group"><p class="repo-group-label" aria-hidden="true">On GitHub</p>${githubItems}${more}</div>` : ""}${newItem}</div>${loading}${githubNote}${emptyNote}</div></div>`;
  const nameField = creating ? `<div class="repo-name-field"><label class="sr-only" for="new_repository">New repo name</label><input id="new_repository" name="new_repository" autocomplete="off" autocapitalize="off" spellcheck="false" maxlength="64" placeholder="new-repo-name" aria-describedby="plan-repo-error"${disabled(locked)}></div>` : "";
  const eligible = cap.execution?.eligible === true;
  const finishes = allowedFinishes(creating ? "local_candidate" : picked?.max_finish ?? "local_candidate");
  const profileOptions = cap.profiles.map((profile, index) => `<option value="${escape(profile.alias)}"${index === 0 ? " selected" : ""}>${escape(profile.alias)} · ${escape(profile.effort)}</option>`).join("");
  const maxMinutes = Math.max(1, Math.floor(cap.limits.wall_seconds / 60));
  const options = `<details id="plan-more" class="plan-more"><summary>More options</summary><div class="plan-more-body">
      ${field("done_when", "Done when", '<input id="done_when" name="done_when" maxlength="1000" autocomplete="off" placeholder="The tests pass and the README explains it" aria-describedby="done_when-hint">', "Optional. Leave it empty and your request is the finish line.")}
      ${field("non_goals", "Leave alone", '<textarea id="non_goals" name="non_goals" rows="2" maxlength="32032" placeholder="One thing per line" aria-describedby="non_goals-hint"></textarea>', "Optional, one per line.")}
      <div class="form-grid">${field("finish", "When it's done", `<select id="finish" name="finish">${finishes.map(finish => `<option value="${finish}"${finish === defaults.finish ? " selected" : ""}>${planFinishLabels[finish]}</option>`).join("")}</select>`)}${field("profile", "Model", `<select id="profile" name="profile">${profileOptions}</select>`)}</div>
      <div class="form-grid three">${field("capacity", "Workers at once", `<input id="capacity" name="capacity" type="number" min="1" max="${cap.limits.capacity}" step="1" value="${defaults.capacity}" required>`)}${field("wall_minutes", "Time limit, minutes", `<input id="wall_minutes" name="wall_minutes" type="number" min="1" max="${maxMinutes}" step="1" value="${Math.max(1, Math.min(maxMinutes, Math.round(defaults.wall_seconds / 60)))}" required>`)}${field("repair_attempts", "Retries", `<input id="repair_attempts" name="repair_attempts" type="number" min="0" max="${cap.limits.repair_attempts}" step="1" value="${defaults.repair_attempts}" required>`)}</div>
      ${cap.repository_onboarding?.enabled ? `<div class="plan-more-row"><span>Need a repo from your operator's folders?</span>${button("repositories", "Add from an operator folder", { quiet: true, disabled: locked })}</div>` : ""}
      <p class="plan-fine">Luna checks every step on the server. Planning never merges, releases or deploys anything.</p>
    </div></details>`;
  const note = !approved.length && !canAdd
    ? (cap.repository_onboarding?.enabled ? "No repositories yet. Add one under More options." : "No repositories yet. Ask your operator to add one.")
    : eligible ? "Luna drafts a plan first. Nothing runs until you press Start run." : "";
  const noteHtml = note ? escape(note) : 'Luna drafts a plan first. Running it needs a <button type="button" class="text-link" id="action-setup" data-action="setup">one-time setup</button>.';
  return `<section class="plan-card"><form data-form="start" class="plan-form">
    <div class="plan-box"><label for="objective">What do you want done?</label>
      <div class="composer"><textarea id="objective" name="objective" rows="4" required maxlength="8000" placeholder="Fix the flaky login test. Or: start a tiny CLI that prints tonight's moon phase."></textarea>
        <input type="hidden" name="repository" value="${escape(picked?.alias ?? "")}">
        <div class="plan-controls">${chip}${nameField}<span class="plan-spacer"></span>${eligible ? `<button class="button plan-run" type="submit" value="start"${disabled(locked || !approved.length && !canAdd)}>Start run</button>` : ""}<button class="button primary plan-go" type="submit" value="plan"${disabled(locked || !approved.length && !canAdd)}>Plan it</button></div>
      </div>
      <p class="plan-error" id="plan-repo-error" role="alert" hidden></p>
      <p class="plan-note">${noteHtml}</p>
    </div>
    ${options}
  </form></section>`;
}

/** The setup guidance the plan form's "one-time setup" link opens. Read-only: nothing here turns running on. */
function renderRunningSetup(state: ViewState): string {
  const eligible = state.capabilities?.execution?.eligible === true;
  return `<section class="editor-card setup-card" aria-labelledby="setup-running"><div class="section-heading"><h2 id="setup-running" tabindex="-1">Running work</h2><span class="setup-state">${eligible ? "Set up" : "Not set up yet"}</span></div>${eligible
    ? "<p>Running is set up. Start run appears next to Plan it.</p>"
    : "<p>Luna can plan now. Running a plan needs a one-time check on the machine Luna runs on: Codex is signed in, your plan includes Codex, and Luna's connection to Codex passes its checks.</p><p class=\"muted\">Your Luna server turns on Start run once that check passes; the steps are in the Luna Factory README. Until then you can plan as much as you like, and nothing runs.</p>"}</section>`;
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
