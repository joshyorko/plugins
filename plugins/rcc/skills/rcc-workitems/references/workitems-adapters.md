# Work Items And Adapters

Use this guide for producer/consumer queues, classic Robocorp work items, `actions-work-items`, and custom adapters.

For cross-source Python library evidence, work item example gaps, and refresh commands, see `../../rcc/references/python-library-audit.md`.

For production DocumentDB/MongoDB queue ladders, retry/outbox design, CI helper boundaries, and observability payloads, see `docdb-rpa-patterns.md`.

## Adapter Lineage

`robocorp-adapters-custom` predates the generalized `actions-work-items` package. Treat it as the production-proven adapter layer for classic `robocorp.workitems` robot workflows, especially DocumentDB/MongoDB queue families, GridFS attachments, explicit output queues, orphan recovery, retry handling, and RCC producer/consumer/reporter runs.

`actions-work-items` is the later generalized package for Action Server and non-robot workflows. It offers Robocorp-style aliases and a similar API shape, but it has its own adapter contract and should not replace production `robocorp-adapters-custom` guidance.

## Classic robocorp.workitems

For new portable queue workflows, prefer our `actions-work-items` and user-owned adapters. Preserve classic contracts when maintaining existing robots. Upstream `robocorp.workitems` can select its hosted adapter in activity contexts; explicitly choose a local adapter for fixtures.

```python
from robocorp import workitems
from robocorp.tasks import task


@task
def producer() -> None:
    for seed in workitems.inputs:
        with seed:
            for row in seed.payload["rows"]:
                workitems.outputs.create({"id": row["id"], "status": "new"})


@task
def consumer() -> None:
    for item in workitems.inputs:
        with item:
            payload = item.payload
            workitems.outputs.create({"processed": True, "source": payload})
```

Use `with item:` so input items are released consistently. `outputs.create(...)` needs a currently reserved input item, so pure producers should start from a seed input or use an adapter-specific queue seeding script. Raise `BusinessException` for non-retryable data/business problems and `ApplicationException` for retryable technical failures. At task boundaries those exceptions fail the item; inside `with item:` loops, the context manager records the failed item and lets the loop continue.

## Local upstream SDK compatibility fixtures

This recipe is **legacy upstream SDK compatibility only**, using dummy data and no hosted account. For production, use a verified user-owned queue/backend. FileSecrets is test-only: upstream Vault defaults to hosted `RobocorpVault`, while `BaseSecretManager` is only an extension point. Upstream Storage uses Control Room asset APIs without a built-in generic local adapter. Production owned Vault/Storage providers and Action Server's intended unified control-plane role are future work, not shipped capabilities.

In an existing RCC project that declares `robocorp-workitems` and `robocorp-vault` (or the `robocorp` meta package), create `devdata/env.json` as a flat environment object:

```json
{
  "RC_WORKITEM_ADAPTER": "FileAdapter",
  "RC_WORKITEM_INPUT_PATH": "devdata/input.json",
  "RC_WORKITEM_OUTPUT_PATH": "devdata/producer-output.json",
  "RC_VAULT_SECRET_MANAGER": "FileSecrets",
  "RC_VAULT_SECRETS_FILE": "devdata/test-secrets.json"
}
```

`devdata/input.json` contains a seed item, for example `[{"payload": {"rows": [{"id": "fixture-1"}]}, "files": {}}]`. `devdata/test-secrets.json` contains only dummy values, for example `{"fixture": {"token": "test-only"}}`. File attachment paths are relative to the item JSON file. The Vault key is `RC_VAULT_SECRETS_FILE` (plural). Keep real secrets out of committed files and logs.

Read the exact task keys in `robot.yaml`, set `producer_task` / `consumer_task` accordingly, and run from the project root:

```sh
rcc task run --robot robot.yaml --task "$producer_task" -e devdata/env.json
```

Inspect the producer's exit status, final PASS, and output payloads before continuing. Create `devdata/env-consumer.json` with the same explicit local adapters, set `RC_WORKITEM_INPUT_PATH` to `devdata/producer-output.json` and `RC_WORKITEM_OUTPUT_PATH` to a different `devdata/consumer-output.json`, then run:

```sh
rcc task run --robot robot.yaml --task "$consumer_task" -e devdata/env-consumer.json
```

Verify the consumer's PASS and output payloads, including any failed items; a runner PASS alone does not prove every item succeeded. Validate fixtures with network access denied **after** environment provisioning so an unintended hosted fallback fails. Use declared RCC environments, not host Python. Do not call upstream Storage or omit the explicit adapters to make a fixture pass.

