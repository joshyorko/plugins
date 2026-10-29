import { allowedFinishes, finishLabels, needsOperatorDecision, planDefaults, planFinishLabels, preserveCanonicalTaskTitle, repositoryReasonCopy, runPath, type CampaignView, type FollowUpKind, type FollowUpTask, type RunView } from "./domain";
import type { ViewState } from "./controller";
import { renderMission, renderProposalReview } from "./graph-view";
import { layoutWaves } from "./campaign-map";
import { icon, lunaMark, phaseGlyph, taskGlyph, taskTone } from "./lunar";
import { activityAt, blockerCopy, freshness, nowSentence, runTier, statusLabel, clock, type SinceTracker, type Tier } from "./narrative";
import { isRepoMenuOpen, openRepoMenu } from "./repo-menu";
import { plainSummary } from "./lanes";

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
  if (!reason) return "Open Details for more information.";
  const messages: Record<string, string> = {
    planning_only: "Your plan is ready to review.", evidence_missing: "Not checked yet.", owned_execution_active: "This run is still active.", read_only_status: "Luna is checking the latest status.",
    pending_owner_decision: "A decision from you is needed.", current_owned_turn: "Reply in this conversation to continue.",
    stop_requires_native_cessation_proof: "Check running work before continuing.", same_goal_remaining_budget: "Continue this plan within its remaining limits.",
    effect_outcome_unknown: "Luna can't confirm whether the change happened.", owned_liveness_unknown: "Before starting new work, Luna needs to check what's still running in Codex.",
    time_budget_exhausted: "The time limit has been reached.", repair_budget_exhausted: "The retry limit has been reached.",
    diagnosis_required: "Luna needs a diagnosis before another retry.", read_only_native_reconciliation: "Check running work before continuing.",
    inspect_native_evidence: "Open Details for the saved information.", native_approval_requires_native_ui: "Answer this approval in Codex.",
  };
  return messages[reason] ?? "Luna couldn't confirm this yet. Refresh to check again.";
};
const disabled = (value: boolean): string => value ? " disabled" : "";
const button = (action: string, label: string, options: { primary?: boolean; pagePrimary?: boolean; disabled?: boolean; icon?: "back" | "plus" | "refresh" | "settings" | "close"; quiet?: boolean; iconOnly?: boolean } = {}): string => `<button type="button" id="action-${action}" data-action="${action}" class="button${options.primary ? " primary" : ""}${options.pagePrimary ? " page-primary" : ""}${options.quiet ? " quiet" : ""}${options.iconOnly ? " icon-only" : ""}"${options.iconOnly ? ` aria-label="${escape(label)}" title="${escape(label)}"` : ""}${disabled(options.disabled ?? false)}>${options.icon ? icon(options.icon) : ""}${options.iconOnly ? "" : escape(label)}</button>`;
type PresentedAction = NonNullable<RunView["presentation"]>["actions"][number];
const actionButton = (action: PresentedAction, primary = false, busy = false): string => {
  const label = action.kind === "reconcile" ? "Check running work" : action.kind === "steer" ? "Reply to Luna" : action.kind === "answer" ? "Answer" : action.label;
  return `<button type="button" class="button${primary ? " primary" : ""}" id="control-${escape(action.kind)}" data-action="control" data-kind="${escape(action.kind)}" data-tool="${escape(action.tool ?? "")}"${disabled(busy || !action.allowed || !action.tool)} aria-describedby="action-reason">${escape(label)}</button>`;
};
const statusMark = (tier: Tier, label: string): string => `<span class="status tier-${tier}"><span class="status-shape" aria-hidden="true"></span>${escape(label)}</span>`;
function proofLine(run: RunView): string {
  const counts = run.control ? run.presentation?.criteria : undefined;
  if (!counts) return `<span class="proof unverified">${phaseGlyph(0, 0)}Not checked yet</span>`;
  const progress = counts.failed ? `${counts.failed} need another look` : counts.proven === counts.mandatory ? "Done" : counts.proven ? `${counts.proven} of ${counts.mandatory} checks done` : "Not checked yet";
  return `<span class="proof">${phaseGlyph(counts.proven, counts.mandatory)}${progress}</span>`;
}
export function renderWorkbench(root: HTMLElement, state: ViewState, editor: Editor, preview: boolean, options: WorkbenchOptions = {}): void {
  options = { ...options, hostCanMessage: options.canSendFollowUps === true, canSendFollowUps: state.connected && options.canSendFollowUps === true && !state.pending && !state.refreshing && !state.graphLoading && !state.graphStale };
  options.since?.observe(state.runs);
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
  if (options.canvasWidth === undefined && root.clientWidth > 0) {
    // Mirrors the CSS: main max 1360px with 24px padding; the inspector column leaves at 861px+.
    const main = Math.min(root.clientWidth, 1360) - (root.clientWidth <= 520 ? 24 : 48);
    options.canvasWidth = (main > 860 ? main - 340 - 16 : main) - 2;
  }
  const errorPage = Boolean(state.error && !state.initialized);
  const offlineAt = run ? activityAt(run) : state.runs.map(activityAt).filter((value): value is number => value !== undefined).sort((a, b) => b - a)[0];
  const status = state.error && !state.initialized ? ""
    : state.error && state.connected
    ? `<div class="toast" role="alert">Couldn't complete that. ${button("refresh", "Retry", { quiet: true }).replace('id="action-refresh"', 'id="action-retry"')}</div>`
    : state.pending ? `<div class="notice pending"><span class="spinner" aria-hidden="true"></span>${state.pending.tool === "cancel_factory_run" ? "Stopping work and checking what remains…" : state.pending.tool === "connect_factory_github_repository" ? "Connecting the repo…" : state.pending.tool === "create_factory_local_repository" ? "Creating the repo…" : "Waiting for Luna…"}</div>`
        : state.notice ? `<div class="notice success" role="status">${escape(state.notice)}</div>` : "";
  const offline = !state.connected && state.initialized
    ? `<div class="offline-banner" role="status"><span>Offline. Showing what Luna saw${offlineAt === undefined ? " in the last saved view" : ` at ${escape(clock(offlineAt, options.now))}`}.</span>${button("reconnect", "Reconnect", { quiet: true })}</div>`
    : "";
  const stopMenuAction = run && runTier(run) === "working" ? run.presentation?.actions.find(action => action.kind === "cancel" && action.tool === "cancel_factory_run" && action.allowed) : undefined;
  const stopMenuItem = stopMenuAction ? actionButton(stopMenuAction, false, Boolean(state.pending)).replace("<button ", '<button role="menuitem" ') : "";
  root.innerHTML = `<div class="workbench" data-surface="${surface}" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">
    ${preview ? '<div class="preview-banner">Fixture visual preview · Synthetic run data · No live integration</div>' : ""}
    <header class="appbar${run && !editor ? " appbar-plan" : ""}">${run && !editor ? `<button type="button" id="action-overview" class="button quiet icon-only mobile-back" data-action="overview" aria-label="Back to plans" title="Back to plans">${icon("back")}</button>` : ""}<a class="brand" href="/" data-action="overview" aria-label="Luna Factory">${lunaMark(22)}<span>Luna Factory</span></a>${run && !editor ? `<nav class="breadcrumbs" aria-label="Breadcrumb"><span class="breadcrumb-separator">${icon("arrow", 12)}</span><a href="/" data-action="overview">Plans</a><span class="breadcrumb-separator">${icon("arrow", 12)}</span><span class="breadcrumb-title">${escape(run.objective)}</span></nav>` : ""}
      <div class="top-actions">${button(state.connected ? "refresh" : "reconnect", state.connected ? "Refresh" : "Reconnect", { icon: "refresh", quiet: true, iconOnly: true, disabled: state.refreshing || Boolean(state.pending) })}<details class="header-menu"><summary class="button quiet icon-only" aria-label="More options" title="More options">${icon("more")}</summary><div class="header-menu-popover" role="menu" aria-label="More options">${stopMenuItem}${button("settings", "Settings").replace("<button ", '<button role="menuitem" ')}${state.capabilities?.workspaces?.connect_github ? button("connect-github", "Connect GitHub", { disabled: !state.connected || Boolean(state.pending) }).replace("<button ", '<button role="menuitem" ') : ""}</div></details></div></header>
    <main id="main" class="main">
      <div class="status-area" aria-live="polite" aria-atomic="true">${status}${offline}${state.contextError ? `<div class="notice">${escape(state.contextError)}</div>` : ""}</div>
      ${surface === "thread" ? '<div class="thread-inspector-label">This conversation</div>' : ""}${errorPage ? `<section class="empty full-page-error" aria-labelledby="load-error-title">${lunaMark(40)}<h1 id="load-error-title">Couldn't reach Luna Factory</h1><p>The connection timed out.</p>${button("reconnect", "Try again", { primary: true })}</section>` : !state.initialized ? '<section class="empty loading-state" aria-label="Loading Luna"><div class="skeleton"><span></span><span></span><span></span></div></section>' : editor === "start" ? renderHome(state, options) : editor ? renderEditor(editor, state, run) : state.selectedId ? run ? renderCampaign(run, state, options) : '<section class="empty"><h1>Opening this plan</h1><p>Reading its latest state.</p></section>' : renderHome(state, options)}
    </main></div>`;
  const taskTitle = root.querySelector<HTMLTextAreaElement>('form[data-form="graph-node"] #task-title');
  const titleNode = state.graph?.nodes.find(node => node.id === state.selectedNodeId);
  const canonicalTitle = titleNode?.editor_title === null ? null : titleNode?.editor_title ?? titleNode?.title ?? null;
  if (taskTitle && titleNode) taskTitle.value = canonicalTitle === null ? "" : canonicalTitle;
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
    if (taskTitle && typeof draft.get("title") === "string") taskTitle.value = canonicalTitle === null ? String(draft.get("title")) : preserveCanonicalTaskTitle(String(draft.get("title")), canonicalTitle);
  }
  const objective = root.querySelector<HTMLTextAreaElement>("#objective");
  const planButton = root.querySelector<HTMLButtonElement>('button[type="submit"][value="plan"]');
  if (objective && planButton) planButton.disabled = !objective.value.trim() || !state.connected || Boolean(state.pending);
  const repoFilter = root.querySelector<HTMLInputElement>("#repo_filter");
  if (repoFilter?.value) filterRepositories(root, repoFilter.value);
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

type HomePlan = { id: string; title: string; repository: string; tier: Tier; history: boolean; meta: string; at: number | undefined; run?: RunView };
const campaignTier = (status: CampaignView["status"]): Tier => ({ planned: "planned", promoted: "unverified", finished: "finished", stopped: "stopped" } satisfies Record<CampaignView["status"], Tier>)[status];
function confirmedTerminalHistory(run: RunView): boolean {
  if (!(run.state === "CANCELLED" || run.state === "INTERRUPTED" || run.state === "FAILED") || run.active_workers > 0 || run.claim_held) return false;
  const presentation = run.presentation;
  if (!presentation) return true;
  return !presentation.claim.held && presentation.claim.status === "released"
    && presentation.owner.liveness === "idle" && presentation.workers.every(worker => worker.liveness === "idle");
}
function terminalStatusLabel(run: RunView): string {
  return run.state === "FAILED" ? "Failed" : run.state === "INTERRUPTED" ? "Interrupted" : "Stopped";
}

function githubUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "github.com" ? url.toString() : null;
  } catch { return null; }
}

function issueUrl(value: string | null): string | null {
  const url = githubUrl(value);
  if (!url) return null;
  return /^https:\/\/github\.com\/[^/]+\/[^/]+\/issues\/\d+$/i.test(url) ? url : null;
}

function pullRequestUrl(value: string | null): string | null {
  const url = githubUrl(value);
  if (!url) return null;
  return /^https:\/\/github\.com\/[^/]+\/[^/]+\/pull\/\d+$/i.test(url) ? url : null;
}

function homePlans(state: ViewState): HomePlan[] {
  const campaigns = state.campaigns ?? [];
  const linked = new Set(campaigns.flatMap(campaign => [campaign.planning_run_id, ...campaign.run_ids]));
  const fromCampaigns = campaigns.map(campaign => {
    const latestRun = [...campaign.runs].sort((a, b) => b.updated_at - a.updated_at).map(item => state.runs.find(run => run.id === item.id)).find((run): run is RunView => run !== undefined);
    const planRun = state.runs.find(run => run.id === campaign.planning_run_id);
    const run = latestRun ?? planRun;
    const ready = campaign.planning.tasks.ready;
    const parentUrl = issueUrl(campaign.parent.display?.url ?? null);
    const provenance = parentUrl
      ? `<a class="source-link" href="${escape(parentUrl)}" target="_blank" rel="noopener noreferrer">from #${campaign.parent.forge.number}</a>`
      : `from #${campaign.parent.forge.number}`;
    const meta = [escape(campaign.repository.alias), provenance, `${campaign.planning.tasks.total} ${campaign.planning.tasks.total === 1 ? "task" : "tasks"}`, `${ready} ready`, run?.pending_decision ? escape(plainSummary(run.pending_decision.question)) : ""].filter(Boolean).join(" · ");
    const history = run ? confirmedTerminalHistory(run) : campaign.status === "stopped";
    return { id: run?.id ?? campaign.planning_run_id, title: campaign.title, repository: campaign.repository.alias, tier: run ? history ? "stopped" : runTier(run) : campaignTier(campaign.status), history, meta, at: run ? activityAt(run) : campaign.updated_at, ...(run ? { run } : {}) };
  });
  const standalone = state.runs.filter(run => !linked.has(run.id)).map(run => {
    const tasks = run.control?.tasks ?? [];
    const ready = tasks.filter(task => task.state.toLowerCase() === "ready").length;
    const history = confirmedTerminalHistory(run);
    return { id: run.id, title: run.objective, repository: run.repository, tier: history ? "stopped" : runTier(run), history, meta: [escape(run.repository), `${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}`, `${ready} ready`, run.pending_decision ? escape(plainSummary(run.pending_decision.question)) : ""].filter(Boolean).join(" · "), at: activityAt(run), run };
  });
  return [...fromCampaigns, ...standalone];
}

