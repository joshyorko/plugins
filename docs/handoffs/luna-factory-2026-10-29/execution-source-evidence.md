# Execution source verification, draft PR #104

PR: https://github.com/joshyorko/plugins/pull/104. No merge, release, installation,
deployment or real native inference was performed.

The implementation/test source checkpoint is
`1e739aff2e91f7803aaa83408a0caa41fefc3b09`. Subsequent handoff-only commits retain
these implementation bytes. Exact final PR-head hosted checks must be inspected
on the PR; this document does not treat a prior head's checks as final-head proof.

## Source result

- Linux Codex 0.165.0-compatible identity can use the connected peer's kernel UID,
  SO_PEERPIDFD, procfs/PID namespace and executable SHA-256. Client-controlled
  initialize userAgent/home/platform fields never attest identity. Every guarded
  first-write poll rechecks the initialized kernel binding. The existing
  serverInfo metadata parser remains for versions reporting it.
- Explicit qualified promotion creates one new executable run from an approved
  plan revision. Its approval lineage carries campaign/plan IDs, revision, source
  subject and snapshot digest. An IMMEDIATE transaction links run, claim, campaign
  and replay row; it never rewrites the planning run. Effective execution capacity
  is checked before admission. Exact replay, concurrent identical admission,
  stale revision, link failure rollback, canonical task edits and ready-leaf
  selection have disposable SQLite fixtures.
- Explicit `native_paths` are qualification-profile-bound. Factory validates local
  files while native frames carry mapped daemon CWD/skill/subject-CLI paths. The
  two-namespace read-only probe compares repository and required binary/skill
  content. It creates no grant and contacts no daemon.
- Nullable canonical task-title/target editing is unchanged. UI source changes
  only the promotion eligibility parser predicate. The original 35-image manifest
  and all PNGs are unchanged. A separate nonvisual compatibility receipt binds the
  current source/bundle; a test reconstructs the captured source hash by reversing
  that exact predicate. No browser recapture or authenticated-host acceptance is
  claimed.

## Local execution evidence

Tests ran in a disposable cloud tooling container on the pinned repository Rust
image `docker.io/library/rust:1.99.0-slim-trixie@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581`.
The UI used pinned
`docker.io/library/node:24.11.1-bookworm-slim@sha256:48abc13a19400ca3985071e287bd405a1d99306770eb81d61202fb6b65cf0b57`.
These are tooling images, not a new Luna deployment image. Rust builds were
serialized; Vitest used one worker. No captures were generated.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | passed |
| `cargo clippy --locked --all-targets -- -D warnings` | passed |
| `cargo test --locked -- --test-threads=2` | 344 passed, 0 failed, 2 live tests ignored |
| UI typecheck / production build | passed |
| Vitest `--maxWorkers=1` | 282 passed; one opt-in compiled integration skipped in unit suite |
| Compiled graph/workbench integration | separately passed, 1/1, real local Rust/SQLite/HTTP and synthetic AppBridge; zero native dispatch |
| Python/package tests | 84 passed |
| `bin/check` | 79 passed, 10 existing environment-specific skips; repository structure validated |
| Working tree / whitespace | clean at source checkpoint; `git diff --check` passed |

The local debug binary SHA-256 was
`a0d8a39c39a41239426ef62c6d25f77175f418cdd2bd7b47f088e516a750954e`.
This is a disposable test binary, not a deployed artifact.

Current UI source SHA-256:
`1a0e925dab392435be0470f097a0a7be928435e5bea672d596b0702a2cc8ef1a`.
Current bundle SHA-256:
`7a4eadb336746ba222abdcce93220ec58eb7cdbf28cc2ab5e2975a6893601abf`.
Historical capture hashes remain in the original manifest.

Earlier checkpoint failures were repaired: forged campaign links now require their
atomic promotion record; namespace paths are validated on the correct side;
requested capacity is checked before admission; exact concurrent replay is handled;
the MCP test now accepts boolean eligibility. Local tooling setup needed the
provided CA, repository-scoped Git trust for the mounted checkout, and pinned Node
in the Rust test container. These were fixture/tooling changes, not live changes.

## Independent source acceptance

A fresh read-only Luna High reviewer inspected `dad48772245effb4078053070f81f287fce94bae`.
The reviewer identified the capacity and concurrent-replay findings, verified both
fixes, and reported no remaining P0-P2 finding in the bounded review. Source and UI
implementation bytes are identical between that checkpoint and `1e739aff...`;
subsequent changes add the concurrent regression and update the MCP test. The
reviewer ran no builds or live checks; the writer separately verified execution.
Requested route was Luna High; effective model/effort was not exposed by metadata.
No Astra or additional implementation writer was used.

## Deployment and actual-execution boundary

The following remain unverified and were not attempted in cloud:

- Access to Dakota's services, active config, private state, actual workers or
  qualification database. Existing services/config/data/work were not changed.
- A new deployed source SHA, image digest or actual endpoint identity. Handoff
  reports the existing deployed planning/status head as
  `c0c3c0ed3c4d7dc62a8d3cf1381b0802fb574c1a`, merge
  `e907d97c353570eba2adf8040131a48d81ea2c6a`, and image
  `localhost/luna-factory@sha256:d49d47c2685e89c52730d0118bf6f399dd6787fc3ca2f7fa485cdaf1b6ec05fc`.
  Those values were not independently observed from this container.
- Host PID and path visibility. The default OCI private PID namespace cannot
  attest a host daemon through this kernel mechanism. Mappings alone do not fix it.
- Real account/subscription entitlement, runtime routing, adapter lifecycle,
  callback ownership, intent/send/ack crash windows, actual managed claim lifecycle
  and native canary/job results. Fixture grants and transports prove none of these.
- Independently verified OS process-tree cessation. Existing cessation evidence
  remains scoped to native owner/descendants/reported terminals, explicitly
  `process_tree: "unverified"`.

The next operator action is to review the exact PR artifact and choose/authorize a
compatible same-namespace process placement or isolation change, preserving the
existing graph/config/ledger. Then compare paths on the actual daemon host, verify
sanitized authentication/subscription evidence, and explicitly authorize the
capacity-1 disposable qualification turn through the operator CLI. Later ordinary
grant/promotion, rollout, and any destructive crash exercise require their own
explicit authority. See `plugins/luna-factory/docs/execution-canary-runbook.md`.
No live action was denied or left with an uncertain outcome; these actions were
not attempted because the required environment and explicit authority are absent.
The Factory is not claimed operational.
