# Shared mockup fixture: `joshyorko/actions` campaign #101

Illustrative data only. Every mockup in every direction uses this same campaign, so
differences between directions come from the design, not the data.

## Campaign

- Repository: `joshyorko/actions`
- Parent issue: **#101 Actions v2: uv packaging and cross-platform CI**
- Started by: "Use Luna Factory to finish the issue graph in joshyorko/actions. Parent issue #101. YOLO it."
- Finish authority: Open PRs (Luna cannot merge, release, or deploy)
- Started 2h 14m ago. Time budget 6h. Worker capacity 4.
- Other campaigns: `joshyorko/plugins` #58 "Luna Factory OCI packaging" (running, 1 worker, healthy),
  `joshyorko/omarchy-bootc` #23 "Update and transition correctness" (paused, waiting on you),
  `joshyorko/fizzy` #12 "Cards export" (finished yesterday, 4 PRs merged)

## Agents

| Agent | Role | Current work | Last meaningful activity |
| --- | --- | --- | --- |
| Luna | coordinator | Owns #101, schedules work, gates merges | 2m ago: queued repair for #109 |
| Worker 1 | worker | idle, released after #105 | 41m ago: opened PR #147 |
| Worker 2 | worker | #106 Migrate google-sheets pack | 4m ago: 12/14 tests passing locally |
| Worker 3 | worker | #107 Migrate slack pack | 9m ago: pushed 3 commits to `luna/107-slack` |
| Worker 4 | worker | #109 Cache RCC holotree in CI (repair 1 of 2) | 2m ago: `test (windows)` failed, repairing |
| CI triage | subagent of Worker 4 | Reading the Windows job log | 1m ago |

## Issue graph (13 children, 4 waves)

| # | Title | Depends on | State | Owner | PR / checks |
| --- | --- | --- | --- | --- | --- |
| 102 | Add uv lockfile support to package builder | none | merged | Worker 1 | #140 merged |
| 103 | Extract shared CI workflow | none | merged | Worker 2 | #141 merged |
| 104 | Pin RCC 18.19 in devcontainer | none | merged | Worker 3 | #142 merged |
| 105 | Migrate browser action pack | 102 | PR ready | Worker 1 | #147, 6/6 checks passing |
| 106 | Migrate google-sheets pack | 102 | working | Worker 2 | no PR yet |
| 107 | Migrate slack pack | 102 | working | Worker 3 | draft #149, 2/6 checks |
| 108 | Matrix CI for linux/mac/win | 103 | blocked (Luna handling) | none | macos-14 runner queue is 45m, retry at 14:30 |
| 109 | Cache RCC holotree in CI | 103, 104 | repairing | Worker 4 | #148, 5/6 checks (windows failed) |
| 110 | Signed release artifacts | 108 | waiting | none | none |
| 111 | Update docs for uv workflow | 105, 106, 107 | waiting | none | none |
| 112 | Remove legacy conda.yaml path | 105, 106, 107 | waiting | none | none |
| 113 | Release workflow OIDC publishing | 103 | **needs you** | none | Touches protected `.github/workflows/release.yml` (`id-token: write`) |
| 114 | Release v2.0.0 | 110, 111, 112, 113 | waiting | none | none |

Totals: 3 merged, 1 PR ready for your review, 3 in progress (one repairing), 1 blocked
(Luna is retrying), 1 needs your decision, 4 waiting on dependencies.

## Attention

1. **Decision:** #113 needs `id-token: write` in the protected `release.yml`. Options: allow it
   for this PR only, skip #113 and leave release publishing manual, or "I'll do it myself".
2. **Review (not urgent):** PR #147 is ready, 6/6 checks passing, +412 −1,108 across 23 files.
   Luna can't merge it because the finish authority is "Open PRs".
3. **Not attention (Luna is handling):** #108 runner queue, #109 repair 1 of 2.

## Recent meaningful events

- 14:06 Worker 4: `test (windows)` failed on PR #148. Repair 1 of 2 started.
- 14:04 Worker 2: 12 of 14 tests passing locally for google-sheets.
- 13:59 PR #147: all 6 checks passing.
- 13:55 Worker 3: pushed 3 commits to `luna/107-slack`.
- 13:41 Worker 1 opened PR #147 (#105) and was released.
- 13:30 Luna paused #113: protected file `.github/workflows/release.yml`.
- 13:12 #108 blocked: no macos-14 runner available. Luna will retry at 14:30.
- 12:58 PR #142 merged (#104).
