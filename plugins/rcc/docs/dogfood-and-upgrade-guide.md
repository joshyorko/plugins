# Actions & Robot Bootstrapper: Operator Lifecycle, Dogfood & Upgrade Guide

This guide covers starting, inspecting, dogfooding, backing up, upgrading, and rolling back the `rcc` plugin's bundled Actions & Robot Bootstrapper MCP server (`plugins/rcc/bootstrapper/`).

## 0. Install from the Plugin Release (stdlib Python only)

The release workflow builds a compressed Action package generated from `plugins/rcc/bootstrapper/` and attaches it as `rcc-actions-bootstrapper-0.6.0.zip` when `v0.6.0` is published. That release has not been published from this branch, so its live package URL is not available yet. After publication, install the package and pinned native tools on Linux x86-64 or Windows AMD64 with Python's standard library only:

```bash
python3 plugins/rcc/scripts/install_bootstrapper.py
```

On Windows PowerShell, run `py -3 plugins/rcc/scripts/install_bootstrapper.py`. Setup verifies SHA-256 pins before atomically publishing the package and binaries under the user's RCC Bootstrapper home. The extracted Action package is self-contained; setup does not import the repository source or require Pydantic. Runtime databases, the private Action Server caches, UV cache, and `ROBOCORP_HOME` live under the separate `state/` directory. Existing versioned installs are verified and reused on retry. Setup preserves the caller's `HOME` and `USERPROFILE`.

Before the release asset exists, build the package ZIP from canonical source and qualify the actual installer with pinned artifacts:

```bash
python3 plugins/rcc/scripts/build_bootstrapper_package.py \
  --action-server /path/to/actions-runtime-1.1.1-linux64 \
  --output /tmp/rcc-actions-bootstrapper-0.6.0.zip
python3 plugins/rcc/scripts/dogfood_bootstrapper.py \
  --package-archive /tmp/rcc-actions-bootstrapper-0.6.0.zip \
  --output-receipt /tmp/rcc-bootstrapper-dogfood.json
```

`--package-archive` verifies the generated ZIP and downloads the pinned Runtime and RCC binaries over HTTPS. For a fully offline installer run, pass `--offline-artifact-dir` containing the ZIP plus exact platform-pinned binary filenames.

To start it on Linux, provide a private API key and an approved workspace root:

```bash
export RCC_BOOTSTRAPPER_API_KEY="replace-with-a-locally-generated-secret"
python3 plugins/rcc/scripts/bootstrapper_runtime.py start \
  --approved-root "$PWD/workspace"
```

Use the equivalent `$env:RCC_BOOTSTRAPPER_API_KEY = ...` and `py -3 ...` form in PowerShell. Add `--trusted-root` only for a directory whose code the operator has reviewed. The package and setup support Linux x86-64 and Windows AMD64; other platforms fail before setup writes or downloads anything. Windows execution remains **NOT QUALIFIED** until its native Action Server MCP path passes CI/live dogfood.

---

## 1. Quick Start (`http://127.0.0.1:8765/mcp`)