function renderHome(state: ViewState, options: WorkbenchOptions): string {
  const plans = homePlans(state);
  const row = (plan: HomePlan): string => `<li><div class="plan-row tier-${plan.tier}" data-run-id="${escape(plan.id)}">
    <span class="plan-glyph">${plan.run ? miniMap(plan.run) || phaseGlyph(plan.run.presentation?.criteria.proven ?? 0, plan.run.presentation?.criteria.mandatory ?? 0, 22) : phaseGlyph(0, 0, 22)}</span>
    <span class="plan-main"><a class="plan-open" href="${runPath(plan.id)}" data-run-id="${escape(plan.id)}">${escape(plan.title)}</a><span class="plan-meta">${plan.meta}</span></span>
    <span class="plan-side">${statusMark(plan.tier, plan.history ? plan.run ? terminalStatusLabel(plan.run) : "Stopped" : plan.run ? statusLabel(plan.run) : plan.tier === "working" ? "Running" : plan.tier === "finished" ? "Done" : plan.tier === "planned" ? "Planned" : plan.tier === "needs" ? "Needs you" : plan.tier === "stopped" ? "Stopped" : "Status unknown")}<span class="plan-fresh">${escape(freshness(plan.at, options.now))}</span></span>
  </div></li>`;
  const sections = [
    { key: "needs", title: "Needs you", tiers: ["needs", "stopped", "unverified"] as Tier[] },
    { key: "running", title: "Running", tiers: ["working"] as Tier[] },
    { key: "planned", title: "Planned", tiers: ["planned"] as Tier[] },
    { key: "done", title: "Done", tiers: ["finished"] as Tier[] },
    { key: "history", title: "History", tiers: [] as Tier[] },
  ].map(group => {
    const rows = group.key === "history" ? plans.filter(plan => plan.history) : plans.filter(plan => !plan.history && group.tiers.includes(plan.tier));
    if (!rows.length) return "";
    if (group.key === "done") return `<details class="home-done"><summary>Show ${rows.length} done</summary><ul class="plan-list">${rows.map(row).join("")}</ul></details>`;
    if (group.key === "history") return `<details class="home-history"><summary>Show ${rows.length} from history</summary><ul class="plan-list">${rows.map(row).join("")}</ul></details>`;
    return `<section class="home-group group-${group.key}" aria-labelledby="group-${group.key}"><h2 id="group-${group.key}">${group.title}<span>${rows.length}</span></h2><ul class="plan-list">${rows.map(row).join("")}</ul></section>`;
  }).join("");
  const readNote = state.campaignsStale ? '<p class="home-read-note" role="status">Some saved plans could not be refreshed. Showing the last version.</p>' : state.campaignsSkipped ? `<p class="home-read-note" role="status">${state.campaignsSkipped} ${state.campaignsSkipped === 1 ? "plan" : "plans"} could not be shown.</p>` : "";
  return `<section class="home-page"><h1>Plans</h1><section class="plan-composer home-composer">${renderPlan(state)}</section>${readNote}<div class="home-plans">${sections}</div></section>`;
}


