# Python Library Audit

Use this reference when improving RCC-family skill examples for Python libraries, package pins, or source-backed recipes. It captures the 2026-05-23 audit across official docs, Josh repositories, upstream Robocorp/Sema4AI repositories, and public Robocorp runtime/process examples.

The 2026-11-08 [source snapshot](source-map.md#2026-11-08-onboarding-and-local-compatibility-refresh) selectively refreshes project-shape, local SDK fixture, and hosted-service boundaries below. Package-version observations remain historical, not new pin recommendations.

Recheck sources before making "latest", version-current, or default-template claims. Treat Josh-owned RCC sources as current stack evidence, and upstream Robocorp/Sema4AI docs as package/API evidence unless the task explicitly targets their hosted product. Control Room backend architecture must stay marked as inference when derived from public clients or examples instead of backend source.

## Source Snapshot

Official docs and source repos checked. Only the RCC source row was refreshed on 2026-09-03; the Python package/library rows retain the 2026-05-23 snapshot and must not be treated as current-version proof:

- Sema4AI actions docs: `https://sema4.ai/docs/build-agents/develop-actions/action-structure`
- Sema4AI Python action/MCP source: `https://github.com/Sema4AI/actions` at `master` / `9d9479e`
- Robocorp Python library docs: `https://sema4.ai/docs/automation/python/robocorp`
- Robocorp Python library source: `https://github.com/robocorp/robocorp` at `master` / `64cc9a3`
- Josh Action Server fork: `https://github.com/joshyorko/actions` at `community` / `3bae23bb49fe`
- Josh RCC fork: `https://github.com/joshyorko/rcc` at tag `v18.19.7` / `4148c2b71705c9d2baf0e88b48d08a79cb7bda0f`
- Sema4AI gallery: `https://github.com/Sema4AI/gallery` at `main` / `d6afd61`
- Public Robocorp runtime/process examples: `k8s-on-demand-runtimes`, `azure-on-demand-runtimes`, `example-worker-logs`, `template-python-workitems`, and `python-producer-consumer-reporting`.

Useful org-level repos found during the scan:

- Sema4AI: `actions`, `gallery`, `cookbook`, `vscode-extension`.
- Robocorp: `robocorp`, `rpaframework`, `template-python`, `template-python-browser`, `template-python-workitems`, `template-python-assistant-ai`, `template-action`, `example-action-server-starter`, `example-advanced-python-template`, `python-producer-consumer-reporting`, `example-web-store-work-items`, `example-asset-storage`, `example-using-vault`, `example-encrypt-workitems`, `example-langchain-data-ingestion`.
- Josh: `actions`, `rcc`, `robot-templates`, `robocorp`, `gallery`, `robocorp_adapters_custom`, `fetch-repos-bot`, `fizzy-symphony`, `linkedin-easy-apply`, `yo-dawg`, `home-lab-actions`, `actions-robot-boostrapper`, `context-eng-copilot`, `rccremote-docker`, `prefect-setup`, `cook-with-gas-rpa-challenge`.

## Library Map

| Surface                | Libraries                                                                                                                    | Use for                                                                                                 | Example stance                                                                                                                                                                                                       |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Action packages        | `actions-core`, `actions-runtime`, `actions.mcp`                                                                             | `@action`, direct MCP tools, named data queries, OpenAPI/MCP exposure                                   | Use the owning Action Server references for current `@action`, typed responses, secrets, and MCP v2 examples; historical upstream APIs are not interchangeable.                                                                 |
| RCC robot projects     | `robocorp`, `robocorp-tasks`, `robocorp-log`, `robocorp-browser`, `robocorp-workitems`, `robocorp-vault`, `robocorp-storage` | Contained automation run through `robot.yaml` and `conda.yaml`                                          | Keep literal package/API names; RCC owns the environment boundary.                                                                                                                                                   |
| Browser automation     | `robocorp-browser`                                                                                                           | Playwright-backed browser tasks in RCC projects or action packages                                      | Configure before `browser.page()`; use isolated install/post-install for CI; persistent contexts are single-run-sensitive.                                                                                           |
| Work item queues       | `robocorp-workitems`, `actions-work-items`, `robocorp-adapters-custom`                                                       | Producer/consumer/reporter flows, Action Server/non-robot queues, file/SQLite/Redis/DocumentDB adapters | Prefer `actions-work-items` and user-owned adapters for portable flows; preserve `robocorp.workitems` and existing custom contracts for compatibility. |
| Secrets and assets     | `robocorp-vault`, `robocorp-storage`, `robocorp-log`                                                                         | Upstream compatibility boundaries and safe logging                                                              | FileSecrets is test-only; Vault defaults to hosted RobocorpVault and Storage needs Control Room asset APIs. No shipped generic owned provider is implied.                                                                                 |
| RPA Framework and HITL | `rpaframework`, `rpaframework-assistant`, Robot Framework libraries                                                          | Legacy `RPA.*` robots, Python-callable RPA utilities, Assistant desktop dialogs                         | Prefer modern `robocorp.*` libraries for new RCC Python robots. Use `RPA.Assistant` through `rpaframework-assistant` when a desktop/HITL dialog is intentionally part of the workflow.                               |

## Coverage Notes To Keep Current

- `robocorp-storage`: the boundary below replaces hosted asset examples; do not imply that configuring an endpoint ships a standalone backend.
- `robocorp-vault`: local FileSecrets development guidance and `robocorp.log.suppress_variables()` examples are included below for secret rotation or credential generation tasks.
- `robocorp-log`: `process_snapshot()` and suppression examples are included below for debugging long-running robots without leaking secrets.
- `actions-core`: use the existing Action Server references rather than duplicating or importing upstream action examples.
- `actions.mcp`: consult the owning Action Server references for supported MCP v2 tooling.
- `robocorp-browser`: browser examples below cover `configure_context(...)`, persistent-context constraints, and `browser.configure(install=True, isolated=True)` versus `python -m robocorp.browser install ... --isolated`.
- `actions-work-items`: use Josh's `workflow-producer-consumer` template as community-branch evidence when documenting action-package producer/consumer flows.

## Reference Examples

### Action Packages

Use the owning [`action-server` skill](../../action-server/SKILL.md) for current `actions-core` / `actions-runtime`, `from actions import action`, typed responses, and MCP v2 examples. Python `robocorp.tasks` uses `@task` with `robot.yaml`; Actions Core uses `@action` with `package.yaml` v2; Robot Framework uses `.robot` suites. Do not mix entry points or copy historical `sema4ai.*` examples into our Actions packages.

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

### Test-only Upstream Vault Compatibility

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

The example above applies only to existing upstream integrations or test-only FileSecrets fixtures. `robocorp.vault` defaults to hosted `RobocorpVault`; custom `BaseSecretManager` is an extension point, not a shipped production self-hosted provider. Select the fixture adapter explicitly before importing/using Vault:

```text
RC_VAULT_SECRET_MANAGER=FileSecrets
RC_VAULT_SECRETS_FILE=/absolute/path/to/dev-secrets.yaml
```

Use only dummy values in FileSecrets files. They are test fixtures, not production secret stores; see the [local compatibility recipe](../../rcc-workitems/references/workitems-adapters.md#local-upstream-sdk-compatibility-fixtures).

### Process Snapshot

```python
from robocorp import log
from robocorp.tasks import task


@task
def capture_debug_snapshot() -> None:
    log.process_snapshot()
```

Use `process_snapshot()` for targeted debugging of long-running or resource-sensitive robots. Keep secret suppression in place around sensitive variables before adding snapshots to a workflow.

### Asset Storage Boundary

Upstream `robocorp.storage` constructs an `AssetsClient` using a Control Room workspace, endpoint, and token. It has no built-in generic local-provider selection. Do not use its `get_json` / `set_json` / file APIs as an implicitly self-hosted default. Use explicit user-owned files or storage interfaces for local workflows; a production owned Vault/Storage provider remains future work. Existing upstream integrations require a verified backend and explicit scope.

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
from actions import Response, action


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

Prefer `actions-work-items` and user-owned adapters for new portable workflows. Keep `robocorp.workitems` when maintaining existing robot API/adapter contracts.

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
