# Python Library Audit

Use this reference when improving RCC-family skill examples for Python libraries, package pins, or source-backed recipes. It captures the 2026-05-23 audit across official docs, Josh repositories, upstream Robocorp/Sema4AI repositories, and public Robocorp runtime/process examples.

Recheck sources before making "latest", version-current, or default-template claims. Treat Josh-owned RCC sources as current stack evidence, and upstream Robocorp/Sema4AI docs as package/API evidence unless the task explicitly targets their hosted product. Control Room backend architecture must stay marked as inference when derived from public clients or examples instead of backend source.

## Source Snapshot

Official docs and source repos checked:

- Sema4AI actions docs: `https://sema4.ai/docs/build-agents/develop-actions/action-structure`
- Sema4AI Python action/MCP source: `https://github.com/Sema4AI/actions` at `master` / `9d9479e`
- Robocorp Python library docs: `https://sema4.ai/docs/automation/python/robocorp`
- Robocorp Python library source: `https://github.com/robocorp/robocorp` at `master` / `64cc9a3`
- Josh Action Server fork: `https://github.com/joshyorko/actions` at `community` / `3bae23bb49fe`
- Josh RCC fork: `https://github.com/joshyorko/rcc` at `main` / `59011b848ee1`
- Sema4AI gallery: `https://github.com/Sema4AI/gallery` at `main` / `d6afd61`
- Public Robocorp runtime/process examples: `k8s-on-demand-runtimes`, `azure-on-demand-runtimes`, `example-worker-logs`, `template-python-workitems`, and `python-producer-consumer-reporting`.

Useful org-level repos found during the scan:

- Sema4AI: `actions`, `gallery`, `cookbook`, `vscode-extension`.
- Robocorp: `robocorp`, `rpaframework`, `template-python`, `template-python-browser`, `template-python-workitems`, `template-python-assistant-ai`, `template-action`, `example-action-server-starter`, `example-advanced-python-template`, `python-producer-consumer-reporting`, `example-web-store-work-items`, `example-asset-storage`, `example-using-vault`, `example-encrypt-workitems`, `example-langchain-data-ingestion`.
- Josh: `actions`, `rcc`, `robot-templates`, `robocorp`, `gallery`, `robocorp_adapters_custom`, `fetch-repos-bot`, `fizzy-symphony`, `linkedin-easy-apply`, `yo-dawg`, `home-lab-actions`, `actions-robot-boostrapper`, `context-eng-copilot`, `rccremote-docker`, `prefect-setup`, `cook-with-gas-rpa-challenge`.

## Library Map

| Surface                | Libraries                                                                                                                    | Use for                                                                                                 | Example stance                                                                                                                                                                                                       |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Action packages        | `sema4ai-actions`, `sema4ai-mcp`, `sema4ai-data`                                                                             | `@action`, direct MCP tools, named data queries, OpenAPI/MCP exposure                                   | Keep `package.yaml` v2, typed inputs, `Response[T]`, `ActionError`, `Secret`, `OAuth2Secret`, `SecretSpec`, `Request`, and `Table` examples current.                                                                 |
| RCC robot projects     | `robocorp`, `robocorp-tasks`, `robocorp-log`, `robocorp-browser`, `robocorp-workitems`, `robocorp-vault`, `robocorp-storage` | Contained automation run through `robot.yaml` and `conda.yaml`                                          | Keep literal package/API names; RCC owns the environment boundary.                                                                                                                                                   |
| Browser automation     | `robocorp-browser`                                                                                                           | Playwright-backed browser tasks in RCC projects or action packages                                      | Configure before `browser.page()`; use isolated install/post-install for CI; persistent contexts are single-run-sensitive.                                                                                           |
| Work item queues       | `robocorp-workitems`, `actions-work-items`, `robocorp-adapters-custom`                                                       | Producer/consumer/reporter flows, Action Server/non-robot queues, file/SQLite/Redis/DocumentDB adapters | Use `robocorp.workitems` for modern RCC robot flows, `actions-work-items` for Action Server/non-robot flows, and `robocorp-adapters-custom` for production-proven custom adapters such as DocDB-backed robot queues. |
| Secrets and assets     | `robocorp-vault`, `robocorp-storage`, `robocorp-log`                                                                         | Control Room secrets, assets, safe logging                                                              | Show mock vault only for local development; hide sensitive values with `robocorp.log`; never put real values in committed `devdata`.                                                                                 |
| RPA Framework and HITL | `rpaframework`, `rpaframework-assistant`, Robot Framework libraries                                                          | Legacy `RPA.*` robots, Python-callable RPA utilities, Assistant desktop dialogs                         | Prefer modern `robocorp.*` libraries for new RCC Python robots. Use `RPA.Assistant` through `rpaframework-assistant` when a desktop/HITL dialog is intentionally part of the workflow.                               |