function renderCampaign(run: RunView, state: ViewState, options: WorkbenchOptions): string {
  const busy = !state.connected || Boolean(state.pending);
  const attention = needsOperatorDecision(run);
  const tier = runTier(run);
  const presentation = run.control ? run.presentation : undefined;
  const primary = presentation?.primary_action;
  const campaign = state.campaigns?.find(item => item.planning_run_id === run.id || item.run_ids.includes(run.id));
  const livenessUnknown = presentation?.blocker_kind?.kind === "liveness_unknown";
  const banner = attention || run.state === "BLOCKED";
  const taskCount = campaign?.planning.tasks.total ?? state.graph?.nodes.length ?? run.control?.tasks.length ?? 0;
  const readyCount = campaign?.planning.tasks.ready ?? state.graph?.nodes.filter(task => task.state.toLowerCase() === "ready").length ?? run.control?.tasks.filter(task => task.state.toLowerCase() === "ready").length ?? 0;
  const taskLedger = state.graph?.run_id === run.id ? state.graph.nodes : run.control?.tasks ?? [];
  const doneCount = taskLedger.filter(task => task.state.toLowerCase() === "done").length;
  const waveCount = state.graph?.run_id === run.id ? layoutWaves(state.graph.nodes).waves.length : 0;
  const executionReady = state.capabilities?.execution?.eligible === true;
  const reviewingChange = state.proposal?.status === "proposed";
  const currentCampaignGraph = campaign && state.graph?.run_id === campaign.planning_run_id && state.graph.revision === campaign.planning.revision && !state.graphStale;
  const planPrimary = run.planning_only
    ? reviewingChange ? "" : executionReady && currentCampaignGraph ? button("promote-campaign", "Start run", { primary: true, pagePrimary: true, disabled: busy || state.graphLoading }) : button("setup", "Set up running", { primary: true, pagePrimary: true, disabled: busy })
    : "";
  const attentionAction = livenessUnknown && !attention
    ? presentation?.actions.find(item => item.kind === "reconcile" && item.allowed) ?? primary
    : primary;
  const attentionTitle = attention ? run.pending_decision?.question ?? blockerCopy(run) ?? "Luna needs your answer." : livenessUnknown ? "Luna can't confirm the earlier run stopped." : blockerCopy(run) ?? "Luna is blocked.";
  const attentionBody = attention && run.pending_decision ? "Choose an answer to continue this plan." : !attention && livenessUnknown ? "Before starting new work, Luna needs to check what's still running in Codex." : reasonCopy(attentionAction?.reason ?? null);
  const stopAction = livenessUnknown && !attention ? presentation?.actions.find(item => item.kind === "cancel" && item.allowed) : undefined;
  const running = tier === "working";
  const agents = (presentation?.workers.filter(worker => worker.liveness === "active").length ?? 0) + (presentation?.owner.liveness === "active" ? 1 : 0);
  const runningMeta = running ? `Running · ${agents} ${agents === 1 ? "agent" : "agents"}${presentation?.budget.time_remaining_seconds === null || presentation?.budget.time_remaining_seconds === undefined ? "" : ` · ${Math.max(0, Math.ceil(presentation.budget.time_remaining_seconds / 60))}m left`}` : "";
  const meta = [statusMark(tier, statusLabel(run)), escape(run.repository), campaign ? `from #${campaign.parent.forge.number}` : "", `${taskCount} ${taskCount === 1 ? "task" : "tasks"}`, `${readyCount} ready`, waveCount ? `${waveCount} ${waveCount === 1 ? "step" : "steps"}` : ""].filter(Boolean).join(" · ");
  const safePullUrl = pullRequestUrl(presentation?.deliverable.reference ?? null);
  const finishedPrimary = tier === "finished" && safePullUrl ? `<a class="button primary page-primary" href="${escape(safePullUrl)}" target="_blank" rel="noopener noreferrer">Open pull request ${icon("open", 14)}</a>` : "";
  const serverPrimary = !run.planning_only && !reviewingChange && !banner && !running && tier !== "finished" && primary?.allowed && primary.tool ? actionButton(primary, true, busy).replace('class="button primary"', 'class="button primary page-primary"') : "";
  return `<section class="campaign-head">
      <div class="plan-title-row"><h1>${escape(run.objective)}</h1>${banner ? "" : planPrimary || serverPrimary || finishedPrimary}${running ? `<span class="running-meta">${escape(runningMeta)}</span>` : ""}</div>
      <p class="plan-meta-line">${meta}</p>
      ${run.planning_only || !taskCount ? "" : `<p class="plan-progress">${phaseGlyph(doneCount, taskCount, 18)}${doneCount} of ${taskCount} ${taskCount === 1 ? "task" : "tasks"} done</p>`}
      ${running ? `<details class="plan-menu"><summary aria-label="More actions">⋯</summary>${presentation?.actions.filter(item => item.kind === "cancel" && item.allowed && item.tool).map(item => actionButton(item, false, busy)).join("") ?? ""}</details>` : ""}
    </section>
    ${banner ? `<section class="plan-banner tier-${attention ? "needs" : livenessUnknown ? "stopped" : tier}" aria-labelledby="decision-title"><h2 id="decision-title">${escape(attentionTitle)}</h2><p id="action-reason">${escape(attentionBody)}</p><div class="banner-actions">${attentionAction?.allowed && attentionAction.tool ? actionButton(attentionAction, true, busy) : button("refresh", "Refresh", { primary: true, disabled: busy })}${!attention && livenessUnknown && stopAction ? `<button type="button" class="button" data-action="control" data-kind="cancel" data-tool="${escape(stopAction.tool ?? "")}"${disabled(busy)}>Stop everything</button>` : `<button type="button" class="button quiet" data-action="chat-follow-up" data-kind="blocker"${disabled(!options.canSendFollowUps)}>Ask in chat</button>`}</div></section>` : ""}
    ${renderProposalReview(state)}
    ${renderMission(state, run, task => renderFollowUpActions(run, options, task), options.canvasWidth, renderEvidence(run, state))}`;
}

