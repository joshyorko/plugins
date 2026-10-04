# Native protocol evidence

Captured against `codex-cli 0.159.2` on 2026-10-04. These files do **not** establish
the complete ChatGPT/tunnel/inference vertical slice.

## Reproduce the read-only audit

```sh
python3 plugins/luna-factory/scripts/audit_app_server.py \
  --output /tmp/luna-inherited-audit.json \
  --schema-output /tmp/luna-schema-contract.json

# Explicitly weaker: clean-profile protocol/catalog evidence only.
python3 plugins/luna-factory/scripts/audit_app_server.py --clean-config \
  --output /tmp/luna-clean-audit.json
```

The script generates JSON schemas with the installed binary, initializes stdio,
reads `model/list` and `skills/list`, then closes and reaps the exact process. Its
request allowlist contains no turn or mutation method. It never copies credentials,
changes global config, chooses another provider, or prints raw stderr/config.

- `schema-contract-0.159.2.json`: generated schema hashes, required/property names,
  native skill input schema, and relevant exact event-item schema excerpts
- `clean-catalog-audit.json`: actual native clean-profile handshake and catalog;
  exact `gpt-6-luna` advertised with low/medium/high/xhigh/max efforts
- `inherited-config-audit.json`: inherited-profile initialize blocked before
  catalog access by native SQLite state runtime initialization under the mounted
  read-only Codex home
- `fake_app_server.py`: **synthetic** transport fixture, never an inference harness

The clean catalog is capability evidence only. It does not prove that the user's
provider/profile/authentication is usable. The clean profile did not discover the
canonical Luna Factory skill through `skills/list`. Explicit native skill input
is schema-supported, but actual skill loading and an owner/worker inference turn
remain unproved. Requested/configured routing must never be relabeled as observed
execution routing. No inference was requested in this audit.

The Rust transport itself also passed the read-only live test:

```sh
# Use a temporary empty CODEX_HOME only for an explicitly clean-profile check.
LUNA_FACTORY_LIVE_AUDIT=1 LUNA_FACTORY_CODEX_BIN=/path/to/codex \
  cargo test --locked --manifest-path plugins/luna-factory/server/Cargo.toml \
  --test native_live -- --ignored
```

## Version-specific findings

- `codex app-server proxy --help` describes a stdio proxy to the **running** native
  app-server control socket and supports `--sock SOCKET_PATH`. The client can use
  exactly these trusted arguments to reconnect to an operator-owned daemon. No
  daemon was started, bootstrapped, stopped or reconfigured during this audit.
  Killing this proxy is not proof that daemon-owned work stopped.
- `codex app-server --profile NAME` is rejected by argument parsing. Top-level
  `codex --profile NAME app-server --stdio` parses, then rejects actual startup
  with `--profile only applies to runtime commands and codex mcp`. A help-only
  success is not profile support. Do not silently ignore a selected profile.
- A clean `--strict-config` launch plus `config/read` verified the current worker
  setting `agents.max_concurrent_threads_per_session = 2`. Do not send the older
  remembered `agents.max_threads` key.
- `thread/start` has a trusted `config` map, `cwd`, and `model`. No helper sets
  `approvalPolicy`, `sandbox`, `modelProvider`, or `serviceTier`; installed native
  configuration remains authoritative for those fields.
- `turn/start` receives `{type:"skill", name:"luna-factory", path:<SKILL.md>}` and
  text input, with `model`, `effort`, and optional `outputSchema`. This uses native
  skills rather than copying skill text into a second prompt stack.
- `turn/steer` requires `expectedTurnId`; `turn/interrupt` requires `turnId`.
  Neither transport acceptance nor interrupt acceptance establishes cancellation.
- Descendants use explicit `parentThreadId` or
  `source.subAgent.thread_spawn.parent_thread_id`, persisted `thread/list`, and
  `thread/loaded/list` plus metadata reads for ephemeral loaded threads. Combine
  this with the runtime's previously recorded spawn IDs. Pagination bounds,
  missing threads, event lag, unknown status, and disconnected old execution
  leave ownership uncertain. Only observed `idle` is idle evidence; `notLoaded`
  is not stopped proof.

## Relevant native events

- `turn/completed`: `params.threadId`, `params.turn.id/status/items`
- `item/completed`: `params.threadId/turnId/item/completedAtMs`
- `thread/status/changed`: `params.threadId/status`
- `agentMessage` item: `text`, optional `phase` (`commentary` or `final_answer`)
  and optional `delivery`. The schema explicitly warns that absent phase is
  unknown; do not interpret commentary or asynchronous messages as convergence.
- `collabAgentToolCall` item: `tool`, `senderThreadId`, `receiverThreadIds`,
  `agentsStates`, `status`. For `spawnAgent`, receivers are new child identities.
  `model` and `reasoningEffort` are **requested** values, not execution telemetry.

Server-originated approval requests pass through the bounded internal event
channel. They are never automatically approved. Raw native events/results are
not safe public tool/UI payloads; the control plane must select and redact them.

## History envelope correction

Codex 0.159.2 `thread/items/list` returns `ThreadItemEntry` records with required
`turnId` and `item` fields. They are not raw `ThreadItem` values. The adapter
validates and unwraps that envelope, and rejects wrong-turn data before using
items as process-exit or acceptance evidence. The committed schema audit records
the actual installed entry schema. Synthetic history fixtures use the same shape.

## Lost acknowledgement correlation

`turn/start.clientUserMessageId` is persisted before dispatch. Native user-message
history exposes it as `ThreadItem.userMessage.clientId`, inside the turn/item
history envelope. This is a correlation key, not a claim that `turn/start` is
idempotent. Recovery finds a unique accepted turn and reads its status/result
without reissuing `turn/start`. Missing, ambiguous or unavailable history keeps
the claim blocked. The runtime does not infer failed execution from a timeout.

The official current app-server turn-start test verifies `clientUserMessageId`
becomes `userMessage.clientId`:
https://github.com/openai/codex/blob/main/codex-rs/app-server/tests/suite/v2/turn_start.rs
The installed 0.159.2 schemas also expose both fields. Synthetic fixtures test
completed-but-unacknowledged dispatch, restart, missing correlation and ambiguity;
these do not establish native live execution proof.

## Experimental owned background-terminal control

The installed 0.159.2 `generate-json-schema --experimental` output includes
`thread/backgroundTerminals/list` and `thread/backgroundTerminals/terminate`.
Initialize opts into `capabilities.experimentalApi` for this connection only.
List records identify `processId` and `itemId`; raw commands/cwd are not retained
or sent to the workbench. Stop intent is durable, identity is re-read before
termination, and a lost mutation reply is not blindly retried.

A source audit explains why the native boolean is insufficient proof:
- [process manager](https://github.com/openai/codex/blob/de3721a7be07054c8c2a41102b5a501f34155361/codex-rs/core/src/unified_exec/process_manager.rs) calls `terminate_confirmed` before removing its tracked entry
- [unified process](https://github.com/openai/codex/blob/de3721a7be07054c8c2a41102b5a501f34155361/codex-rs/core/src/unified_exec/process.rs) signals logical exit after the local terminate call
- [local PTY process](https://github.com/openai/codex/blob/de3721a7be07054c8c2a41102b5a501f34155361/codex-rs/utils/pty/src/process.rs) discards the kill result; it does not await actual exit in `terminate`

The fixture distinguishes positive command exit, acknowledgement plus vanished
entry without exit evidence, unsupported observation and uncertain stop outcome.
Only actual terminal command evidence can close command ownership. These tests
are synthetic; they do not prove a real native process tree has stopped.
