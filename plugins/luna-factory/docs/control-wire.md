# Control and presentation boundary

The existing run JSON keys and uppercase state vocabulary remain compatible.
The server adds `control` and `presentation` below. Their revisions are the same
nonnegative safe JSON integer. Old payloads lacking these additions are unverified;
the new UI reads/refreshes them but does not invent permission to retry or complete.

## Control schema 1

`control` contains `schema_version: 1`, `revision`, `intent_generation`,
`dispatch_generation`, `criteria`, `tasks`, `attempts`, bounded effect summaries,
and `child_policy: "cooperative_unverified"`.

- Criterion: `id`, `description`, `status` as proven/failed/unproved, nullable
  stable `reason`, and bounded `check_refs`.
- Task: `id`, `title`, `criterion_ids`, authoritative `dependencies`, typed-state
  and admission projections, nullable reason/owner thread, and `attempt_ids`.
- Attempt: `id`, `task_id`, intent/dispatch generation, exact `subject`, nullable
  thread/turn identity, and an observation/acceptance status. Identity/acknowledgement
  alone is not execution proof.
- Effects are bounded metadata, not model-readable raw commands/logs. The UI
  does not render unknown effect objects or infer settlement from them.

## Server presentation

`presentation` contains:

| Field | Contract |
| --- | --- |
| `revision` | Equals control revision. |
| `primary_action`, `actions` | Descriptors with `kind`, human `label`, stable `reason`, nullable tool name, `allowed`. Kinds are wait/refresh/answer/steer/cancel/resume/reconcile/inspect. |
| `criteria` | proven/failed/unproved/mandatory counts matching the control criteria. No percentage. |
| `result` | kind and label: finished_verified/stopped_unresolved/working/needs_input/unverified. Finished requires all mandatory current proof; stopped requires cessation evidence, not an empty ledger. |
| `owner` | Nullable thread/turn IDs and active/idle/unknown liveness from observed facts. |
| `workers` | Bounded observed thread IDs with active/idle/unknown liveness; created identities do not become active workers by inference. |
| `budget` | Remaining time, remaining repair attempts and repairs used under original limits. |
| `claim` | held flag and owned/released/foreign/unknown status; no age-based unlock. |
| `deliverable` | local_candidate/push/pr_ready, verified/unproved, exact subject and nullable reference. A URL/prose is not verified PR delivery or merge authority. |

One safe primary action is visible. Current server-authorized cancel/steer controls
may remain folded as secondary actions. UI action display is not enforcement;
Factory rechecks the same current revision, source, authority, ownership, liveness
and budget at the effect boundary. Contradictory control/presentation revisions or
counts must fail parsing rather than displaying false completion.

Targeted mutation requests add optional `expected_revision`; omission keeps older
callers compatible but does not bypass fresh server action guards. Decision answers
retain `expected_decision_id` and answer fingerprints. An identical already-recorded
answer is a no-call retry even if the caller's revision is old; a different answer
or stale decision ID fails closed.

`reconcile_factory_run` is app-only and observes existing native execution. It must
not start/resume a turn, interrupt, terminate or retry an uncertain effect. Native
approval requests remain in Codex, never an app-answer grant. Repair diagnosis is
a separately bounded optional resume input, not a changed goal or budget reset.

## Compatibility and delivery handoff

The previous public server exposed 14 tools. This slice adds read-only native
reconciliation and optional revision/diagnosis fields plus additive result data.
The exact final tool/schema identity and published head must be recorded in the
PR evidence before accepting mcp-tunnel-kit PR #11 against new bytes. Do not modify
that repository or claim its old pin passed this contract. Portable/compatibility
manifests, standard MCP icons and existing UI entrypoints remain unchanged.

The recoverable live Cutover package/database stays at 6cc2062 until a complete
exact tested production-path checkpoint is safe to use. No real first run,
ChatGPT-side registration, merge, release or deployment is performed by the swarm.