## actions-work-items

Josh's `actions` community branch includes `actions-work-items>=0.2.4`, the later generalized package for Action Server-centered packages and other non-robot producer/consumer workflows.

Install Redis or DocumentDB support with `actions-work-items[redis]`, `actions-work-items[docdb]`, `actions-work-items[documentdb]`, or `actions-work-items[all]`.

```python
from actions.work_items import BusinessException, inputs, outputs

for item in inputs:
    with item:
        if "id" not in item.payload:
            raise BusinessException("Missing id")
        outputs.create({"id": item.payload["id"], "processed": True})
```

Supported import forms:

```python
from actions import workitems
from actions_work_items import workitems
import actions_work_items as workitems
from actions.work_items import inputs, outputs
```

Default local SQLite adapter:

```python
from actions.work_items import SQLiteAdapter, init

adapter = SQLiteAdapter(
    db_path="./workitems.db",
    queue_name="default",
    files_dir="./work_item_files",
)
init(adapter)
```

Environment variables used across local, Redis, and DocumentDB adapter paths:

```text
RC_WORKITEM_ADAPTER
RC_WORKITEM_QUEUE_NAME
RC_WORKITEM_OUTPUT_QUEUE_NAME
RC_WORKITEM_FILES_DIR
RC_WORKITEM_DB_PATH
RC_WORKITEM_INPUT_PATH
RC_WORKITEM_OUTPUT_PATH
RC_REDIS_URL
DOCDB_URI
DOCDB_HOSTNAME
DOCDB_PORT
DOCDB_USERNAME
DOCDB_PASSWORD
DOCDB_DATABASE
RC_WORKITEM_AUTO_APPEND_OUTPUT_SUFFIX
RC_WORKITEM_FILE_SIZE_THRESHOLD
```

`RC_WORKITEM_FILES_DIR`, `RC_WORKITEM_DB_PATH`, and queue-name envs are custom/local adapter concerns. The built-in modern FileAdapter path is intentionally smaller: `RC_WORKITEM_ADAPTER`, `RC_WORKITEM_INPUT_PATH`, and `RC_WORKITEM_OUTPUT_PATH`. Redis adapters use `RC_REDIS_URL`. DocumentDB adapters can use a single `DOCDB_URI` or split `DOCDB_HOSTNAME`, `DOCDB_PORT`, `DOCDB_USERNAME`, `DOCDB_PASSWORD`, and `DOCDB_DATABASE` settings. `RC_WORKITEM_AUTO_APPEND_OUTPUT_SUFFIX` controls output queue naming in adapters that support automatic suffixing, and `RC_WORKITEM_FILE_SIZE_THRESHOLD` controls when larger files are stored through the backend file path such as GridFS.

## Custom Adapter Contract

Custom adapters follow the Robocorp work item adapter shape. Josh's `robocorp_adapters_custom` adapters use:

- `reserve_input() -> str`
- `release_input(item_id, state, exception=None)`
- `create_output(parent_id, payload=None) -> str`
- `load_payload(item_id)`
- `save_payload(item_id, payload)`
- `list_files(item_id)`
- `get_file(item_id, name)`
- `add_file(item_id, name, content)`
- `remove_file(item_id, name)`

Josh's `actions-work-items` base adapter is similar but its `release_input` splits error fields into `exception_type`, `code`, and `message`, and its `add_file` signature includes `original_name`.

Adapter implementations should make reservation, release, payload, and file behavior testable independently. Use local SQLite or file adapters in CI before testing distributed backends.

## robocorp-adapters-custom

Josh's `robocorp_adapters_custom` repo provides these adapters:

- SQLite: `robocorp_adapters_custom._sqlite.SQLiteAdapter`
- Redis: `robocorp_adapters_custom._redis.RedisAdapter`
- DocumentDB/MongoDB: `robocorp_adapters_custom._docdb.DocumentDBAdapter`
- Yorko Control Room: `robocorp_adapters_custom._yorko_control_room.YorkoControlRoomAdapter`

Common SQLite env file:

```json
{
  "RC_WORKITEM_ADAPTER": "robocorp_adapters_custom._sqlite.SQLiteAdapter",
  "RC_WORKITEM_DB_PATH": "devdata/work_items.db",
  "RC_WORKITEM_FILES_DIR": "devdata/work_item_files",
  "RC_WORKITEM_QUEUE_NAME": "default",
  "RC_WORKITEM_OUTPUT_QUEUE_NAME": "default_output"
}
```

