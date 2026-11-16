# Actions & Robot Bootstrapper: Operator Lifecycle, Dogfood & Upgrade Guide

This guide covers starting, inspecting, dogfooding, backing up, upgrading, and rolling back the `rcc` plugin's bundled Actions & Robot Bootstrapper MCP server (`plugins/rcc/bootstrapper/`).

---

## 1. Quick Start (`http://127.0.0.1:8765/mcp`)

The `rcc` plugin registers `http://127.0.0.1:8765/mcp` in [`plugins/rcc/mcp.json`](file:///home/kdlocpanda/plugins/plugins/rcc/mcp.json) and [`plugins/rcc/.mcp.json`](file:///home/kdlocpanda/plugins/plugins/rcc/.mcp.json). Start the local Actions Runtime hosting the bootstrapper package with:

```bash
# 1. Inspect local action-server + rcc binaries and SHA-256 digests
python3 plugins/rcc/scripts/bootstrapper_runtime.py doctor

# 2. Start the owned runtime on 127.0.0.1:8765 with an approved workspace root
python3 plugins/rcc/scripts/bootstrapper_runtime.py start \
  --approved-root "$PWD/workspace" \
  --verify-sha256

# 3. Check live runtime & /mcp health
python3 plugins/rcc/scripts/bootstrapper_runtime.py status

# 4. Run a fast /mcp protocol smoke check (initialize, tools/list, resources/list, prompts/list)
python3 plugins/rcc/scripts/bootstrapper_runtime.py mcp-smoke

# 5. Stop the owned runtime cleanly
python3 plugins/rcc/scripts/bootstrapper_runtime.py stop
```

### Enabling Native Action Canvas & Managed Work Items (`--api-key` Mode on Linux)

When started with `--api-key` (or `RCC_BOOTSTRAPPER_API_KEY`) on Linux x86-64, Actions Runtime `1.1.1` additionally mounts its native Action Canvas MCP tools (`capability_discover`, `canvas_*`, `ui://action-canvas/v1/canvas.html`) and Bearer-authenticated `/api/managed-work-items/v1` endpoints:

```bash
RCC_BOOTSTRAPPER_API_KEY="local-dev-secret-token" \
  python3 plugins/rcc/scripts/bootstrapper_runtime.py start \
  --approved-root "$PWD/workspace" \
  --verify-sha256
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