## Coverage Notes To Keep Current

- `robocorp-storage`: asset examples below cover `list_assets`, `get_text`, `get_json`, `get_file`, `get_bytes`, `set_text`, `set_json`, `set_file`, `set_bytes`, and `delete_asset`; recheck the API before adding new asset types.
- `robocorp-vault`: local FileSecrets development guidance and `robocorp.log.suppress_variables()` examples are included below for secret rotation or credential generation tasks.
- `robocorp-log`: `process_snapshot()` and suppression examples are included below for debugging long-running robots without leaking secrets.
- `sema4ai.actions`: broaden examples beyond basic `@action` to include `Request`, `Table`, `SecretSpec`, `setup`, `teardown`, and explicit `ActionError` handling through `Response(error=...)`.
- `sema4ai.mcp`: keep annotation hints visible: `read_only_hint`, `destructive_hint`, `idempotent_hint`, and `open_world_hint`.
- `robocorp-browser`: browser examples below cover `configure_context(...)`, persistent-context constraints, and `browser.configure(install=True, isolated=True)` versus `python -m robocorp.browser install ... --isolated`.
- `actions-work-items`: use Josh's `workflow-producer-consumer` template as community-branch evidence when documenting action-package producer/consumer flows.

## Reference Examples

### Action With Typed Result And Expected Error

```python
from sema4ai.actions import ActionError, Response, action


@action(is_consequential=False)
def normalize_order(order_id: str) -> Response[dict[str, str]]:
    if not order_id.strip():
        raise ActionError("order_id is required")
    return Response(result={"order_id": order_id.strip().upper()})
```

Use `ActionError` for expected user/data failures. Unexpected exceptions should remain real failures unless the action contract explicitly returns a typed error payload.

### Request-Aware Action

```python
from sema4ai.actions import Request, Response, action


@action
def caller_info(request: Request) -> Response[dict[str, str | None]]:
    return Response(result={"request_id": request.headers.get("x-request-id")})
```

Use `Request` only when action behavior needs invocation metadata. Keep normal business inputs as typed function parameters.

### Data Query Shape

```python
from sema4ai.actions import Response, Table
from sema4ai.data import get_connection, query


@query
def recent_orders() -> Response[Table]:
    result_set = get_connection().query("select id, status from orders limit 20")
    return Response(result=result_set.to_table())
```

Prefer `Response[Table]` for named data queries so tabular results stay structured instead of becoming large plain strings.

### Direct MCP Tool

```python
from sema4ai.mcp import tool


@tool(read_only_hint=True, open_world_hint=False)
def add(a: int, b: int) -> int:
    return a + b
```

Use direct `sema4ai.mcp` only when the package intentionally defines MCP tools/resources/prompts. Ordinary `@action` packages are exposed by Action Server through `/mcp`.

### Robot Task With Setup And Output Directory

