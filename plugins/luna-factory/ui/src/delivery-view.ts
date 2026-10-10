// GitHub-reported delivery telemetry. Display only: it is never Luna proof, never attention and never
// merge or integration authority. Every value is labelled as reported by GitHub.
import type { ViewState } from "./controller";
import type { Delivery, DeliveryNode, DeliveryPull } from "./domain";

const esc = (value: string | number): string => String(value).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** The delivery report for this run's node, only when the server returned one. */
export function deliveryFor(state: ViewState, runId: string | undefined, nodeId: string): { delivery: Delivery; node: DeliveryNode | undefined } | null {
  const delivery = state.delivery;
  if (!delivery || !runId || delivery.run_id !== runId) return null;
  return { delivery, node: delivery.nodes.find(node => node.node_id === nodeId) };
}

const reasonLabels: Record<string, string> = {
  rate_limited: "GitHub's rate limit was reached",
  github_unreachable: "GitHub could not be reached from Luna",
  github_timeout: "GitHub did not answer in time",
  github_app_not_configured: "The GitHub App is not configured",
  github_app_not_installed: "The GitHub App is not installed on this repository",
  repository_not_in_github_app_allowlist: "This repository is not allowed for the GitHub App",
  repository_not_approved_in_luna: "This repository is not approved in Luna",
  issue_unavailable: "GitHub no longer reports this issue",
  source_repository_mismatch: "This task's issue belongs to a different repository",
  polling_budget_deferred: "Waiting for the next polling window",
  no_github_sources: "No task in this plan has a GitHub source",
};
export function deliveryReason(reason: string | null): string {
  if (!reason) return "GitHub reported no further detail.";
  return reasonLabels[reason] ?? "GitHub data is unavailable for a reason the server did not detail.";
}

function age(seconds: number): string {
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `${hours} h ago` : `${Math.round(hours / 24)} days ago`;
}
/** Age relative to the server's read time, so it never depends on the viewer's clock. */
function observedAge(delivery: Delivery, node: DeliveryNode): string | null {
  return node.observed_at === null ? null : age(Math.max(0, delivery.observed_at - node.observed_at));
}

type Outcome = "success" | "failure" | "pending" | "neutral";
const outcomeLabels: Record<Outcome, string> = { success: "passed", failure: "failed", pending: "pending", neutral: "neutral or skipped" };
function runOutcome(run: DeliveryPull["checks"]["runs"][number]): Outcome {
  if (run.status !== "completed") return "pending";
  if (run.conclusion === "success") return "success";
  if (["neutral", "skipped", "stale"].includes(run.conclusion ?? "")) return "neutral";
  return "failure";
}
function statusOutcome(state: string | null): Outcome {
  return state === "success" ? "success" : state === "pending" || state === "expected" ? "pending" : state === "failure" || state === "error" ? "failure" : "neutral";
}
function outcomes(pull: DeliveryPull): Array<{ name: string; outcome: Outcome; summary: string | null }> {
  return [
    ...pull.checks.runs.map(run => ({ name: run.name, outcome: runOutcome(run), summary: run.summary })),
    ...pull.checks.statuses.map(status => ({ name: status.context, outcome: statusOutcome(status.state), summary: null })),
  ];
}
const dot = (outcome: Outcome) => `<i class="check-dot c-${outcome}" aria-hidden="true"></i>`;
/** Open pull requests first: the most relevant reported delivery for a task. */
function primaryPull(node: DeliveryNode): DeliveryPull | undefined {
  return node.pull_requests.find(pull => pull.state === "open") ?? node.pull_requests[0];
}
function pullState(pull: DeliveryPull): string {
  if (pull.state === "open") return pull.draft ? "draft" : "open";
  return pull.state === "merged" ? "merged on GitHub" : pull.state === "closed" ? "closed" : "state unknown";
}
function checksSentence(pull: DeliveryPull): string {
  const items = outcomes(pull);
  if (!items.length) return "no checks reported";
  const passed = items.filter(item => item.outcome === "success").length;
  const failed = items.filter(item => item.outcome === "failure").length;
  const pending = items.filter(item => item.outcome === "pending").length;
  return [`${passed} of ${pull.checks.total || items.length} checks passed`, failed ? `${failed} failed` : "", pending ? `${pending} pending` : ""].filter(Boolean).join(", ");
}

