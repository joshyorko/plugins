# LF-08 bounded CAS integration

Implementation: `35fdf6e714d5ae606579fdfd884b9fb8ecee35f0`, draft [PR #64](https://github.com/joshyorko/plugins/pull/64), based on PR #63 `d97e8f263bed9b710301d1d9804611d3e2df39b7`. This is a working **read-only adapter and durable planning boundary**. It is not a qualified CAS execution adapter. Feature development stops here pending the [integration acceptance gates](integration-readiness.md).

## Implemented boundary

`inspect_factory_cas` is visible to both model and app. It accepts an operator-configured alias; optionally, a run ID and recorded planning request ID together. It sends only `inspect_target`, `read_dispatch_receipt`, and `read_thread` to CAS. The app-only Lanes timeline read added in #75 is described [below](#observed-agent-timeline-75). Graph/status reads remain free of CAS network calls. The existing workbench does not add a CAS execution control.

The operator may add this optional field to a **new isolated** configuration:

```json
{
  "cas_targets": {
    "preview": {
      "endpoint": "http://127.0.0.1:8080",
      "package": "codex-action-server",
      "target": "local",
      "cwd": "/absolute/path/to/disposable/repository"
    }
  }
}
```

`get_factory_backends` returns the configured aliases. Call `inspect_factory_cas({"target_alias":"preview"})` for target inspection. Package selection is explicit: the monolithic package is `codex-action-server` even with its `observe` action profile; `codex-observe` is a separately assembled split package. Neither a profile nor a package name proves authentication or authorization.

Only literal loopback HTTP origins are accepted. Credentials, hostname resolution, remote endpoints, custom base paths, query strings, fragments, redirects and environment proxies are excluded. Each request has a ten-second timeout and a streamed 1 MiB response limit; at most three requests are made. A privately authenticated remote gateway requires a separately reviewed adapter extension; this slice does not read bearer tokens or forward credentials. Operator-approved loopback forwarding is not independently authenticated by this client.

The optional `Control.cas_requests` map uses the existing SQLite journal. `CasPlanned` records a generated request UUID, fixed operation, immutable endpoint/package/target/CWD, local graph stamp, source subject, task and bounded canonical payload digest. It requires a planning-only run, no native owner and no claim. The field defaults empty and is omitted when empty. The low-level Store/Event preparation seam is exercised in tests; **there is no public planning-request creation tool, CAS start tool or dispatcher**. A prepared plan is not an admitted execution reservation and must not later be silently upgraded to one.

The Factory digest covers its normalized operation/request payload; it is **not CAS's internal receipt fingerprint**. Public CAS receipts omit the fingerprint and target/CWD provenance. Recorded local bindings prevent alias drift but cannot establish remote identity or defeat endpoint substitution. Every response therefore retains `binding_verified:false` and `execution_eligible:false`.

Receipt observations never write run state, acceptance, audit revisions, attempts, budgets, owner identity, claims or settlement. A missing/unknown/in-progress receipt does not mean no external effect occurred. Even CAS states named accepted/completed/failed/interrupted are observations, not Factory acceptance or cessation proof. No new key, mutation retry, interruption or authentication follows a receipt. Exact thread/CWD reads remain untrusted observations; they never run local file predicates against alleged remote work.

## Observed agent timeline (#75)

`read_factory_agent_timeline {run_id, thread_id, cursor?}` is an **app-only** read (`_meta.ui.visibility: ["app"]`, `readOnlyHint: true`) for the Lanes view. It is display data labelled "Observed in Codex". It is never Factory proof, never creates "Needs you" attention and never changes the Map.

**Read allowlist.** The adapter may call exactly these CAS actions, all reads: `inspect-target`, `read-dispatch-receipt`, `read-thread`, `get-thread-snapshot`, `list-thread-items`, `list-thread-turns` and `list-thread-timeline`. `CasClient` rejects any other action name before it builds a request. One client serves one tool call and refuses a fourth request. The existing limits are unchanged: literal loopback HTTP origins only, no credentials, no proxies or redirects, a 10 s timeout per request and a streamed 1 MiB response limit. Every response keeps `binding_verified:false` and `execution_eligible:false`.

**Exact binding.** The operator binds a repository to a configured CAS target:

```json
{
  "cas_targets": {"preview": {"endpoint": "http://127.0.0.1:8080", "package": "codex-action-server", "target": "local", "cwd": "/absolute/path/to/repository"}},
  "cas_timelines": {"fixture": "preview"}
}
```

`cas_timelines` maps a Factory repository alias to a CAS target alias. It is optional and defaults to empty, so the feature is off unless the operator enables it. The request scope is always `{target, cwd}` from that configuration plus a `thread_id` the Factory already knows: the run's owner thread, or one of its owned or active worker threads. The tool input has no target, CWD or endpoint field (unknown fields are rejected), and any other thread fails before HTTP. A cursor is forwarded only if this server issued it for that thread.

**Reads per call.** One call makes at most two requests:

1. `get-thread-snapshot` checks the echoed target, CWD and thread ID client-side and reads status, waiting flags, the latest turn's error code, `native_updated_at` and the snapshot `revision`. The latest item's text is never read.
2. `list-thread-items` reads one newest-first page (`limit: 100`, `sort_direction: "desc"`), only when the snapshot changed.

`list-thread-turns` and `list-thread-timeline` are allowlisted, with strict parsers and unit tests, but Lanes does not call them. Timeline item entries carry no per-item timestamps, and CAS gates `thread/timeline/list` to native 0.160.1. Item entries carry `startedAtMs`/`completedAtMs`.

**Vocabulary.** Each event is `{at, kind, summary, thread_id}`. `at` is Unix seconds from the item's `completedAtMs`, or `startedAtMs` when that is absent. `kind` is one of `assigned`, `command`, `file_change`, `commit`, `pr_opened`, `check_result`, `subagent_spawned`, `message`, `waiting` or `error`. `summary` is at most 160 characters and passes through `safe_summary`.

- **Commands.** Classified from program words only, for example `cargo test`, `git commit` or `gh pr create`. Arguments, paths, environment values and output are never shown.
- **File changes.** Reported as counts only.
- **Agent messages.** Contribute only their first prose line. Long opaque tokens are withheld, and code blocks are never shown.
- **Snapshot events.** Waiting flags and a failed latest turn become `waiting` and `error` events at the thread's `native_updated_at`.
- **Dropped items.** Reasoning, plans, compaction, image, review-marker and unknown item types are dropped and counted in `omitted`, as are items with no timestamp.
- **Children.** `children[]` is derived from `collabAgentToolCall`/`spawnAgent` items as `parent_thread → receiver_thread`.

The result is `{run_id, thread_id, observed, status, source:"codex-action-server", events[≤100], children[≤64], omitted, next_cursor, freshness}`.

**Unavailable is a result, not an error.** A repository without a binding returns `status:"unavailable"`, `reason:"cas_timeline_not_configured"` and a human `detail`, with no HTTP. CAS failures collapse to a closed set of reasons with no remote text: `cas_unavailable`, `cas_action_failed`, `cas_binding_mismatch`, `cas_response_too_large` and `cas_invalid_response`. Unknown runs, foreign threads, caller-supplied scope and unissued cursors are request errors.

**Cache.** Each configured alias and thread has an entry in a bounded in-memory cache: at most 64 threads and 4 older pages per thread. Memory rather than SQLite is deliberate. Observations are disposable, and keeping them out of the ledger means they can never reach the control journal or need a migration. A restart costs one fresh read.

- **Within 20 s.** Repeated calls are answered from memory with no CAS request, including cached failures.
- **After 20 s.** The snapshot is re-read with the cached `revision`. An unchanged thread costs one small request (`changed:false`). Only a changed thread reads a new item page.

With the workbench's 30 s poll, and its own 25 s per-agent bound, each rostered agent costs at most one snapshot per poll while Lanes is visible, plus one item page when that thread changed.

**Observations stay observations.** `read_agent_timeline` reads the run once, without holding the SQLite lock across network IO, and never writes. A fixture test compares every ledger table row for row, and the run projection, before and after observed reads. The reads include commits, PR creation, failed and passing checks, approvals, failures and CAS errors. Run state, criteria, claims, attempts, budgets and receipts stay unchanged.

**Operator endpoint is not loopback.** The operator's current CAS endpoint, `http://172.30.86.1:8088`, is not a literal loopback origin. This adapter rejects it at configuration load (`cas_literal_loopback_required`). To use the timeline against it, the operator must approve a loopback forward, for example a local port forward bound to `127.0.0.1`, and configure that loopback origin. A separately reviewed remote adapter is the alternative. A forward is not independently authenticated by this client. A configured CAS target is configuration, not proof that a live environment exists. This slice never contacted that endpoint or any running service.

## Pinned upstream contract and blockers

CAS was independently re-fetched at `bf0b3823e033b9b5abd86904e0a565d6b3586206`. HTTP paths are `/api/actions/<package>/<hyphenated-action>/run`, request bodies wrap `payload`, and Actions responses wrap `result` and nullable `error`. Source evidence: [models](https://github.com/joshyorko/codex-action-server/blob/bf0b3823e033b9b5abd86904e0a565d6b3586206/src/codex_shared/models.py), [inspection](https://github.com/joshyorko/codex-action-server/blob/bf0b3823e033b9b5abd86904e0a565d6b3586206/src/codex_shared/inspection.py), [execution](https://github.com/joshyorko/codex-action-server/blob/bf0b3823e033b9b5abd86904e0a565d6b3586206/src/codex_shared/execution.py), [receipts](https://github.com/joshyorko/codex-action-server/blob/bf0b3823e033b9b5abd86904e0a565d6b3586206/src/dispatch_receipts.py).

Execution remains hard-disabled because:

- CAS omits `account/read`; configured providers, rate limits and model catalogs do not establish subscription entitlement. No API-billed fallback is introduced.
- Per-action native sockets close at action completion. Durable callback ownership is unresolved in [CAS #11](https://github.com/joshyorko/codex-action-server/issues/11).
- Start actions cannot carry Factory's canonical skill input, owner output schema or client message correlation field. Combining thread creation and first turn differs from the existing persistent native client lifecycle.
- Logical target and CWD do not prove a resolved remote workspace/container UID or repository/source identity.
- Public receipts omit scoped fingerprint/provenance; acknowledgment is not proof of native execution or cessation.
- Remote artifact proof and complete owner/descendant/terminal cessation are unqualified. Native experimental method availability is version-specific: reviewed terminal operations at 0.160.1 cannot be inferred from the narrower 0.161.0 allowlist.

The official authentication/product distinctions are retained in [backend evidence](graph-backend-evidence.md#official-openai-capability-research); Cloud/GitHub execution is unchanged and unqualified. Native ownership, generations, READY gating, continuation, evidence and fail-closed recovery remain on their existing path.

## Verification

| Gate | Result |
| --- | --- |
| Rust suite | 175 passed; one intentionally ignored live-native audit; no failures. |
| Dedicated CAS tests | 4 parser/binding unit tests and 8 HTTP/SQLite boundary tests, included above. |
| Rust fmt / Clippy / build | Passed; lockfile includes reqwest 0.12.28 with no TLS/default features. |
| UI and graph acceptance | 123 passed, including compiled daemon/workbench demonstration; focused graph E2E repeated after the final build. Typecheck/build and bundled UI parity passed. |
| Packaging / repository | 29 packaging and 53 repository tests passed. |
| Pinned CAS unit contracts | 47 passed in a separate process. |
| Actual CAS HTTP/MCP boot | 2 passed against Actions Runtime 1.0.1, with absent/fixture native transport only. |
| Factory MCP → actual CAS HTTP | Passed using `scripts/verify_cas_boundary.py`, new empty ledger, disposable Git repository, absent native binary/socket and Python MCP client. Zero native dispatches. |
| Offline rollback rehearsal | PR60/PR63/LF08 binaries confirm forward reads, expected downgrade rejection and matching backup restoration on disposable ledgers; [exact evidence](rollback-verification.json). |

The real-runtime harness initially selected `codex-observe` while launching the monolithic observe profile; HTTP correctly rejected the wrong package route. Correcting the explicit package selection produced the accepted result. The upstream CAS contract tests and HTTP boot tests also exposed an import-isolation failure when combined in one Python process (`pydantic` class identity during MCP import). The same pinned tests passed separately; no upstream code was changed and no full CAS-suite pass is claimed.

The [retained transcripts](cas-verification-results.txt) and [real-runtime result](cas-runtime-evidence.json) identify commands, test names and artifact hashes. The runtime binary SHA-256 is `191f40cfebcadbd53bf7b14988098b983b99b4efc1aa8d8ac9e8ada6ad37ab3b`. Independent code review checked wire compatibility, transport bounds, journal immutability and unchanged claim/settlement behavior without actionable findings. This does not replace maintainer review.

Reproduce the cross-process inspection using the pinned CAS checkout's test virtualenv, installed test dependencies and downloaded pinned Runtime binary:

```bash
/absolute/cas/.venv/bin/python plugins/luna-factory/scripts/verify_cas_boundary.py \
  --cas-source /absolute/cas \
  --action-server /absolute/test-bin/action-server \
  --factory /absolute/plugins/plugins/luna-factory/server/target/debug/luna-factoryd
```

The harness creates and cleans only its own process groups and temporary files. Its target socket does not exist; it does not connect to an operator daemon. Actual ChatGPT context acceptance and real native/CAS inference remain unperformed.
