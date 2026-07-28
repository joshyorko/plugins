# DocDB RPA Workflow CI Pattern

Source: local example `bps-rpa-cx-qa-bot-main`, inspected 2026-05-14 from the `plugins` workspace.

Use this for production GitHub Actions workflows that run RCC-backed RPA bots with DocumentDB/MongoDB work-item queues. Use `../../rcc-workitems/references/docdb-rpa-patterns.md` for adapter, queue, retry, outbox, and payload details.

## Production Job Ladder

A durable DocDB RPA workflow usually has:

1. `validate-ref`: validate environment/ref pairing before claiming a self-hosted runner.
2. `setup`: generate `cx_qa_run_<github.run_id>` and download shared CA files if needed.
3. `seed`: load DocDB credentials, create indexes, seed the base queue.
4. `producer`: run the producer and count pending consumer items to build a matrix.
5. `consumer`: matrix workers reserve from the same DocDB queue.
6. `reporter`: aggregate first-pass result items.
7. `retry-failed-items`: reset failed items once, rerun consumer, store retry evidence, publish final outbox rows.
8. `consolidated-dashboard`: download DocDB artifacts, generate reports, and upload a small debug artifact bundle.

The point is to keep state in DocDB, not in transient GitHub artifacts between jobs.

## Required Guardrails

- Validate `sit` workflows run from the `sit` branch and `prod` workflows run from `main`.
- Install or locate RCC before robot commands, then print `rcc --silent version`.
- Disable telemetry with `rcc --silent config identity -t` when the workflow can.
- Load DocDB credentials in every job that needs them:

```bash
rcc --silent task script -r robot.yaml -- python scripts/load_docdb_credentials.py
```

The credential helper should read Secrets Manager or equivalent, print `::add-mask::<uri>`, and append `DOCDB_URI=<uri>` to `$GITHUB_ENV`. Do not pass a masked URI as a job output; GitHub masking can collapse it into an unusable value and it widens the secret boundary.

- Production tasks should run without local env files:

```bash
rcc run -t Producer --silent
rcc run -t Consumer --silent
rcc run -t Reporter --silent
```

Use `-e devdata/env-docdb-*.json` only for local MongoDB smokes.

## Helper Calls

Run helper scripts through an allowlisted robot task:

```bash
export DOCDB_HELPER_ARGS_JSON=$(jq -cn \
  --arg queue_name "$QUEUE_NAME" \
  '["create_docdb_indexes.py","--queue-name",$queue_name]')
rcc run -t RunDocDBHelper --silent
```

Use `DOCDB_HELPER_OUTPUT_FILE` for matrix/count outputs:

```bash
export DOCDB_HELPER_OUTPUT_FILE="$RUNNER_TEMP/docdb-item-count.json"
export DOCDB_HELPER_ARGS_JSON=$(jq -cn \
  --arg queue_name "${QUEUE_NAME}_output" \
  --arg max_workers "$MAX_WORKERS" \
  '["count_docdb_items.py","--queue-name",$queue_name,"--max-workers",$max_workers]')
rcc run -t RunDocDBHelper --silent
```

Avoid `python3 scripts/helper.py` in active jobs; it bypasses the RCC environment, package pins, truststore setup, and `PYTHONPATH`.

## Matrix Workers

DocDB workers should reserve atomically from one shared queue:

```yaml
strategy:
  matrix: ${{ fromJson(needs.producer.outputs.matrix) }}
  max-parallel: 3
  fail-fast: false
```

Set:

```yaml
RC_WORKITEM_ADAPTER: robocorp_adapters_custom._docdb.DocumentDBAdapter
DOCDB_BASE_QUEUE_NAME: ${{ needs.setup.outputs.queue_name }}
RC_WORKITEM_QUEUE_NAME: ${{ needs.setup.outputs.queue_name }}_output
RC_WORKITEM_OUTPUT_QUEUE_NAME: ${{ needs.setup.outputs.queue_name }}_results
CONSUMER_ID: ${{ matrix.shard_id }}
```

There is no need to pre-shard JSON files when the adapter owns reservation. Keep `max-parallel` at or below the runner scale-set capacity.

## Evidence And Artifacts

Store evidence even on worker failure:

```yaml
- name: Store consumer log in DocDB
  if: always()
  run: |
    cp output/log.html "$RUNNER_TEMP/consumer-shard-${{ matrix.shard_id }}-log.html"
    export DOCDB_HELPER_ARGS_JSON=$(jq -cn \
      --arg run_id "${{ github.run_id }}" \
      --arg consumer_id "${{ matrix.shard_id }}" \
      --arg file "$RUNNER_TEMP/consumer-shard-${{ matrix.shard_id }}-log.html" \
      '["store_artifact_docdb.py","--artifact-type","consumer_log","--file",$file,"--run-id",$run_id,"--consumer-id",$consumer_id]')
    rcc run -t RunDocDBHelper --silent
```

Do the same for screenshots, reporter logs, retry logs, and reporter summaries. The dashboard job should download artifacts from DocDB, then upload only the dashboard/report/debug bundle to GitHub artifacts for convenience.

## Tests Worth Keeping

Add tests that lock the workflow contract:

- helper scripts are invoked through `RunDocDBHelper`,
- active workflow `rcc` lines include `--silent`,
- production tasks do not pass `-e devdata/env-docdb-*.json`,
- DocDB URI is loaded through the helper and not exposed as a job output,
- environment/ref pairing is validated before runner-heavy jobs,
- retry screenshots/outbox helpers receive the results queue name,
- local DocDB env files use concrete local values, and
- local scripts support both file-adapter and DocDB modes when both are retained.
