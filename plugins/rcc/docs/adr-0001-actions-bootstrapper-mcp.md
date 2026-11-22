# ADR-0001: Bundled Actions & Robot Bootstrapper MCP via Actions Runtime `/mcp`

- **Status**: Accepted
- **Date**: 2026-04-20
- **Issue**: [`joshyorko/plugins#109`](https://github.com/joshyorko/plugins/issues/109)
- **Related Issues**: [`joshyorko/plugins#107`](https://github.com/joshyorko/plugins/issues/107) (Runtime 1.1.0), [`joshyorko/plugins#108`](https://github.com/joshyorko/plugins/issues/108) (Runtime 1.1.1 / Action Canvas / Managed Work Items)

## Context

The legacy standalone repository `joshyorko/actions-robot-boostrapper` (`2769fb7a939af7db6111428dd27153a1a15e4c53`) was created as an early Robocorp Action Server experiment to bootstrap Action packages and RCC robots from chat assistants. A line-by-line audit of its 36 `@action` entrypoints surfaced structural defects that prevent safe or reliable use:

1. **Destructive scaffolding bugs**: `bootstrap_action_package` and `create_robot` invoke `action-server new` or `rcc robot initialize` and immediately overwrite `package.yaml` / `actions.py` / `robot.yaml` / `conda.yaml` / `tasks.py` with `""` (empty strings), leaving broken projects on disk.
2. **Module shadowing**: Scaffolding or editing `actions.py` inside an Action package shadows the `actions` module from `actions-core` (`from actions.args import ActionError, Response, Secret`), breaking runtime imports.
3. **Arbitrary shell execution & silent error swallowing**: `run_shell_command`, `script_in_robot`, `bootstrap_action_package`, and `start_action_server` execute user strings via `subprocess` (`shell=True` in multiple paths), while `_run()` catches `subprocess.CalledProcessError` and returns `Response(result=e.output)` as a successful response.
4. **Secret leakage**: `start_action_server` passes `--action-secrets` as a raw JSON string in `sys.argv[3]` and logs `Command: ...` including plaintext secrets to stdout.
5. **Broken self-management & invalid CLI flags**: `start_action_server` resolves `Path(__file__).parent / "start_action_server.py"` (which does not exist inside `src/actions_bootrapper/`), `stop_action_server` posts unauthenticated `/api/shutdown`, and multiple robot actions call nonexistent RCC subcommands (`rcc create`, `rcc robot init --name`, `rcc robot dependencies`, `rcc task list`).
6. **Split ownership**: Maintaining a separate standalone repository drifted behind the `rcc` plugin skills (`rcc`, `action-server`, `rcc-robots`, `rcc-workitems`, `verify-rcc`) and the modern `joshyorko/actions` runtime (`actions-runtime` `1.1.1`, `actions-core` `1.1.0`, `actions-work-items` `0.5.0`).

At the same time, the `rcc` plugin in `joshyorko/plugins` previously shipped as a **skills-only** package without a bundled bootstrapper Action package or an executable release-dogfood harness that exercises the real Actions Runtime `/mcp` transport end-to-end.

## Decisions

### Decision 1: Canonical Source Ownership Lives in `plugins/rcc/bootstrapper/`

`joshyorko/plugins` (`plugins/rcc/bootstrapper/`) is the **canonical source of truth** for the modernized Actions & Robot bootstrapper Action package (`rcc-actions-bootstrapper`).

- Upstream provenance (`joshyorko/actions-robot-boostrapper@2769fb7a939af7db6111428dd27153a1a15e4c53`) and its Apache-2.0 license are preserved in [`plugins/rcc/bootstrapper/LICENSE`](file:///home/kdlocpanda/plugins/plugins/rcc/bootstrapper/LICENSE) and [`plugins/rcc/bootstrapper/UPSTREAM.md`](file:///home/kdlocpanda/plugins/plugins/rcc/bootstrapper/UPSTREAM.md).
- The standalone `joshyorko/actions-robot-boostrapper` repository is **retired/archived in favor of `plugins/rcc/bootstrapper/`**. No dual-write mirror is maintained; downstream users consume the bootstrapper directly from the `rcc` plugin or point `action-server start --dir` at `plugins/rcc/bootstrapper`.

### Decision 2: Host `/mcp` via the Existing Actions Runtime (`action-server`) as an Opt-In Local Service

We **do not** build a custom Python MCP server or stdio wrapper around `rcc` and `action-server`, and we **do not** auto-register an unauthenticated loopback URL in `plugins/rcc/mcp.json` or `plugins/rcc/.mcp.json`. Instead, the existing Actions Runtime (`action-server start`) hosts the bootstrapper Action package on demand and exposes its native Streamable HTTP `/mcp` endpoint (`http://127.0.0.1:8765/mcp` by default):

- **Opt-in client registration (no always-on plugin MCP entry)**: HTTP MCP entries never auto-start a local daemon and cannot safely embed a per-user Bearer token inside portable/Codex plugin manifests. Auto-registering `http://127.0.0.1:8765/mcp` in `plugins/rcc/mcp.json` / `.mcp.json` caused every unrelated Codex session to fail MCP startup when the local runtime was stopped (or fail with HTTP 403 once `--api-key` was enforced). Operators therefore register the endpoint explicitly (for example via `codex mcp add rcc-bootstrapper --url http://127.0.0.1:8765/mcp --bearer-token-env-var RCC_BOOTSTRAPPER_API_KEY`) and start the runtime on demand (`bootstrapper_runtime.py start` / `serve`) or via the optional Linux `systemd --user` service helper (`bootstrapper_runtime.py systemd-install`).
- All bootstrapper capabilities are authored with standard `actions-core` (`1.1.0`) primitives (`@tool`, `@action`, `@resource`, `@prompt`, Pydantic models, `Response[T]`, `ActionError`, `Secret`, and MCP tool annotations `read_only_hint`, `destructive_hint`, `idempotent_hint`, `open_world_hint`).
- Hosting on Actions Runtime `/mcp` ensures every bootstrapper invocation dogfoods the real runtime's FastMCP v2 transport, OpenAPI schema generation, action invocation pipeline, SQLite run store, and (when started with `--api-key` on Linux) native Action Canvas MCP tools (`capability_discover`, `canvas_*`) and `/api/managed-work-items/v1` endpoints.
- The retained Linux receipt qualifies bootstrapper tools, the Canvas resource read, and managed Work Items schema authentication only. It does **not** qualify a Canvas create/apply workflow or managed Work Items operations (`/capabilities` returned HTTP 422 with zero operations).

### Decision 3: Local Installation, First-Use Behavior, Datadir, Ports, Auth, and Upgrade/Rollback

1. **Installation & Mandatory Fail-Closed Binary Verification**:
   - [`plugins/rcc/scripts/bootstrapper_runtime.py`](file:///home/kdlocpanda/plugins/plugins/rcc/scripts/bootstrapper_runtime.py) manages the local runtime lifecycle (`doctor`, `start`, `serve`, `stop`, `status`, `backup`, `restore`, `mcp-smoke`, and optional Linux `systemd-install` / `systemd-uninstall`).
   - **Fail-Closed Checksum Enforcement Before Execution**: Before executing `action-server` or `rcc`, both the runtime launcher and the bootstrapper core (`policy.py`) compute the binary's SHA-256 digest and compare it against the pinned release matrix ([`capability-and-version-matrix.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/capability-and-version-matrix.md)). Any binary whose SHA-256 digest does not match an approved pinned release digest **fails closed before subprocess launch** (`PolicyViolationError("untrusted_binary_digest", ...)`).
2. **External Per-User Datadir**:
   - Runtime state is stored **outside** the git checkout in a per-user state directory so plugin upgrades never clobber runtime state or dirty the repository:
     - Linux: `${XDG_DATA_HOME:-~/.local/share}/rcc-bootstrapper/runtime`
     - Windows: `%LOCALAPPDATA%\rcc-bootstrapper\runtime`
     - Override: `--datadir <path>` or `RCC_BOOTSTRAPPER_DATADIR`.
   - Isolated test/CI runs set `--datadir`, `ACTIONS_HOME`, `ROBOTS_HOME`, and `ROBOCORP_HOME` to temporary directories so user caches are untouched.
3. **Port & Loopback Binding**:
   - Default bind address is `127.0.0.1` on port `8765` (`http://127.0.0.1:8765/mcp`), matching Actions Runtime's loopback DNS-rebinding allowlist (`127.0.0.1:*`, `localhost:*`, `[::1]:*`).
4. **Authentication and executable trust**: An API key is required for every owned Runtime. `bootstrapper_runtime.py` reads `--api-key` or `RCC_BOOTSTRAPPER_API_KEY` and passes it to `action-server start`; it refuses startup without a non-empty key. The key must be supplied to the MCP client through its user-level secret store or environment-backed header configuration. The portable plugin manifest cannot safely carry a user secret, so authenticated client attachment is host-specific and requires operator setup. The Runtime receives the key in its process arguments, which can be observed by same-user process inspection; it is never written to the receipt or runtime logs. Executable trust is configured separately using `--trusted-root` or `RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS`; approved write roots do not grant execution trust, and the MCP caller's `trust_project` boolean cannot add trusted roots. Execution requires both a server-configured trusted root and explicit per-call confirmation. On Linux with Actions Runtime `1.1.1`, API-key mode also enables native Action Canvas tools and `/api/managed-work-items/v1` Bearer-authenticated routes.
5. **Coordinated SQLite Backup & Upgrade/Rollback**:
   - Before upgrading the runtime version or migrating an existing datadir (`server.db` + `runs.db`), `bootstrapper_runtime.py` stops any owned running server, verifies no active writer holds the datadir lock, and copies `server.db`, `runs.db` (if present), and `server-info.json` into a timestamped backup directory (`<datadir>/backups/<timestamp>-<from_version>-to-<to_version>/`) using SQLite's online backup API (`sqlite3.Connection.backup`) for `server.db` and `runs.db` so WAL/journal state is consistent.
   - Rollback restores the backed-up SQLite files while the runtime is stopped.

### Decision 4: Owned Runtime Instance vs Explicit Attach (Never Mutate Unrelated Processes)

The bootstrapper launcher enforces strict instance ownership via `<datadir>/server-info.json` and `<datadir>/runtime.lock`:

- `server-info.json` records:
  - `instance_id` (UUID4)
  - `pid` and `process_create_time` / `proc_stat_starttime`
  - `host`, `port`, `mcp_url`, `datadir`, `package_dir`
  - `runtime_binary`, `runtime_version`, `runtime_sha256`
  - `auth_mode` (`api-key`; anonymous mode is refused)
  - `trusted_project_roots` (operator-configured executable trust boundary)
  - `started_at`
- **Occupied-port protection**: Before starting on `host:port`, `bootstrapper_runtime.py` probes whether a listener already occupies the port.
  - If the listener belongs to the **same owned instance** (matching `server-info.json` PID, process identity, and datadir), `start` is idempotent and returns the active instance metadata.
  - If the port is occupied by **any other process** (an unrelated service or another `action-server` started outside this datadir), `start` and `stop` **fail closed** with a clear diagnostic rather than killing, reconfiguring, or silently attaching to the foreign listener.
- **Explicit attach**: Callers that intentionally want to inspect or run dogfood checks against an already-running external runtime must pass `--attach-url <http://127.0.0.1:PORT>` explicitly; attach mode never stops or mutates the external server's datadir.
- **Stale metadata reconciliation**: If `server-info.json` references a dead PID or a PID whose OS process creation timestamp/command line no longer matches `action-server`, `status` and `start` mark the prior record stale and reconcile it safely before starting a new owned process.

### Decision 5: Small, Typed, Safe Model-Facing Bootstrapper API

The 36 legacy actions are replaced by **8 typed MCP tools/actions**, **2 read-only MCP resources**, and **1 guided MCP prompt** implemented in [`plugins/rcc/bootstrapper/src/rcc_bootstrapper/bootstrapper_actions.py`](file:///home/kdlocpanda/plugins/plugins/rcc/bootstrapper/src/rcc_bootstrapper/bootstrapper_actions.py):

| Surface | Name | Annotations (`RO` / `Destructive` / `Idempotent` / `OpenWorld`) | Purpose |
| --- | --- | --- | --- |
| `@tool` / `@action` | `bootstrapper_discover` | `True` / `False` / `True` / `False` | Inspect available Action/Robot templates, approved project roots, runtime/RCC tool versions, and capability flags. |
| `@tool` / `@action` | `bootstrapper_plan` | `True` / `False` / `True` / `False` | Dry-run preview of files, SHA-256 digests, and validation commands for a scaffold request without modifying disk. |
| `@tool` / `@action` | `bootstrapper_scaffold` | `False` / `False` / `False` / `False` | Atomically stage and publish a complete, runnable Action (`package.yaml` v2) or Robot (`robot.yaml` + `conda.yaml` + `tasks.py`) project into an empty directory under an approved root. |
| `@tool` / `@action` | `bootstrapper_validate` | `False` / `False` / `True` / `False` | Validate `package.yaml` or `robot.yaml` schema, check Python module imports (including `actions-core` shadowing protection), and optionally preflight tool health. |
| `@tool` / `@action` | `bootstrapper_run` | `False` / `False` / `False` / `False` | Run an approved action or robot task (including work-item producer/consumer batches and controlled failure/retry drills) inside a trusted project root with process-group ownership, timeout enforcement, and secret redaction. |
| `@tool` / `@action` | `bootstrapper_status` | `True` / `False` / `True` / `False` | Query live or crash-reconciled status of a tracked run (`running`, `succeeded`, `failed`, `cancelled`, `timed_out`, `unknown_retained`) or project health. |
| `@tool` / `@action` | `bootstrapper_cancel` | `False` / `True` / `True` / `False` | Cancel an active owned run and reap its process group, distinguishing confirmed termination from `unknown_retained`. |
| `@tool` / `@action` | `bootstrapper_results` | `True` / `False` / `True` / `False` | Execute bounded named queries (`project_summary`, `run_summary`, `work_items_summary`, `failure_diagnosis`) and retrieve redacted logs/artifact metadata. |
| `@resource` | `bootstrapper://catalog/templates` | Read-only resource | JSON catalog of supported Action & Robot templates and their pinned dependencies. |
| `@resource` | `bootstrapper://runs/{run_id}` | Read-only resource | Redacted JSON run record and artifact manifest for a given `run_id`. |
| `@prompt` | `bootstrapper_dogfood_workflow` | Prompt template | Guided multi-step prompt for discovering, planning, scaffolding, validating, running, and inspecting an Action or Robot project. |

### Safety & Reliability Invariants Enforced by the Core

1. **No Arbitrary Shell or Unbounded File Writes**: Neither `run_shell_command` nor unconstrained file overwrite actions exist. Execution is restricted to structured `action-server` / `rcc` / Python entrypoints on validated projects inside `approved_roots`.
2. **Canonical Path Resolution & Traversal Defense**: Every target path is resolved with `Path.resolve()`, checked against canonical `approved_roots`, and verified not to traverse symlinks escaping the root.
3. **Atomic Scaffolding & Non-Empty Directory Refusal**: `bootstrapper_scaffold` refuses any destination that already contains files (including hidden files or `.git`), renders all template files into a sibling staging directory, validates every file is non-empty and syntactically valid, and publishes via atomic `os.replace` / rename with rollback on error.
4. **Secret Isolation & Redaction**: Secrets accept `actions.args.Secret` or secret environment bindings, are injected only via child environment variables or mode-`0600` temporary secret files inside the run directory, are never passed on the command line, and are scrubbed from stdout/stderr/error summaries before persistence or return.
5. **Process-Group Ownership & Honest Crash Reconciliation**: Subprocesses launch in dedicated process groups (`start_new_session=True` on POSIX, `CREATE_NEW_PROCESS_GROUP` on Windows). Timeouts and cancellations terminate the entire process group. If the bootstrapper or runtime restarts while a run was marked `running` and the PID is no longer verifiably owned, the run transitions to `unknown_retained` (never falsely reported as `succeeded` or cleanly `cancelled`).

## Consequences

- **Positive**:
  - The `rcc` plugin provides both expert skills and an opt-in bundled bootstrapper Action package backed by Actions Runtime `1.1.1` without causing MCP startup failures on unrelated sessions where the local runtime is not running.
  - Release qualification (`plugins/rcc/scripts/dogfood_bootstrapper.py`) tests the real `/mcp` streamable HTTP transport, tool/resource/prompt discovery, Action and Robot scaffolding, work-item producer/consumer execution, failure diagnosis/retry, and Runtime `1.1.0` vs `1.1.1` comparison.
  - All 36 legacy actions have an explicit disposition in [`legacy-action-inventory.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/legacy-action-inventory.md) with unit regression tests preventing recurrence of the legacy bugs.
- **Boundaries & Non-Goals**:
  - Action Canvas interactive workflows remain owned by `action-server` skill guidance (`#108`); we do not create a duplicate `action-canvas` skill.
  - Unverified client/platform combinations (e.g., ChatGPT Developer Mode E1 public tunnel, macOS plugin runtime qualification, Windows native Canvas storage) remain explicitly gated as `NOT QUALIFIED` in [`capability-and-version-matrix.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/capability-and-version-matrix.md).