Role-specific env files should set output queue names explicitly. Do not rely on adapter defaults in multi-stage flows:

```json
{
  "RC_WORKITEM_ADAPTER": "robocorp_adapters_custom._sqlite.SQLiteAdapter",
  "RC_WORKITEM_DB_PATH": "devdata/work_items.db",
  "RC_WORKITEM_FILES_DIR": "devdata/work_item_files",
  "RC_WORKITEM_QUEUE_NAME": "repos",
  "RC_WORKITEM_OUTPUT_QUEUE_NAME": "repo_results"
}
```

If producer, consumer, and reporter share one backend, write down the ladder:

```text
seed input -> producer queue -> consumer queue -> reporter queue -> final artifact
```

The common bug is cascading default names such as `queue_output_output`. Fix it by making each role's input and output queue explicit.

Common Redis env file:

```json
{
  "RC_WORKITEM_ADAPTER": "robocorp_adapters_custom._redis.RedisAdapter",
  "REDIS_HOST": "localhost",
  "REDIS_PORT": "6379",
  "RC_WORKITEM_QUEUE_NAME": "default"
}
```

Common DocumentDB/MongoDB local env file:

```json
{
  "RC_WORKITEM_ADAPTER": "robocorp_adapters_custom._docdb.DocumentDBAdapter",
  "DOCDB_URI": "mongodb://qauser:qapassword@localhost:27017/rpa_qa_workitems?authSource=admin&retryWrites=false",
  "DOCDB_DATABASE": "bps_rpa_workitems",
  "RC_WORKITEM_QUEUE_NAME": "cx_qa_run_local",
  "RC_WORKITEM_OUTPUT_QUEUE_NAME": "cx_qa_run_local_output",
  "RC_WORKITEM_FILES_DIR": "devdata/work_item_files",
  "RC_WORKITEM_ORPHAN_TIMEOUT_MINUTES": "30",
  "RC_WORKITEM_FILE_SIZE_THRESHOLD": "1000000"
}
```

For production DocDB, prefer a run-scoped queue family:

```text
cx_qa_run_<run_id> -> cx_qa_run_<run_id>_output -> cx_qa_run_<run_id>_results
```

Each role should receive the exact queue and output queue in the job environment. Local `devdata/env-docdb-*.json` files are for local MongoDB smoke runs; do not pass them to live CI jobs because they can override live queue names, credentials, or database settings.

When helpers such as `count_docdb_items.py`, `create_docdb_indexes.py`, `store_artifact_docdb.py`, `reset_failed_items.py`, or `publish_rpa_outbox.py` need the robot dependencies, expose an allowlisted RCC task and drive it through JSON args:

```bash
export DOCDB_HELPER_ARGS_JSON='["count_docdb_items.py","--queue-name","cx_qa_run_local_output","--max-workers","3"]'
rcc run -t RunDocDBHelper --silent
```

Use `DOCDB_HELPER_OUTPUT_FILE` when a helper needs to return machine-readable data to GitHub Actions.

## Process And Reporter Patterns

- Producer tasks usually read one seed input and create child output items.
- Consumer tasks loop the previous step's outputs and release each item as completed or failed.
- Reporter tasks can consume consumer outputs, use a sentinel payload such as `{"TYPE": "Reporter"}`, or inspect prior step work items through a Process API pattern.
- Retry automation should target application, orchestrator, or unspecified/system failures. Do not retry business/data failures by default.
- Local file adapters simulate JSON queue handoff, but they do not reproduce all hosted scheduling semantics.
- Local reporter smoke tests can use an explicit `COMPLETED_CONSUMER_WORKITEMS_JSON` fixture when the project does not have a hosted Process API run to inspect.
- Email-triggered examples rely on parsed Control Room email payloads and files; raw email payload paths are historical/deprecated example material.
- Terminal marker payloads such as `_TERMINAL_=True` are process-control examples, not a general work-item requirement.

## Local Queue Semantics

- SQLite is persistent and good for local/CI producer-consumer tests.
- File adapters are closest to Control Room JSON input/output fixtures and are best for tiny deterministic tests.
- Redis and DocumentDB support distributed workers but add service availability and locking/failure modes.
- Keep queue name and output queue name explicit in env files when multiple roles share one database.
- For DocumentDB, create indexes for the whole queue family before seeding/running workers, including claim/status/run lookups, duplicate-check keys, retry markers, outbox identity, and artifact retrieval.
- Older examples may use adapter paths such as `robocorp_adapters_custom.sqlite_adapter.SQLiteAdapter`; verify the installed package version. Current canonical paths use underscored modules such as `robocorp_adapters_custom._sqlite.SQLiteAdapter`.

