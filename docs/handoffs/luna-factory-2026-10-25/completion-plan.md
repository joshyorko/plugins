# Issue #102 completion plan

Goal: finish the approved backend repairs and five UX slices on
`luna-factory/qualification-gate`, updating draft PR #91.

The current owner request authorizes a team and supersedes the archived
single-agent restriction. Keep one integration branch and one PR. Preserve the
uncommitted original checkout and its sync-conflict files.

## Acceptance and ownership

| Task | Required result | Owner | State |
| --- | --- | --- | --- |
| A1 | Compile, fail closed at every effect path, revoke active work, protect qualification writes and exact identities, persist clock high-water mark | qualification worker | final validation and review |
| A2 | Descriptor-pinned clone/publication/recovery, cancellation of descendants, final size checks, documented aggregate limit | workspace security worker | focused tests and independent review passed; integrated checks pending |
| A3 | Tokens, shell, header | UX worker, sequential slice 1 | running |
| A4 | Plain language and banned-term regression | UX worker, sequential slice 2 | pending A3 |
| A5 | Home composer and repository popover | UX worker, sequential slice 3 | pending A4 |
| A6 | Plan, task drawer, edit/proposal review | UX worker, sequential slice 4 | pending A5 |
| A7 | Activity, inline card, states and mobile | UX worker, sequential slice 5 | pending A6 |
| A8 | Package/docs/catalog parity and portable one-command deployment flow | parent | running |
| A9 | Fresh independent review and exact-head source, test and CI evidence | parent and independent reviewer | pending integration |
| A10 | Relaunch the validated Rust OCI service with retained state, verify MCP connection, keep execution denied without an operator grant | parent | immediate priority, pending backend checks/review only |

## Execution and verification

1. Use the fetched PR head `1bd2e66210fb9585dfa44ed93810c55d6124a3c9`
   in an isolated checkout of the existing branch. Apply the already-approved
   design rather than repeat design exploration.
2. Give workers exclusive write scopes. Serialize resource-heavy Rust commands
   and browser captures. Use the pinned project Rust image, two Cargo jobs,
   bounded container memory, and one Vitest worker.
3. Repair concrete failures with meaningful regression coverage. Preserve
   qualification boundaries, containment, planning, server authority, revision
   fencing, and truthful activity.
4. Run Rust formatting, locked Clippy on all targets with warnings denied,
   locked full tests, Python suites, UI typecheck/tests/build, screenshot
   captures, bundle parity and `bin/check`. Inspect changed light, dark, narrow
   and mobile captures. Record command/runtime/subject and full diagnostics.
5. Review the integrated security and UX acceptance. Resolve findings, commit
   coherent checkpoints to the same branch, push PR #91, and verify CI against
   the exact final SHA. Keep the PR draft unless the owner asks otherwise.

## Authority and limits

The owner's latest directive makes safe backend deployment and the intended OMP
connection the immediate priority. Preserve unfinished UI changes, deploy the
validated backend with the currently shipped UI, and resume the five UX slices
after runtime verification. Use a clean detached build checkout of the same
branch commit when active UI edits prevent a clean image build; this is a build
input, not another implementation branch or PR.

The owner's follow-up authorizes getting the OCI service running after the code
is ready. Verify and back up the existing private state before replacement;
preserve the previous image and configuration for rollback. Enabling the
existing Codex MCP connection does not qualify execution or adopt another
session's native workers. Do not restart unrelated Codex sessions.

No real qualification writes, credential or GitHub App permission changes,
live inference, merge, or release. The source
may implement and test a qualification canary using disposable fixtures; real
runtime qualification requires explicit operator authority. Local checks and
synthetic screenshots do not prove deployed or live ChatGPT acceptance.

## Review focus

- Revocation during blocked native writes and active turns; no post-revoke workers.
- Untrusted qualification files, TTY/sandbox checks, native identity, clock rollback.
- Root/ancestor replacement, publication collisions, partial recovery and clone descendants.
- Scope/capacity denial without admissions or native effects; containment remains available.
- Composer and edit flow, stale proposals, empty/offline/error states, keyboard and mobile layout.

Deferred: changes outside issue #102 and live execution qualification/ChatGPT
acceptance without the required explicit operator authority.