/** A compact PR chip with check dots for a map node. Omitted unless GitHub reported a pull request. */
export function deliveryChip(state: ViewState, runId: string | undefined, nodeId: string): string {
  const report = deliveryFor(state, runId, nodeId);
  const node = report?.node;
  if (!report || !node || node.status !== "observed") return "";
  const pull = primaryPull(node);
  if (!pull) return "";
  const stale = node.freshness === "stale";
  const items = outcomes(pull);
  const shown = items.slice(0, 5);
  const ready = pull.ready_for_review && !stale;
  const when = observedAge(report.delivery, node);
  const spoken = `Reported by GitHub: pull request ${pull.number}, ${pullState(pull)}, ${checksSentence(pull)}${ready ? ", ready for review" : ""}${stale && when ? `, stale, observed ${when}` : ""}.`;
  return `<span class="pr-chip${stale ? " stale" : ""}${ready ? " ready" : ""}" title="${esc(spoken)}"><span aria-hidden="true">PR #${esc(pull.number)}</span>${shown.length ? `<span class="check-dots" aria-hidden="true">${shown.map(item => dot(item.outcome)).join("")}${items.length > shown.length ? `<small>+${items.length - shown.length}</small>` : ""}</span>` : ""}<span class="sr-only">${esc(spoken)}</span></span>`;
}

/** The inspector's "Reported by GitHub" section. Present only once `read_factory_delivery` answered. */
export function deliverySection(state: ViewState, runId: string | undefined, nodeId: string): string {
  const report = deliveryFor(state, runId, nodeId);
  if (!report) return "";
  const { delivery, node } = report;
  const caveat = '<p class="small muted">Shown for context only. GitHub results are not Luna proof, and nothing here can merge or integrate.</p>';
  const retry = delivery.retry_at !== null ? ` Luna will not ask GitHub again for about ${Math.max(1, Math.round((delivery.retry_at - delivery.observed_at) / 60))} min.` : "";
  const header = `<h4>Reported by GitHub</h4>`;
  if (!node) {
    if (delivery.available) return "";
    return `<section class="github-report" aria-label="Reported by GitHub">${header}<p class="small">Unavailable: ${esc(deliveryReason(delivery.reason))}.${esc(retry)}</p>${caveat}</section>`;
  }
  const when = observedAge(delivery, node);
  if (node.status !== "observed") {
    const why = node.status === "deferred" ? "Not read yet. Waiting for the next polling window." : `Unavailable: ${deliveryReason(node.reason)}.`;
    return `<section class="github-report" aria-label="Reported by GitHub">${header}<p class="small">${esc(why)}${delivery.reason === "rate_limited" ? esc(retry) : ""}</p>${caveat}</section>`;
  }
  const stale = node.freshness === "stale";
  const freshness = `<p class="small${stale ? " stale-line" : " muted"}">${stale ? `Stale · observed ${esc(when ?? "earlier")}. ${esc(deliveryReason(node.reason))}.${esc(retry)}` : `Observed ${esc(when ?? "at an unknown time")}`}</p>`;
  const issue = node.issue ? `<p class="small">Issue #${esc(node.issue.number)} · ${esc(node.issue.state === "closed" ? "closed on GitHub" : node.issue.state === "open" ? "open" : "state unknown")}</p>` : "";
  const pulls = node.pull_requests.map(pull => {
    const items = outcomes(pull);
    const review = pull.review_decision === "approved" ? "review approved" : pull.review_decision === "changes_requested" ? "changes requested" : pull.review_decision === "review_required" ? "review required" : "no review decision reported";
    const additions = pull.diff.additions ?? "?"; const deletions = pull.diff.deletions ?? "?"; const files = pull.diff.files_changed;
    const ready = pull.ready_for_review ? `<p class="ready-line">${dot("success")}<span>Ready for review${stale ? " (as last observed)" : ""} · open, not a draft, all reported checks passed</span></p>` : "";
    return `<div class="pr-report">
      <p><strong>Pull request #${esc(pull.number)}</strong> · ${esc(pullState(pull))} · ${esc(review)}</p>
      ${ready}
      <p class="small diff-stat"><span class="add">+${esc(additions)}</span> <span class="del">−${esc(deletions)}</span>${files !== null ? ` · ${esc(files)} ${files === 1 ? "file" : "files"}` : ""}${pull.head_sha ? ` · head <span class="mono">${esc(pull.head_sha.slice(0, 7))}</span>` : ""}</p>
      <p class="small">${esc(checksSentence(pull))}${pull.checks.truncated ? ` · showing ${items.length} of ${pull.checks.total}` : ""}</p>
      ${items.length ? `<ul class="check-list">${items.map(item => `<li>${dot(item.outcome)}<span>${esc(item.name)}<small>${esc(outcomeLabels[item.outcome])}${item.summary ? ` · ${esc(item.summary)}` : ""}</small></span></li>`).join("")}</ul>` : ""}
    </div>`;
  }).join("");
  const none = node.pull_requests.length ? "" : '<p class="small muted">GitHub reports no linked pull request for this issue.</p>';
  const more = node.pull_requests_truncated ? `<p class="small muted">Showing ${node.pull_requests.length} of ${node.pull_requests_total} linked pull requests.</p>` : "";
  return `<section class="github-report" aria-label="Reported by GitHub">${header}${freshness}${issue}${pulls}${none}${more}${caveat}</section>`;
}