## Modern Python Vs Legacy Robot Framework

Modern Python robots use `robocorp.workitems` and these local development env names:

```text
RC_WORKITEM_ADAPTER=FileAdapter
RC_WORKITEM_INPUT_PATH=devdata/work-items-in/test-input/work-items.json
RC_WORKITEM_OUTPUT_PATH=devdata/work-items-out/last-run/work-items.json
```

Legacy Robot Framework / RPA Framework flows use `RPA.Robocorp.WorkItems` and older env names:

```text
RPA_WORKITEMS_ADAPTER=RPA.Robocorp.WorkItems.FileAdapter
RPA_INPUT_WORKITEM_PATH=devdata/work-items-in/test-input/work-items.json
RPA_OUTPUT_WORKITEM_PATH=devdata/work-items-out/last-run/work-items.json
```

Do not mix the two env families inside one run unless you are intentionally bridging old and new code.

## Full Producer / Consumer / Reporter Flow

Use this structure for serious work item robots:

```python
from pathlib import Path

from robocorp import workitems
from robocorp.tasks import task


@task
def producer() -> None:
    for source in workitems.inputs:
        with source:
            repos = source.payload["repos"]
            for repo in repos:
                workitems.outputs.create({"repo": repo, "status": "queued"})


@task
def consumer() -> None:
    output = Path("output")
    output.mkdir(exist_ok=True)
    for item in workitems.inputs:
        with item:
            repo = item.payload["repo"]
            artifact = output / f"{repo.replace('/', '_')}.json"
            artifact.write_text('{"status": "done"}')
            workitems.outputs.create(
                {"repo": repo, "status": "done", "artifact": str(artifact)}
            )


@task
def reporter() -> None:
    counts = {"done": 0, "failed": 0}
    for item in workitems.inputs:
        with item:
            counts[item.payload.get("status", "failed")] += 1
    Path("output/final_report.json").write_text(str(counts))
```

Release each input inside the loop. For Robot Framework, mirror the same behavior with `Release Input Work Item` and explicit `DONE`, `BUSINESS`, or `APPLICATION` states.

## Files And Attachments

Work item payloads are JSON, and files are attachments. Local file-adapter inputs usually look like:

```json
[
  {
    "payload": {"order_id": "1001"},
    "files": {"invoice.pdf": "devdata/files/invoice.pdf"}
  }
]
```

Use `outputs.create(payload=..., files=[...])` when a downstream task needs artifacts immediately. For staged output items, create with `save=False`, call `add_file(...)`, then call `save()`. `add_file` changes are staged until the item is saved. When reading attachments, pass an explicit destination such as `Path("output") / filename`; `get_file(name)` defaults to the robot root/current directory, which can scatter artifacts during local tests.

## Legacy RPA Framework Keywords

Legacy Robot Framework flows commonly map to these operations:

```robotframework
Create Output Work Item
Set Work Item Variable    status    processed
Add Work Item File        output/result.json
Save Work Item
Release Input Work Item   DONE
```

Use `BUSINESS` for non-retryable data problems and `APPLICATION` for retryable technical failures. Modern Python code should prefer `BusinessException` and `ApplicationException`; keep the Robot Framework keywords only when maintaining `RPA.Robocorp.WorkItems` robots.

## Operational Recovery

Add `devTasks` for queue operations:

```bash
rcc run -r robot.yaml --dev -t CheckSQLiteDB --silent
rcc run -r robot.yaml --dev -t RecoverOrphanedItems --silent
rcc run -r robot.yaml --dev -t DiagnoseReporter --silent
```

Use recovery scripts when items are stuck in `reserved` after an interrupted worker. Use reporter diagnostics when downstream payloads do not match the reporter's expected schema.

## Validation Commands

```bash
rcc run -t Producer -e devdata/env-sqlite-producer.json
rcc run -t Consumer -e devdata/env-sqlite-consumer.json
rcc run -t Reporter -e devdata/env-sqlite-for-reporter.json
rcc run --dev -t CheckSQLiteDB
```

When a consumer sees no work, inspect the adapter env file, queue names, database path, and whether the producer released output items into the same backend.
