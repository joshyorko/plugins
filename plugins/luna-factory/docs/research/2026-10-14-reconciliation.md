# Luna Factory research reconciliation (2026-10-14)

This note compares the 2026-10-10 research with the work that has merged since. It is checked against `origin/main` at `f6e2417`. It changes no issue, code or authority.

Sources:

- [Architectural investigation](2026-10-10-architectural-investigation.md) and [engineering handoff](2026-10-10-engineering-handoff.md). Both are kept unchanged. They were written against PR68 (`bec70a5`) and Rust source `dd304ef`.
- Staged plans in [`plans/`](plans/), checked against `e194b01`:
  - [#79 per-attempt proof, then two workers](plans/79-per-attempt-proof-then-two-workers.md)
  - [#80 route policy, then verified escalation](plans/80-route-policy-then-verified-escalation.md)
  - [#81 certified delivery, then authorized merge](plans/81-certified-delivery-then-authorized-merge.md)
  - [#69 activity cursor and #61 continuation checkpoint](plans/69-61-activity-cursor-and-continuation-checkpoint.md)
- Issue bodies (#61, #67, #69–#82) and the PR list as read on 2026-10-14.

Line references in the plans refer to `e194b01`. #86, #88 and #89 touched `lifecycle.rs`, `store.rs`, `config.rs` and `schemas.rs` after that, so recheck the lines before using them.

## What merged since the research

| PR | Merge | Issues | Link type |
| --- | --- | --- | --- |
| #83 | model-callable view tools and titled context | #74 | Closes #74; Part of #67 |
| #84 | moonlight logo and in-app mark | #72 | Closes #72; Part of #67 |
| #85 | OCI image build fix | none | Part of #67 |
| #87 (`e194b01`) | receipt threads, run timing, source display, typed blockers | #69, #70, #73 | Closes #73; Part of #67 (#69 and #70 named in the title only) |
| #86 | campaign entity | #77 | Part of #77; Part of #67 |
| #88 | read-only CAS agent timeline for Lanes | #75 | Part of #75; Part of #67 |
| #89 (`f6e2417`) | opt-in least-privileged GitHub App reads | #76, #78 | Part of #76; Part of #78; Part of #67 |

Open: draft PR #91, "enforce persisted execution qualification at every effect path". It is Part of #71 and Part of #61.

Closed issues: #72, #73, #74. All the others listed here are still open.

## Status per issue

Each status line is kept separate. A pass on one line never implies a pass on another.

- **Source checks:** the change is in `main` with its own tests.
- **CI:** hosted checks on the merge commit.
- **Deployed build:** the exact build runs on the isolated preview host.
- **Authenticated ChatGPT acceptance:** an operator checked it through the authenticated ChatGPT host.
- **Provider qualification:** recorded #71 evidence for a real, subscription-entitled native provider.
- **End-to-end convergence:** a real campaign reached a verified result through this path.

Unless stated otherwise, "not recorded" means there is no evidence in this repository, not that the check failed. CI on `f6e2417` was still running when this note was written. The previous main commits (`ce9ad15`, `c0234ab`) passed.

### #61 autonomous continuation

- Source checks: done for the bounded live-owner `next` and `repair` path (#63, fixture-proven). Draft #91 adds the persisted qualification gate. It is not merged.
- CI: passed for #63. #91 is not merged.
- Deployed build: not recorded.
- Authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not done. This is gated by #71.
- End-to-end convergence: not done. The only live test (`native_live.rs`) sends just `initialize` and `model/list`.
- Residual: the live canary, which uses a real owner and worker, a bounded repair, a daemon kill with no replay, and cancellation with a real child and terminal. See the [checkpoint plan](plans/69-61-activity-cursor-and-continuation-checkpoint.md).

### #67 Mission Control (parent)

- Source checks: PR68 and the follow-ups #83–#89 are merged.
- CI: passed for PR68 and the follow-ups that have finished.
- Deployed build: not recorded for PR68 or later. The last preview the user reported was the PR66 planning-only canary.
- Authenticated ChatGPT acceptance: not recorded for PR68 or later.
- Provider qualification: not applicable to the UI. Execution views depend on #71.
- End-to-end convergence: not done.

### #69 receipt attribution and projections

- Source checks: done in #87 (`receipts[].thread_id`, `run.created_at`, wall-clock `activity_at`, source `display`).
- CI: passed.
- Deployed build: not recorded.
- Authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not applicable.
- End-to-end convergence: not applicable.
- Residual: canonical ordering. Receipts are ordered by wall clock at seconds resolution, and graph propose and apply do not move `activity_at`. The plan projects `receipts[].sequence`, adds a `control_revision` per receipt, and adds a `run.activity_cursor` with truncation reporting. Timestamps stay display-only.

### #70 typed blockers

- Source checks: done in #87, which names #70 in its title but doesn't link it. The issue is still open.
- CI: passed.
- Deployed build and authenticated ChatGPT acceptance: not recorded.
- Provider qualification and end-to-end convergence: not applicable.

### #71 native Codex execution qualification

- Source checks: draft #91 (persisted qualification checked at every effect path) is not merged. On `main`, eligibility is still hard-coded false.
- CI: #91 has not merged, so there is no `main` result.
- Deployed build: not recorded.
- Authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not done. **Operator-gated.** This needs live evidence of subscription entitlement with no API fallback, plus adapter start, observe, steer, stop and reconcile, cessation, the claim lifecycle, and a crash window. CAS execution qualification is separate and stays disabled.
- End-to-end convergence: not done.

### #72, #73, #74

These are closed by #84, #87 and #83. Source checks and CI passed. Deployed build and authenticated ChatGPT acceptance are not recorded. #74's desktop draft and mobile handoff in particular still need host acceptance.

### #75 CAS timeline

- Source checks: done in #88 (the read-only allowlist for the timeline, turn, item and snapshot reads, with the existing bounds).
- CI: passed.
- Deployed build and authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not done. CAS stays loopback-only with `binding_verified:false`. Non-loopback transport needs a separately approved decision.
- End-to-end convergence: not applicable.
- Residual: proof that the timeline methods exist on the exact native version that is deployed. The research found that `thread/timeline/list` is not available on every version.

### #76 GitHub App intake

- Source checks: done in #89 (opt-in, least-privileged reads, off by default).
- CI: the `main` run on `f6e2417` was still in progress.
- Deployed build: not recorded.
- Authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not applicable. **Operator-gated:** installing the App and handling its credentials.
- End-to-end convergence: not done.
- Residual: live intake against a real installation, and the remaining sub-issue and dependency cases in the issue.

### #77 campaigns

- Source checks: done in #86 (a thin campaign reference, read-only list and get tools).
- CI: passed.
- Deployed build and authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not done. Promotion to an executable run is still gated by #71.
- End-to-end convergence: not done.
- Residual: idempotent promotion from an exact approved revision (stage 2 of the #61 checkpoint).

### #78 PR, CI and review telemetry

- Source checks: done in #89 for GitHub-reported reads. They are labelled as reported by GitHub and never count as proof.
- CI: as for #76.
- Deployed build and authenticated ChatGPT acceptance: not recorded.
- Provider qualification: not applicable. A live GitHub App installation is operator-gated.
- End-to-end convergence: not done.
- Residual: the retained source/head, producer/policy and bounded poll state that #81 needs before any autonomous delivery.

### #79 parallel dispatch (operator-gated)

- Source checks, CI, deployed build, authenticated ChatGPT acceptance and end-to-end convergence: not started.
- Provider qualification: not done. It requires #71 and the #61 live checkpoint.
- Plan: per-attempt ownership, settlement and proof while still serial, then workspace and capacity reservations, then combined-candidate verification, and only then two workers. The single-active guard is not removed in the first PR.

### #80 routing and escalation (operator-gated)

- Source checks, CI, deployed build, authenticated ChatGPT acceptance and end-to-end convergence: not started.
- Provider qualification: not done. It requires #71 to name which turn-bound native field, if any, reports model and effort. If no such field exists, observed routing stays null and escalation stays disabled.
- Plan: a route policy in the immutable run contract, then a verified per-turn route, then one bounded, persisted escalation per attempt. The operator chooses models, efforts and budgets.

### #81 authorized integration and merge (operator-gated)

- Source checks, CI, deployed build, authenticated ChatGPT acceptance and end-to-end convergence: not started.
- Provider qualification: not done. It requires #71 for live use, #78 observations for PR and merge, and the #76 credential.
- Plan: a logical effect journal, then a certified push, then an open PR for review, then merge as a separately granted authority.
- **#81 must not depend on #79.** A verified sequential candidate can be pushed, opened and merged without parallel dispatch, and parallelism adds no authority. The issue body still lists #79 as a requirement. That edge should be removed when the issue is next edited.

### #82 multiple campaigns per repository (operator-gated)

- Source checks, CI, deployed build, authenticated ChatGPT acceptance and end-to-end convergence: not started.
- Provider qualification: not done. It requires #79, and then its own qualification of claim isolation and shared resources.
- Listing several source-only campaigns already works through #86. Concurrent writers stay deferred.

## Operator-gated items

None of these can be closed by source checks or CI:

1. **#71 live qualification:** real subscription entitlement, adapter control, cessation, the claim lifecycle and a crash window, recorded in a verification doc.
2. **#79:** enabling a second worker after the serial stages and the #71 profile pass.
3. **#80:** choosing exact allowed models, efforts and budgets, and a live run with one permitted escalation and one out-of-policy reroute.
4. **#81:** granting a concrete `integrate{target_branch, merge_method}` authority per campaign.
5. **#82:** qualifying more than one executing campaign per repository.

Also operator-gated: deploying each exact build to the preview, authenticated ChatGPT acceptance, and installing the GitHub App.

## Corrections to the 2026-10-10 research

- The handoff's "#69–#82 all open" is out of date. #72, #73 and #74 are closed, and #69, #70, #75, #76, #77 and #78 have source changes merged.
- The #69 scope has narrowed to canonical ordering and graph activity.
- The dependency corrections the handoff proposed still apply. In particular, #81 must not depend on #79, #77 can persist plans without #71, and the #61 live gate and #71 are one qualification checkpoint.
