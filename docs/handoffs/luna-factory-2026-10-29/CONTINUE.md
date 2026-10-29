# Finish Luna Factory live execution

Continue work in https://github.com/joshyorko/plugins. The owner's goal is a fully
operational Rust Luna Factory runtime, not only a deployed planning/status service.
PR #91 is already merged. Finish the remaining execution bring-up without weakening
qualification, fabricating evidence, copying credentials, or disturbing existing work.

## Start here

1. Read the applicable AGENTS.md and map your checkout before editing.
2. Fetch the latest `luna-factory/qualification-gate` branch and `main`. This branch
   contains the final handoff on top of merged main. Do not redo the recovery or UX.
3. Read `docs/handoffs/luna-factory-2026-10-25/completion-evidence.md`,
   `plugins/luna-factory/docs/execution-qualification.md`,
   `plugins/luna-factory/docs/control-wire.md`,
   `plugins/luna-factory/docs/container-deployment.md`, and issue #71.
4. Keep an execution plan. Use one tracked follow-up branch/PR for the remaining
   execution work. Native subagents are authorized where useful, with exclusive
   write scopes. Push coherent checkpoints regularly, including clearly labelled
   unfinished checkpoints; do not wait for perfect tests before preserving work.

## Exact handoff state

- PR: https://github.com/joshyorko/plugins/pull/91, MERGED.
- Merge commit on main: `e907d97c353570eba2adf8040131a48d81ea2c6a`.
- Tested and deployed PR head: `c0c3c0ed3c4d7dc62a8d3cf1381b0802fb574c1a`.
  Its tree is identical to the merge commit.
- Image: `localhost/luna-factory@sha256:d49d47c2685e89c52730d0118bf6f399dd6787fc3ca2f7fa485cdaf1b6ec05fc`.
- Live Dakota endpoint: `http://127.0.0.1:18789/mcp`.
- Service: `luna-factory-dogfood-luna-factory-1`, healthy, restart unless stopped.
- All hosted checks passed on the deployed head: Rust, UI, Linux, and Windows.
- All this team's implementation work is committed and pushed. There are no
  active implementation workers. The original workstation checkout had unrelated
  dirty work and sync-conflict files before this task; those were preserved.

The runtime has one saved planning run and a three-node graph at revision 10.
The run ID and complete stored payload survived deployment unchanged. The canary
repository remains approved. The actual active planning configuration is preserved;
only the intended native transport fields changed. Do not replace it with the empty
base operator.json. Discover the live container's actual --config argument first.

## What is complete

- Qualification guards cover ordinary effects, queued WebSocket writes, active and
  late workers after revocation, internal effect APIs, strong executable hashing,
  private state boundaries, and monotonic expiry observations including list/revoke.
- Clone/publication uses pinned directories, identity-safe cleanup, no-clobber
  publication, descendant/stderr deadlines, final size checks and restart recovery.
  Sampled aggregate size is not a disk quota.
- All five approved UX slices are implemented and independently accepted. The
  current 35-image manifest is under
  `plugins/luna-factory/ui/test/dogfood-visual-snapshots/manifest.json`.
- UI source SHA-256:
  `c3c5c1b0cc99bc7986ddadf2166ccf0df20e65fa2d9bc42fd77704e6ac81cdfa`.
- UI bundle SHA-256:
  `55337e0a686a958ecd7e8ce102c96c0b4ce807eb6a5ba6e50eb0af96b7a9b56a`.
- UI typecheck/build passed; Vitest passed 282 tests with one opt-in integration
  skipped. That compiled Rust/SQLite/HTTP workbench integration ran separately and
  passed, including an atomic nullable-title update with zero native dispatch.
- The 35 captures passed with zero automatic messages, mutation calls, or clipped
  controls. Card geometry is checked at 16px and 20px host text sizes. These are
  synthetic-host captures, not authenticated ChatGPT Desktop acceptance.
- Repository checks passed 79 tests; Python/package checks passed 81.

Preserve the atomic task-edit contract. `update_task.title: null` keeps the exact
server canonical title; a string explicitly replaces it. `target_id: null` clears
the preference. Display titles remain bounded. Safe canonical editor text is
optional; null withholds filtered text. Do not replace hidden or long titles with
a display placeholder/prefix. New-format proposals require a compatible runtime.

## Why real jobs still cannot run

These are implementation and qualification gaps, not merely missing permission:

1. **Trusted native identity is not supported on the real daemon response.**
   Dakota runs Codex/app-server 0.165.0. A real initialization returns userAgent,
   codexHome, platformFamily and platformOs, but no serverInfo. The current parser
   in `server/src/native.rs::provider_identity` expects serverInfo.
   Passive connection and model/catalog reads work. `current_provider_identity`
   and qualification recording still refuse an unverified identity. Do not treat
   client-influenced user-agent text as independently verified identity or relax
   the grant/effect guards to get a green result. Implement a supported, reviewed
   identity binding using the actual protocol/environment.
2. **Plan promotion is still a stub.**
   `server/src/lifecycle.rs::Factory::promote_campaign` validates then returns the
   static promotion blocker. A grant alone does not enable it. Implement promotion
   as a new executable run bound to one approved plan revision; never convert the
   immutable planning run or silently adopt existing native workers. Preserve
   source, authority, idempotency, acceptance, ownership and revision fences.
3. **Host/container execution paths need qualification.**
   The OCI service talks to the existing host daemon over a mounted Unix socket.
   Verify that actual worktree, repository, skill and canary paths mean the same
   thing to both processes. Do not assume an image-only /opt or /var/lib path is
   visible to Dakota's app-server. Use narrow mounts and the supported transport;
   do not mount the whole Codex home or copy credentials.
4. **No real execution grant or live canary was run.**
   Both configured profiles currently report execution_not_qualified. The canary
   is implemented and fixture-tested, but current cessation evidence explicitly
   covers native threads/descendants/reported terminals, not an independently
   verified OS process tree. Preserve that distinction. Full #71 also requires
   authentication/subscription evidence, adapter start/observe/steer/stop/reconcile,
   claim lifecycle, callback ownership, crash windows, and cessation proof.

Prepare the source fixes, relevant regressions, independent review and a concrete
bounded disposable canary plan before requesting any remaining operator-only
confirmation. No current authority permits bypassing the CLI's operator context,
editing a real qualification database, substituting fixture evidence, changing
credentials, or starting live inference without the required explicit approval.

## Live connections and operator boundaries

The Rust service already connects to Dakota's existing Codex 0.165.0 daemon through
`existing_daemon`. Initialization and the Luna low-effort catalog probe passed
with zero runtime inference. No competing app-server was started.

The existing Codex MCP entry is enabled. Hot reload worked in the ongoing host
session, which successfully called the runtime capability tool and sees 23 model
tools. OMP 18.13.3 was tested in a separate isolated no-session process: the same
PID ran `/mcp reload`, `/mcp test luna-factory`, and a local catalog command. It
connected and exposed 32 active tools without a prompt or inference. The smoke
made one failed local llama.cpp catalog request, not an inference request.

OMP does not honor `_meta.ui.visibility` and exposes the nine app-only tools too.
Do not call this correct visibility filtering. For an existing OMP chat, let an
in-flight MCP call finish, then reload/test the server; no chat restart is needed.
If that profile has no entry, the supported command is:

`/mcp add luna-factory --scope user --url http://127.0.0.1:18789/mcp --transport http`

Native skill orchestration is separate from Rust-managed execution. Native
subagents making progress do not prove the Rust factory executes jobs. Do not
duplicate or silently adopt an existing native task.

On Dakota only, the durable deployment directory is
`${XDG_STATE_HOME:-$HOME/.local/state}/luna-factory-oci/deployments/c0c3c0ed3c4d`.
Its `run.sh` accepts status/up/down. `evidence/` retains private runtime and OMP
receipts; the prior image/configuration and online SQLite backup are retained.
`rollback.sh` refuses new-format proposals or execution records requiring review.
Do not restore an old backup over newer plans. Private configs, state, credentials
and raw workstation evidence are not published in Git.

A remote agent must be able to do source work from this repository without those
private paths, caches, services or credentials. Live verification needs the
available authorized Dakota/operator environment; report the boundary honestly.

## Finish standard

Use repo-native pinned container tooling. Serialize heavy Rust builds and browser
captures; run Vitest with one worker. Reuse unchanged test evidence and do not
restart the UX exercise. Run relevant changed-path checks and exact-head CI for
the execution fixes, then verify the actual deployed artifact and intended host.

Report exact source/deployed SHAs and image digest, local versus CI versus live
proof, identity and qualification status, the bounded real-job result, cessation
and claim state, and the exact remaining operator step. Do not mark the factory
fully operational merely because the container is healthy or tools are listed.

PR #91's merge is already authorized and complete. Keep future repository writes,
merges, releases and live actions within their explicit scope. Do not create new
issues or change unrelated repositories. The owner's latest instruction for this
handoff was to push all work immediately even if tests were red; no additional
checks were required before publishing this handoff.
