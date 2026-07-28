# DocDB RPA Production Patterns

Source: local example `bps-rpa-cx-qa-bot-main`, inspected 2026-05-14 from the `plugins` workspace.

Use this reference when a robot treats DocumentDB/MongoDB as the durable work-item system, artifact store, retry source, replay source, or outbox boundary. It is a production RPA pattern, not just a bigger local adapter example.

## Core Model

The durable state boundary is the DocDB queue family:

```text
cx_qa_run_<run_id>             # seed and producer input
cx_qa_run_<run_id>_output      # consumer input
cx_qa_run_<run_id>_results     # reporter, retry, dashboard, and outbox input
cx_qa_run_<run_id>_reporter_output
rpa_results_outbox             # stable downstream sync contract
```

Never rely on implicit adapter output naming in multi-stage flows. Set `RC_WORKITEM_QUEUE_NAME` and `RC_WORKITEM_OUTPUT_QUEUE_NAME` per role so collections do not drift into `queue_output_output`.

Typical local env files:

```json
{
  "RC_WORKITEM_ADAPTER": "robocorp_adapters_custom._docdb.DocumentDBAdapter",
  "DOCDB_URI": "mongodb://qauser:qapassword@localhost:27017/rpa_qa_workitems?authSource=admin&retryWrites=false",
  "DOCDB_DATABASE": "bps_rpa_workitems",
  "RC_WORKITEM_QUEUE_NAME": "cx_qa_run_local_output",
  "RC_WORKITEM_OUTPUT_QUEUE_NAME": "cx_qa_run_local_results",
  "RC_WORKITEM_FILES_DIR": "devdata/work_item_files",
  "RC_WORKITEM_ORPHAN_TIMEOUT_MINUTES": "30",
  "RC_WORKITEM_FILE_SIZE_THRESHOLD": "1000000"
}
```

Keep live credentials out of committed env files. In CI, set queue and credential env vars directly in the job and do not pass `-e devdata/env-docdb-*.json` to production tasks.

## Robot Task Surface

Expose business tasks and helper tasks through `robot.yaml`:

```yaml
tasks:
  SeedDocDB:
    shell: python scripts/seed_docdb.py
  Producer:
    shell: python -m robocorp.tasks run tasks.py -t producer
  Consumer:
    shell: python -m robocorp.tasks run tasks.py -t consumer
  Reporter:
    shell: python -m robocorp.tasks run tasks.py -t reporter
  RunDocDBHelper:
    shell: python scripts/run_docdb_helper.py
```

`RunDocDBHelper` is a deliberate runtime boundary. It lets CI run helper scripts through the RCC Python environment without direct host-Python calls:

```bash
export DOCDB_HELPER_ARGS_JSON='["count_docdb_items.py","--queue-name","cx_qa_run_local_output","--max-workers","3"]'
export DOCDB_HELPER_OUTPUT_FILE="output/docdb/matrix-output.json"
rcc run -t RunDocDBHelper --silent
```

The helper should:

- parse `DOCDB_HELPER_ARGS_JSON` as a non-empty JSON array,
- allowlist script names,
- restore `sys.argv` after `runpy.run_path`,
- optionally redirect stdout into `DOCDB_HELPER_OUTPUT_FILE`, and
- reject arbitrary script paths.

## CI Shape

For DocDB-backed RPA workflows:

- validate environment/ref pairing before claiming scarce self-hosted runners,
- generate one run-scoped base queue in setup,
- load DocDB credentials inside each job with `rcc --silent task script -r robot.yaml -- python scripts/load_docdb_credentials.py`,
- mask secret URIs with `::add-mask::` and write them to `GITHUB_ENV`, not job outputs,
- create queue indexes before seeding or running workers,
- run RCC with `--silent` in active workflows,
- count pending DocDB work items to generate the worker matrix,
- let consumers atomically reserve from one shared queue instead of pre-sharding JSON files,
- use `fail-fast: false` so one worker failure does not cancel useful evidence from others,
- store logs and screenshots back into DocDB in `if: always()` steps, and
- publish the outbox only after retry has had a chance to improve final results.

Do not replace active CI helper calls with `python3 scripts/...` on the runner host. The helper scripts depend on the robot's pinned Python packages and truststore behavior.

## Artifacts, Retries, And Outbox

Use DocDB for both item state and run evidence:

- work item documents live in `<queue>_work_items` collections,
- small files can be stored inline, large files in GridFS,
- `run_artifacts` stores producer, consumer, reporter, and retry logs,
- screenshots are attached to result work items or artifact records,
- downloads materialize `work-items.json`, `work-items-final.json`, and `work-item-attempt-history.json` for dashboards, and
- dashboards should prefer final per-call results after retry, not first-pass reporter summaries.

Retry should copy only the fields needed to rerun the work item, increment `retry_count`, set `is_retry`, preserve safe source metadata, and mark the original failed item as queued. Avoid mutating downstream sync state while requeueing.

Publish one stable outbox record per `(run_id, callid, form)` identity. Prefer the highest retry count and latest attempt timestamp. Preserve:

- `source.collection`, `source.item_id`, and `source.document_id`,
- artifact/screenshot references,
- safe upstream state metadata, and
- normalized failure category fields.

The RPA bot should write to a central RPA-owned outbox such as `rpa_results_outbox`; a downstream service owns tenant fan-out or domain-system writes.

## Observability Payloads

Result payloads should make failures actionable without leaking sensitive data:

```text
status
retry_count
run_id
callid/contact_id
form/evaluationTemplateId
failed_stage/stage
error_category/failure_category
failure_category_label
failure_category_explanation
failure_category_action
rpa_timing.steps[]
source_metadata
screenshot_links/artifact_refs
```

Use a small timing recorder that writes JSON-safe step entries with `step`, `status`, `started_at`, `ended_at`, `duration_ms`, optional sanitized `error`, and optional metadata. Classify failures into stable buckets that downstream reports, dashboards, issue aggregation, and replay tooling can share.

Keep logs and generated issue bodies sanitized. Do not paste raw row text, screenshots, credentials, tokens, customer data, or raw service payloads into issues.

## Indexes And Tests

Create indexes for the whole queue family, not only the producer queue:

- unique `item_id`,
- queue/state/created-at claim scans,
- state/status and payload status,
- run id and payload run id,
- call/form duplicate checks,
- reservation and claimed-until,
- outbox identity and sync status, and
- issue fingerprint registry if issue aggregation is enabled.

Good tests assert workflow contracts, not only helper functions:

- production workflows run helpers through `RunDocDBHelper`,
- live workflows do not pass local `devdata/env-docdb-*.json`,
- masked DocDB URI is not passed between jobs as an output,
- active RCC workflow lines include `--silent`,
- queue names follow the base/output/results ladder,
- local DocDB env files are concrete and local-only,
- helper allowlists include every production helper,
- index names are compatible with the adapter, and
- local scripts can run either file-adapter or DocDB mode.

## Local Smoke Ladder

For a local DocDB-style smoke on a Linux host or devcontainer with Docker available:

```bash
docker compose up -d mongodb
rcc run -t RunDocDBHelper --silent
rcc run -t SeedDocDB --silent
rcc run -t Producer -e devdata/env-docdb-producer.json --silent
rcc run -t Consumer -e devdata/env-docdb-consumer.json --silent
rcc run -t Reporter -e devdata/env-docdb-reporter.json --silent
```

Prefer a checked-in wrapper when the flow has many helper calls, for example:

```bash
scripts/local-sharded-run.sh 1 docdb
```

Local wrappers may use concrete `devdata/env-docdb-*.json`; production workflows should set live queue and secret env vars in the job.