function renderEvidence(run: RunView, state: ViewState): string {
  const checks = run.control?.criteria ?? run.acceptance.map(description => ({ description, status: "unproved" as const }));
  const checkSection = checks.length ? `<section class="detail-section"><h2>Done when</h2><ul class="detail-checks">${checks.map(check => `<li><span>${taskGlyph(check.status === "proven" ? "done" : check.status === "failed" ? "blocked" : "candidate", 16)}${escape(check.description)}</span><strong>${check.status === "proven" ? "Done" : check.status === "failed" ? "Needs attention" : "Not checked yet"}</strong></li>`).join("")}</ul></section>` : "";
  const model = run.route.observed_model ?? run.route.requested_model;
  const modelDetail = model ? `<section class="detail-section"><h2>Model</h2><p>${escape(model)}${run.route.observed_effort ? ` · ${escape(run.route.observed_effort)}` : ""}</p>${run.route.reroutes?.length ? '<p class="model-note">The model changed during this run. Luna stopped it for review.</p>' : ""}</section>` : "";
  const updatedAt = activityAt(run);
  const dates = [run.created_at === undefined ? "" : `${run.planning_only ? "Created" : "Started"} ${clock(run.created_at)}`, updatedAt === undefined ? "" : `Updated ${clock(updatedAt)}`].filter(Boolean);
  const dateSection = dates.length ? `<section class="detail-section"><h2>Dates</h2><p>${dates.map(escape).join(" · ")}</p></section>` : "";
  const remaining = run.presentation?.budget.time_remaining_seconds;
  const time = remaining === null || remaining === undefined ? "" : `<section class="detail-section"><h2>Time left</h2><p>${Math.ceil(remaining / 60)} minutes</p></section>`;
  const pullRequests = state.delivery?.run_id === run.id ? state.delivery.nodes.flatMap(node => node.pull_requests).flatMap(pull => {
    const url = pullRequestUrl(pull.url);
    return url ? [`<li><a href="${escape(url)}" target="_blank" rel="noopener noreferrer">Pull request #${pull.number} · ${escape(pull.state ?? "State unknown")}</a></li>`] : [];
  }).join("") : "";
  const pullRequest = pullRequests ? `<section class="detail-section"><h2>Pull request</h2><ul>${pullRequests}</ul></section>` : "";
  const technical = `<details class="technical-details"><summary>Show technical details</summary><dl><dt>Plan ID</dt><dd class="mono">${escape(run.id)}</dd><dt>Current subject</dt><dd class="mono">${escape(run.current_subject)}</dd><dt>Revision</dt><dd>${run.control?.revision ?? "unknown"}</dd><dt>Intent generation</dt><dd>${run.control?.intent_generation ?? "unknown"}</dd><dt>Dispatch generation</dt><dd>${run.control?.dispatch_generation ?? "unknown"}</dd><dt>Deadline</dt><dd>${run.planning_only ? "Not running" : new Date(run.deadline_at * 1000).toLocaleString()}</dd></dl></details>`;
  return `<section class="plan-details" aria-label="Plan details">${checkSection}${pullRequest}${modelDetail}${time}${dateSection}${technical}</section>`;
}

