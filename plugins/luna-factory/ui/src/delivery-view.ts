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
function compactChecks(pull: DeliveryPull): string {
  const items = outcomes(pull);
  if (!items.length) return "no checks reported";
  if (!pull.checks.truncated && items.length === pull.checks.total && items.every(item => item.outcome === "success")) return "checks passed";
  const passed = items.filter(item => item.outcome === "success").length;
  return `${passed} of ${pull.checks.total || items.length} checks passed`;
}
function compactMapChecks(pull: DeliveryPull, stale: boolean): string {
  const items = outcomes(pull);
  if (!items.length) return "no checks reported";
  if (!pull.checks.truncated && items.length === pull.checks.total && items.every(item => item.outcome === "success")) return stale ? "passed" : "checks passed";
  return `${items.filter(item => item.outcome === "success").length}/${pull.checks.total || items.length}${stale ? "" : " passed"}`;
}
function reviewLabel(pull: DeliveryPull): string | null {
  if (pull.review_decision === "approved") return "Review approved";
  if (pull.review_decision === "changes_requested") return "Changes requested";
  if (pull.review_decision === "review_required") return "Review required";
  return null;
}
function safePullUrl(value: string): string | null {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "github.com" && !url.username && !url.password && /^\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/pull\/\d+\/?$/i.test(url.pathname) ? url.toString() : null;
  } catch { return null; }
}

/** A compact pull-request line inside the task card, only when GitHub reported a pull request. */
export function deliveryChip(state: ViewState, runId: string | undefined, nodeId: string): string {
  const report = deliveryFor(state, runId, nodeId);
  const node = report?.node;
  if (!report || !node || node.status !== "observed") return "";
  const pull = primaryPull(node);
  if (!pull) return "";
  const stale = node.freshness === "stale";
  const items = outcomes(pull);
  const when = observedAge(report.delivery, node);
  const spoken = `Reported by GitHub: pull request ${pull.number}, ${pullState(pull)}, ${checksSentence(pull)}${pull.ready_for_review ? ", ready for review" : ""}${stale && when ? `, stale, observed ${when}` : ""}.`;
  const outcome = items.find(item => item.outcome === "failure")?.outcome ?? items.find(item => item.outcome === "pending")?.outcome ?? items[0]?.outcome ?? "neutral";
  const summary = `PR #${pull.number} · ${compactMapChecks(pull, stale)}${stale ? " · stale" : ""}`;
  return `<span class="pr-meta${stale ? " stale" : ""}" title="${esc(spoken)}"><i class="check-dot c-${outcome}" aria-hidden="true"></i><span>${esc(summary)}</span><span class="sr-only">${esc(spoken)}</span></span>`;
}

/** Pull-request details appear only when the server returned GitHub data. */
export function deliverySection(state: ViewState, runId: string | undefined, nodeId: string): string {
  const report = deliveryFor(state, runId, nodeId);
  if (!report) return "";
  const { delivery, node } = report;
  const retry = delivery.retry_at !== null ? ` Luna will not ask GitHub again for about ${Math.max(1, Math.round((delivery.retry_at - delivery.observed_at) / 60))} min.` : "";
  const header = `<h4>Pull request</h4>`;
  if (!node) {
    if (delivery.available) return "";
    return `<section class="github-report" aria-label="Pull request">${header}<p class="small">Unavailable: ${esc(deliveryReason(delivery.reason))}.${esc(retry)}</p></section>`;
  }
  if (node.status !== "observed") {
    const why = node.status === "deferred" ? "Not read yet. Waiting for the next polling window." : `Unavailable: ${deliveryReason(node.reason)}.`;
    return `<section class="github-report" aria-label="Pull request">${header}<p class="small">${esc(why)}${delivery.reason === "rate_limited" ? esc(retry) : ""}</p></section>`;
  }
  const stale = node.freshness === "stale";
  const staleNote = stale ? `<p class="small stale-line">GitHub data may be out of date: ${esc(deliveryReason(node.reason))}.${esc(retry)}</p>` : "";
  const pulls = node.pull_requests.map(pull => {
    const items = outcomes(pull);
    const label = `#${pull.number} · ${reviewLabel(pull) ?? pullState(pull)} · ${compactChecks(pull)}`;
    const url = safePullUrl(pull.url);
    const summary = url ? `<a class="pr-report-link" href="${esc(url)}" target="_blank" rel="noopener noreferrer" title="Open pull request on GitHub">${esc(label)}</a>` : `<span class="pr-report-link">${esc(label)}</span>`;
    const failures = items.filter(item => item.outcome === "failure");
    const failedChecks = failures.length ? `<ul class="check-list">${failures.map(item => `<li>${dot(item.outcome)}<span>${esc(item.name)} <small>failed${item.summary ? ` · ${esc(item.summary)}` : ""}</small></span></li>`).join("")}</ul>` : "";
    return `<div class="pr-report">${summary}${failedChecks}</div>`;
  }).join("");
  const none = node.pull_requests.length ? "" : '<p class="small muted">GitHub reports no linked pull request for this issue.</p>';
  const more = node.pull_requests_truncated ? `<p class="small muted">Showing ${node.pull_requests.length} of ${node.pull_requests_total} linked pull requests.</p>` : "";
  // Only an explicit `complete: false` from the server marks a partial read; stale data already says so.
  const partial = node.complete === false && !stale ? '<p class="small muted">Partial read: GitHub did not return every linked pull request or check.</p>' : "";
  return `<section class="github-report" aria-label="Pull request">${header}${staleNote}${pulls}${none}${more}${partial}</section>`;
}