```python
from pathlib import Path

from robocorp.tasks import get_output_dir, setup, task


@setup(scope="session")
def prepare_output(tasks):
    Path(get_output_dir()).mkdir(parents=True, exist_ok=True)


@task
def write_report() -> None:
    Path(get_output_dir(), "report.txt").write_text("ok", encoding="utf-8")
```

Use `get_output_dir()` instead of hardcoded `output/` when task code needs the RCC/runtime artifact directory.

### Browser With Isolated Install And Persistent Context

Install browsers during environment creation when the project needs deterministic CI or reusable holotree/browser caches:

```bash
python -m robocorp.browser install chromium --isolated
```

```python
from robocorp import browser
from robocorp.tasks import task


browser.configure(
    browser_engine="chromium",
    headless=True,
    install=True,
    isolated=True,
)
browser.configure_context(
    ignore_https_errors=True,
    locale="en-US",
    viewport={"width": 1280, "height": 720},
)


@task
def capture() -> None:
    page = browser.goto("https://example.com")
    page.screenshot(path="output/example.png")
```

Call `configure(...)` and `configure_context(...)` before `browser.page()`, `browser.context()`, or `browser.goto(...)` creates the managed browser state.

For a persistent context, configure the directory before opening a page:

```python
from robocorp import browser


browser.configure(
    browser_engine="chromium",
    headless=True,
    persistent_context_directory="output/browser-context",
)
```

Persistent contexts should not be shared by parallel runs. When `persistent_context_directory` is set, avoid APIs that require a reusable browser/context split, such as `browser.browser()`. For CI, prefer fresh contexts unless session reuse is the feature under test.

### Work Items With Failure Semantics

```python
from robocorp import workitems
from robocorp.tasks import task


@task
def consume() -> None:
    for item in workitems.inputs:
        with item:
            if "id" not in item.payload:
                raise workitems.BusinessException(
                    code="MISSING_ID",
                    message="Input payload is missing id.",
                )
            workitems.outputs.create({"id": item.payload["id"], "status": "done"})
```

`BusinessException` marks non-retryable data/business failures. Use `ApplicationException` for retryable technical failures.

### Local Work Item File Adapter

```text
RC_WORKITEM_ADAPTER=FileAdapter
RC_WORKITEM_INPUT_PATH=devdata/work-items-in/test-input/work-items.json
RC_WORKITEM_OUTPUT_PATH=devdata/work-items-out/last-run/work-items.json
```

This is the modern Python `robocorp.workitems` file-adapter env family. Do not mix it with legacy `RPA_WORKITEMS_*` variables unless bridging older Robot Framework code.

### Vault Mock Plus Secret Suppression

```python
from robocorp import log, vault
from robocorp.tasks import task


@task
def rotate_token() -> None:
    with log.suppress_variables():
        secret = vault.get_secret("service-account")
        secret["token"] = "new-token-from-provider"
        vault.set_secret(secret)
```

For local mock vault only:

```text
RC_VAULT_SECRET_MANAGER=FileSecrets
RC_VAULT_SECRET_FILE=/absolute/path/to/dev-secrets.yaml
```

Mock vault files are development fixtures, not safe secret stores.

### Process Snapshot

```python
from robocorp import log
from robocorp.tasks import task


@task
def capture_debug_snapshot() -> None:
    log.process_snapshot()
```

Use `process_snapshot()` for targeted debugging of long-running or resource-sensitive robots. Keep secret suppression in place around sensitive variables before adding snapshots to a workflow.

### Asset Storage

```python
from pathlib import Path

from robocorp import storage
from robocorp.tasks import task


@task
def update_state() -> None:
    known_assets = storage.list_assets()
    report_path = Path("output/report.json")
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text('{"status": "ok"}', encoding="utf-8")

    if "workflow-state" not in known_assets:
        storage.set_json("workflow-state", {"last_status": "new"})

    state = storage.get_json("workflow-state")
    state["last_status"] = "ok"
    state["known_assets"] = known_assets
    storage.set_json("workflow-state", state)

    storage.set_text("last-status", "ok")
    last_status = storage.get_text("last-status")

    storage.set_file("latest-report", report_path)
    downloaded = storage.get_file(
        "latest-report",
        Path("output/downloaded-report.json"),
        exist_ok=True,
    )

    storage.set_bytes("raw-snapshot", b"snapshot", content_type="application/octet-stream")
    snapshot = storage.get_bytes("raw-snapshot")

    if last_status == "ok" and downloaded.exists() and snapshot:
        # Delete only when cleanup is intended; asset deletion is irreversible.
        storage.delete_asset("raw-snapshot")
```