function renderInline(state: ViewState, run: RunView | undefined, options: WorkbenchOptions, preview: boolean): string {
  const notes = `${preview ? '<div class="preview-banner">Fixture preview · Sample data</div>' : ""}${state.connectionStatus === "disconnected" ? '<p class="connection-note" role="status">Offline. Reopen Luna Factory to reconnect.</p>' : ""}${state.error && state.connected ? '<p class="inline-feedback" role="alert">Luna couldn\'t complete that.</p>' : ""}${state.notice ? `<p class="inline-feedback" role="status">${escape(state.notice)}</p>` : ""}`;
  const shell = (body: string) => `<div class="workbench inline-surface" data-surface="inline" data-display-mode="${escape(options.displayMode ?? "inline")}" data-connection="${state.connectionStatus}">${notes}${body}</div>`;
  if (!run) return shell(`<article class="inline-card"><header class="inline-top">${lunaMark(18)}<span>Luna Factory</span></header><h2>No plans yet</h2><p class="inline-now">Open Luna Factory to describe what you want done.</p></article>`);
  const tier = runTier(run);
  const followKind: FollowUpKind = run.blocker || run.remaining_gap || run.pending_decision ? "blocker" : "summary";
  const summary = run.presentation?.blocker_kind?.kind === "liveness_unknown" ? "Luna can't confirm the earlier run stopped." : plainSummary(nowSentence(run));
  const primaryLabel = tier === "needs" && run.pending_decision ? "Answer" : "Open";
  const followLabel = "Ask in chat";
  return shell(`<article class="inline-card tier-${tier}" aria-labelledby="inline-title">
    <header class="inline-top">${lunaMark(18)}<span>${escape(run.repository)}</span>${statusMark(tier, statusLabel(run))}<span class="inline-fresh">${escape(freshness(activityAt(run), options.now))}</span></header>
    <div class="inline-body"><span class="inline-sky">${miniMap(run) || phaseGlyph(run.presentation?.criteria.proven ?? 0, run.presentation?.criteria.mandatory ?? 0, 32)}</span><div><h2 id="inline-title">${escape(run.objective.slice(0, 300))}</h2><p class="inline-now">${escape(summary)}</p><p class="inline-proof">${proofLine(run)}</p></div></div>
    <div class="inline-actions"><button id="follow-up-${followKind}-${escape(run.id)}" class="button quiet" type="button" data-action="chat-follow-up" data-kind="${followKind}"${disabled(!options.canSendFollowUps)}${options.messagePending ? ' aria-disabled="true"' : ""}>${icon("chat")}${followLabel}</button><button class="button primary" type="button" id="action-expand-mode" data-action="expand-mode"${disabled(!options.canExpand || !state.connected)}>${primaryLabel}</button></div>
    ${options.messagePending ? '<p class="inline-hint" role="status">Sending your message…</p>' : ""}
  </article>`);
}