The `rcc` plugin registers `http://127.0.0.1:8765/mcp` in [`plugins/rcc/mcp.json`](file:///home/kdlocpanda/plugins/plugins/rcc/mcp.json) and [`plugins/rcc/.mcp.json`](file:///home/kdlocpanda/plugins/plugins/rcc/.mcp.json). Start the local Actions Runtime hosting the bootstrapper package with:

```bash
# 1. Inspect local action-server + rcc binaries and SHA-256 digests
python3 plugins/rcc/scripts/bootstrapper_runtime.py doctor

# 2. Start the owned runtime with an API key and an approved workspace root
mkdir -p "$PWD/workspace/trusted"
export RCC_BOOTSTRAPPER_API_KEY="replace-with-a-locally-generated-secret"
python3 plugins/rcc/scripts/bootstrapper_runtime.py start \
  --approved-root "$PWD/workspace" \
  --trusted-root "$PWD/workspace/trusted"

# 3. Check live runtime & /mcp health
python3 plugins/rcc/scripts/bootstrapper_runtime.py status

# 4. Run a fast /mcp protocol smoke check (initialize, tools/list, resources/list, prompts/list)
python3 plugins/rcc/scripts/bootstrapper_runtime.py mcp-smoke

# 5. Stop the owned runtime cleanly
python3 plugins/rcc/scripts/bootstrapper_runtime.py stop
```

### Authenticated Runtime and Native Runtime Features

An API key is mandatory: unauthenticated loopback MCP exposes project execution to any local process. Configure the MCP client to send `Authorization: Bearer <same key>` using its user-level secret store or supported environment-backed header setting. The portable plugin manifest cannot safely carry this secret, so client-side authenticated attachment must be configured separately. The launcher passes the key to the Runtime process; process-level exposure to same-user process inspection is an Actions Runtime contract limitation.

For Codex CLI, one supported explicit attachment is `codex mcp add rcc-bootstrapper --url http://127.0.0.1:8765/mcp --bearer-token-env-var RCC_BOOTSTRAPPER_API_KEY`. This is a user-level Codex MCP registration; installing the plugin manifest alone does not inject the key or qualify authenticated host attachment.

Executable trust is separate from workspace write approval. Add `--trusted-root <path>` only for code whose operator has reviewed it; a tool call's `trust_project` flag cannot add roots. Running requires both an explicit per-call `trust_project=true` and containment within a server-configured trusted root. Without that configuration, scaffold and inspection remain available but execution is refused.

On Linux x86-64, the key also enables Actions Runtime `1.1.1` native Action Canvas MCP tools (`capability_discover`, `canvas_*`, `ui://action-canvas/v1/canvas.html`) and Bearer-authenticated `/api/managed-work-items/v1` endpoints:

Current proof covers Canvas tool discovery/resource read and the managed Work Items schema route only. A Canvas create/apply workflow is **NOT QUALIFIED**; managed Work Items `/capabilities` returned HTTP 422 and no operations, so managed Work Items execution is **NOT QUALIFIED**.

```bash
RCC_BOOTSTRAPPER_API_KEY="local-dev-secret-token" \
  python3 plugins/rcc/scripts/bootstrapper_runtime.py start \
  --approved-root "$PWD/workspace" \
  --trusted-root "$PWD/workspace/trusted"
```

---

## 2. End-to-End Dogfood Workflow (`dogfood_bootstrapper.py`)

[`plugins/rcc/scripts/dogfood_bootstrapper.py`](file:///home/kdlocpanda/plugins/plugins/rcc/scripts/dogfood_bootstrapper.py) runs a complete, self-contained release qualification workflow in isolated temporary directories (`DATADIR`, `ACTIONS_HOME`, `ROBOTS_HOME`, `ROBOCORP_HOME`) without polluting your user home:

1. **Runtime & `/mcp` Handshake**: Starts `action-server` (`1.1.1`) against `plugins/rcc/bootstrapper`, verifies `initialize` → `notifications/initialized`, `tools/list`, `resources/list`, and `prompts/list`.
2. **Template Discovery & Dry-Run Plan**: Calls `bootstrapper_discover` and `bootstrapper_plan` for both an Action package (`workitems-action`) and an RCC Robot (`03-python-work-items`), confirming zero disk mutations during planning.
3. **Atomic Scaffolding & Validation**: Calls `bootstrapper_scaffold` and `bootstrapper_validate` for both projects, confirming non-empty files, valid `package.yaml` v2 / `robot.yaml`, and no `actions.py` shadowing.
4. **Work-Items Producer → Consumer Execution**: Calls `bootstrapper_run` to seed input work items, process them into output work items, and produce structured artifacts (`work_items_report.json`).
5. **Controlled Failure Diagnosis & Retry**: Exercises a controlled non-zero failure run (`simulate_failure=True`), inspects `bootstrapper_results(query="failure_diagnosis")` to verify non-zero exit codes are never swallowed and secrets are redacted, and retries cleanly to `succeeded`.
6. **Runtime 1.1.1 Native Surfaces (Linux `--api-key`)**: Verifies `capability_discover` / `canvas_open` over `/mcp` and `GET /api/managed-work-items/v1/schema`, `GET /api/managed-work-items/v1/capabilities`, and scope-enforced `POST /api/managed-work-items/v1/operations`.

Run the dogfood suite:

```bash
python3 plugins/rcc/scripts/dogfood_bootstrapper.py \
  --package-archive dist/rcc-actions-bootstrapper-0.6.0.zip \
  --output-receipt /tmp/rcc-bootstrapper-dogfood-1.1.1.json
```

### Comparing Baseline (`1.1.0`) vs Candidate (`1.1.1`) Releases

To compare two runtime releases using the same harness:

```bash
# 1. Capture baseline receipt (e.g., against actions-runtime 1.1.0)
python3 plugins/rcc/scripts/dogfood_bootstrapper.py \
  --runtime-binary /path/to/actions-runtime-1.1.0-linux64 \
  --expected-runtime-version 1.1.0 \
  --output-receipt /tmp/dogfood-1.1.0.json

# 2. Run candidate (1.1.1) and emit a structured diff against the baseline receipt
python3 plugins/rcc/scripts/dogfood_bootstrapper.py \
  --runtime-binary /home/linuxbrew/.linuxbrew/bin/action-server \
  --expected-runtime-version 1.1.1 \
  --baseline-receipt /tmp/dogfood-1.1.0.json \
  --output-receipt /tmp/dogfood-1.1.1.json
```

---

## 3. Coordinated SQLite Backup, Upgrade & Rollback

Actions Runtime stores metadata and run history in SQLite databases inside the external datadir (`server.db` and `runs.db`, plus `runs/` artifacts and `bootstrapper_state.db`).

### Before Upgrading `action-server` or `plugins/rcc`

```bash
# Stops the owned runtime if running, acquires the datadir lock, and performs
# a coordinated SQLite online backup of server.db, runs.db, and bootstrapper_state.db
python3 plugins/rcc/scripts/bootstrapper_runtime.py backup \
  --label pre-upgrade-1.1.1
```

Furthermore, whenever `bootstrapper_runtime.py start` detects that `server-info.json` in an existing datadir recorded a different `runtime_version` than the binary being started, it automatically performs a coordinated backup into `<datadir>/backups/` before launching the new binary.

### Rolling Back to a Backup Snapshot

```bash
# 1. Stop the owned runtime
python3 plugins/rcc/scripts/bootstrapper_runtime.py stop

# 2. Restore a specific backup snapshot directory
python3 plugins/rcc/scripts/bootstrapper_runtime.py restore \
  --backup-dir ~/.local/share/rcc-bootstrapper/runtime/backups/<snapshot-name>
```