Use asset storage for shared state or binary/text assets that are not a queue item. Use work item attachments when the data should follow a specific item through a process.

### Work Item Attachments

```python
from robocorp import workitems


for input_item in workitems.inputs:
    with input_item:
        item = workitems.outputs.create({"status": "ready"}, save=False)
        item.add_file("output/report.json", name="report.json")
        item.save()
```

Creating an output item requires a reserved input item. Use `save=False` when the item needs payload and attachment setup before being published to the output queue.

### Action Package Work Items

```python
from actions.work_items import inputs, outputs
from sema4ai.actions import Response, action


@action
def consume(max_items: int = 10) -> Response[dict[str, int]]:
    processed = 0
    for item in inputs:
        if processed >= max_items:
            break
        with item:
            outputs.create({"source": item.payload, "status": "done"})
            processed += 1
    return Response(result={"processed": processed})
```

Use `actions-work-items` for action-package-centered workflows. Use `robocorp.workitems` for modern RCC robot flows.

## Repo Scan Notes

- `joshyorko/actions` branch `community` adds `templates/workflow-producer-consumer/` and includes it in `templates/packaging/templates-prod.json`; upstream `Sema4AI/actions` at the checked commit lists only minimal/basic/advanced/data-access templates there.
- `joshyorko/actions` includes Action Server RCC utility code under `action_server/src/sema4ai/action_server/_rcc_robot_utils.py`, useful when documenting package/robot discovery and Action Server calling RCC.
- `Sema4AI/gallery` is better for real action package shape, auth patterns, models, and `package.yaml` layout than for low-level library API docs.
- `Sema4AI/cookbook` contains agent/action blueprints and invocation-context examples. Use it for higher-level patterns, not package API authority.
- `joshyorko/rcc` docs emphasize isolated Python automation packages, endpoint overrides, dependency export/freeze flow, `rcc task script`, uv-native mode, and holotree caching. Use these to keep Python-library guidance tied to the actual runtime boundary.
- Robocorp example/template repos still have useful recipes, but many are historical or Robot Framework-first. Prefer modern Python templates first, then legacy examples only when maintaining old code.

## Refresh Commands

Run from the Bluefin host or a repo devcontainer with `git` and authenticated `gh`:

```bash
audit_dir="/tmp/plugins-python-library-audit-$(date +%Y%m%d)"
mkdir -p "$audit_dir"
git clone --depth 1 --branch community https://github.com/joshyorko/actions.git "$audit_dir/joshyorko-actions-community"
git clone --depth 1 https://github.com/Sema4AI/actions.git "$audit_dir/sema4ai-actions"
git clone --depth 1 https://github.com/joshyorko/rcc.git "$audit_dir/joshyorko-rcc"
git clone --depth 1 https://github.com/robocorp/robocorp.git "$audit_dir/robocorp"
git clone --depth 1 https://github.com/Sema4AI/gallery.git "$audit_dir/sema4ai-gallery"
gh repo list Sema4AI --limit 100 --json name,description,isArchived,updatedAt,url,primaryLanguage
gh repo list robocorp --limit 100 --json name,description,isArchived,updatedAt,url,primaryLanguage
gh repo list joshyorko --limit 200 --json name,description,isArchived,updatedAt,url,primaryLanguage
```

After refreshing, update this file and `references/source-map.md` with new commit ids before changing examples or pin guidance.