function renderFollowUpActions(run: RunView, options: WorkbenchOptions, task?: FollowUpTask): string {
  const kind: FollowUpKind = task ? "choose" : run.blocker || run.remaining_gap || run.pending_decision ? "blocker" : "summary";
  return `<div class="chat-followups"><button id="follow-up-${kind}-${escape(task?.id ?? run.id)}" type="button" class="ask" data-action="chat-follow-up" data-kind="${kind}"${task ? ` data-task-id="${escape(task.id)}"` : ""}${disabled(!options.canSendFollowUps)}${options.messagePending ? ' aria-disabled="true"' : ""}>${icon("chat", 14)}Ask in chat</button></div>`;
}

const field = (name: string, label: string, control: string, hint = ""): string => `<div class="field"><label for="${name}">${label}</label>${control}${hint ? `<p class="field-hint" id="${name}-hint">${hint}</p>` : ""}</div>`;
function renderEditor(editor: Exclude<Editor, null>, state: ViewState, run: RunView | undefined): string {
  const title = { start: "Plans", settings: "Settings", repositories: "Repos", steer: run?.pending_decision ? "Answer Luna" : "Reply to Luna", stop: "Stop this run?" }[editor];
  const locked = !state.connected || Boolean(state.pending);
  const header = `<div class="editor-heading"><div>${editor === "start" ? "" : '<span class="eyebrow">Luna Factory</span>'}<h1>${title}</h1></div>${button("close-editor", "Close", { icon: "close", disabled: Boolean(state.pending) })}</div>`;
  if (editor === "repositories") return `${header}<section class="editor-card"><p>Connect GitHub or create a repo from the plan composer.</p></section>`;
  if (editor === "steer" || editor === "stop") {
    if (!run) return `${header}<p>Select a run first.</p>`;
    if (editor === "stop") return `${header}<section class="editor-card"><p class="lead">${escape(run.objective)}</p><p>Luna will stop this run and check whether its agents have stopped too. If any are still active or cannot be checked, the plan stays blocked.</p><form data-form="stop"><div class="form-actions"><button class="button danger" type="submit"${disabled(locked)}>Stop run</button>${button("close-editor", "Keep running", { disabled: Boolean(state.pending) }).replace('id="action-close-editor"', 'id="action-keep-running"')}</div></form></section>`;
    return `${header}<section class="editor-card">${run.pending_decision || run.blocker ? `<div class="decision-quote">${escape(run.pending_decision?.question || blockerCopy(run) || "")}</div>` : `<p>Reply to Luna about <strong>${escape(run.repository)}</strong>.</p>`}<form data-form="steer" data-input-identity="${escape(run.pending_decision?.id || run.turn_id || run.id)}">${field("message", "Your reply", '<textarea id="message" name="message" rows="6" required maxlength="4000" placeholder="What should Luna do next?" aria-describedby="message-hint"></textarea>', run.pending_decision ? "Your answer goes to Luna in this conversation." : "Your reply goes to Luna in this conversation.")}<div class="form-actions"><button class="button primary" type="submit"${disabled(locked || !run.pending_decision && !run.turn_id)}>Send reply</button></div></form></section>`;
  }
  if (editor === "start") return `<section class="plan-composer">${header}${renderPlan(state)}</section>`;
  const cap = state.capabilities;
  if (!cap || !cap.profiles.length) return `${header}${renderRunningSetup(state)}<div class="notice">No model is available. Check Luna's setup.</div>`;
  const profileOptions = cap.profiles.map(profile => `<option value="${escape(profile.alias)}"${profile.alias === state.settings.profile ? " selected" : ""}>${escape(profile.alias)} · ${escape(profile.effort)}</option>`).join("");
  const finishOptions = allowedFinishes("pr").map(finish => `<option value="${finish}"${finish === state.settings.finish ? " selected" : ""}>${finishLabels[finish]}</option>`).join("");
  const defaults = planDefaults(cap);
  const limitMinutes = Math.max(1, Math.floor(cap.limits.wall_seconds / 60));
  return `${header}${renderRunningSetup(state)}<section class="editor-card"><h2>Defaults</h2><form data-form="settings">
    <div class="form-grid">${field("profile", "Model", `<select id="profile" name="profile">${profileOptions}</select>`)}${field("capacity", "Agents at once", `<input id="capacity" name="capacity" type="number" min="1" max="${cap.limits.capacity}" step="1" value="${Math.min(state.settings.capacity ?? defaults.capacity, cap.limits.capacity)}" required aria-describedby="capacity-hint">`, `Up to ${cap.limits.capacity} agents`)}</div>
    ${field("finish", "When it's done", `<select id="finish" name="finish">${finishOptions}</select>`)}
    <div class="default-limits"><p><strong>Time limit</strong><span>${Math.min(limitMinutes, Math.round(defaults.wall_seconds / 60))} minutes · set by Luna</span></p><p><strong>Retries</strong><span>${defaults.repair_attempts} · set by Luna</span></p></div>
    <div class="form-actions"><button class="button primary" type="submit"${disabled(locked)}>Save defaults</button></div></form></section>`;
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
 * Finish preference and things to leave alone are the only optional fields. Model and capacity
 * come from Settings; time and retry limits come from the server.
 */
function renderPlan(state: ViewState): string {
  const cap = state.capabilities;
  if (!cap || !cap.profiles.length) return '<section class="plan-card"><p class="plan-note">Luna needs a model configured before it can plan.</p></section>';
  const locked = !state.connected || Boolean(state.pending);
  const defaults = { ...planDefaults(cap), ...state.settings };
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
  const finishes = allowedFinishes(creating ? "local_candidate" : picked?.max_finish ?? "local_candidate");
  const options = `<details id="plan-more" class="plan-options plan-more"><summary>Options</summary><div class="plan-more-body">
      ${field("non_goals", "Leave alone", '<textarea id="non_goals" name="non_goals" rows="2" maxlength="32032" placeholder="One thing per line" aria-describedby="non_goals-hint"></textarea>', "Optional, one per line.")}
      <div class="form-grid">${field("finish", "When it's done", `<select id="finish" name="finish">${finishes.map(finish => `<option value="${finish}"${finish === defaults.finish ? " selected" : ""}>${planFinishLabels[finish]}</option>`).join("")}</select>`)}</div>
    </div></details>`;
  return `<section class="plan-card"><form data-form="start" class="plan-form">
    <div class="plan-box">
      <div class="composer"><label class="sr-only" for="objective">Describe what you want done</label><textarea id="objective" name="objective" rows="4" required maxlength="1000" placeholder="Describe what you want done, or a new thing to build."></textarea>
        <input type="hidden" name="repository" value="${escape(picked?.alias ?? "")}">
        <div class="plan-controls">${chip}${nameField}${options}<span class="plan-spacer"></span><button class="button primary plan-go" type="submit" value="plan" disabled>Plan it</button></div>
      </div>
      <p class="plan-error" id="plan-repo-error" role="alert" hidden></p>
      <p class="plan-note">Luna drafts a plan first. Nothing runs until you press Start run.</p>
    </div>
  </form></section>`;
}

function renderRunningSetup(state: ViewState): string {
  const eligible = state.capabilities?.execution?.eligible === true;
  return `<section class="editor-card setup-card" aria-labelledby="setup-running"><div class="section-heading"><h2 id="setup-running" tabindex="-1">Running</h2><span class="setup-state">${eligible ? "Ready" : "Not ready"}</span></div>${eligible
    ? "<p>The server reports that running is ready. It shows Start run only on plans it approves.</p>"
    : "<p>Running needs setup on the machine hosting Luna. Finish setup there; Start run appears when the server reports it is ready.</p>"}</section>`;
}
