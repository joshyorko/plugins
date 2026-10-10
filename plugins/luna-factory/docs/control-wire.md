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

Presentation is bounded independently of retained SQLite history: task titles use
at most 1000 UTF-16 units, task attempt IDs show the most recent 64, the attempt
projection shows the most recent 256, and worker identity rows show at most 64.
Older recorded history remains authoritative in SQLite; omitted projection rows
are not proof that execution never occurred or that a worker stopped.

Targeted mutation requests add optional `expected_revision`; omission keeps older
callers compatible but does not bypass fresh server action guards. Decision answers
retain `expected_decision_id` and answer fingerprints. An identical already-recorded
answer is a no-call retry even if the caller's revision is old; a different answer
or stale decision ID fails closed.

`reconcile_factory_run` is visible to both the model and app and observes existing
native execution. Its typed `run_id`/`expected_revision` schema is present in the
model-filtered catalog whenever presentation advertises it. It must
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

The inspected toolkit PR #11 head is
`09018681233d70305e70c68c8dbe36879ab66675`, whose contract fixture still pins
Plugins `f2c81429db651c0bfe9da7a22b3853dfb6d9c9ac`
(`tests/fixtures/luna-contract.json:2`). Its compiled-product check requires exact
canonical tool equality and the saved schema snapshot
(`tests/luna_product_contract.py:18`, `:58`); those old bytes are not acceptance
for the onboarding/branding checkpoint or this alignment.

Handoff to that owner: pin the final tested Plugins SHA, explicitly refresh the
compiled-product fixture/expected canonical catalog from 12 to 15 tools, verify
the new initialization icon and additive control/presentation fields, and preserve
optional `expected_revision`/diagnosis argument pass-through and decision-answer
fencing. The model-visible catalog now has eight tools, including read-only
reconciliation. Discovery and registration remain app-only. The bridge must not
drop an advertised model-callable recovery action when updating its exact pin.
The bridge's subset/visibility check and direct-call allowlist remain relevant
(`scripts/luna_bridge.py:48`, `:95`). Re-run its real compiled-product and browser
approval/restart/history gates against the new exact bytes. This PR does not edit
or restart the toolkit/Executor stack or claim those gates passed.

The recoverable live Cutover package/database stays at 6cc2062 until a complete
exact tested production-path checkpoint is safe to use. No real first run,
ChatGPT-side registration, merge, release or deployment is performed by the swarm.

This is historical swarm evidence. The [October 9 maintainer decision](https://github.com/joshyorko/plugins/issues/59#issuecomment-6089593813) authorizes verified integration, release and safe activation; it does not prove cessation of that preserved owner or permit replacing its service while ownership is unknown. See [integration readiness](integration-readiness.md) for the separate code and live acceptance gates.


## Planning graph contract, LF-01 through LF-06

This slice added five model-and-app tools (catalog then: 20 total, 13 model-visible).
The current catalog is 26 total, 18 model-visible; see [Campaign entity](#campaign-entity-77).
All structured results now declare typed output schemas, including nested graph,
control, presentation, settings and backend data. Existing native action methods
retain their execution and evidence gates.

| Tool | Input | Result |
| --- | --- | --- |
| `create_factory_graph` | Existing bounded `start_factory` request fields | `{graph, proposal:null}`; immutable planning-only record, no claim or native execution |
| `get_factory_graph` | `run_id` | Same graph envelope, current ledger revision |
| `get_factory_backends` | Empty object | Configuration-only target catalog, separate advertised/enabled/qualified operations, unknown authentication and entitlement |
| `propose_factory_change` | `run_id`, required `expected_revision`, `idempotency_key`, typed `change` | Persisted proposal and revised graph |
| `apply_factory_change` | `run_id`, `change_id`, required `expected_revision` | Applied proposal and revised graph; exact retry is a no-op |

`graph` contains `run_id`, `revision`, `planning_only`, `repository`, `nodes`,
`criteria`, `attempts`, `claim` and `changes`. Repository includes approved alias,
opaque local identity, base commit and exact source subject. New records retain a
local filesystem identity stamp as well as the original common-Git-directory claim
identity. Replacing the root or Git directory at the same path invalidates graph
mutation. Legacy records without this stamp are readable, but new graph mutations
are denied until a future explicit qualification mechanism exists.

Nodes project the existing Control task records. Additional `source` is nullable
or `{provider,repository_id,item_id,revision}`; `repository_id` is the graph's
opaque local repository binding, not a caller-selected filesystem path or a claim
of authenticated GitHub provenance. Provider/item/revision strings are supplied
source assertions. They confer no authority or accepted evidence. An external
item ID can be qualified with its immutable forge repository ID. Reimporting an
existing provider/repository/item at a new revision does not create another node.
`target_preference` is nullable or a configured planning target ID.

Changes are a closed tagged union:

- `import_candidates`: `nodes` with id, title, criterion_ids, dependencies and source.
- `set_dependencies`: `node_id` and explicit prerequisite IDs.
- `set_target`: `node_id` and `target_id`.

Imports remain CANDIDATE, without execution effects or owned claims. Dependency
changes reject missing nodes, duplicate dependencies and cycles. Only unattempted
candidate nodes are editable. Existing run ownership must be fully settled and
released before graph mutations; an active or unknown native execution is never
reassigned. Planning records can coexist beside an existing claim without touching
it, and all native action APIs reject their immutable planning-only mode.

Proposal actor is server-assigned `local_operator`, meaning the existing trusted
loopback/tunnel boundary. It is not a claim of per-user OAuth authentication.
There is no caller-provided actor/authorization field. Proposals bind source,
revision and canonical payload digest in the existing SQLite control journal.
Apply rechecks authorization and source, then performs one revision-fenced event.
Neither proposal nor apply changes budgets, generations, acceptance or native
identities. A planning run cannot later be converted into an executable run by
changing preference or calling Resume.

New `set_dependencies` and `set_target` commands reject unchanged values with
`graph_change_noop`. Dependency order is not meaningful; reordering the same set
is also unchanged. This rejection creates no proposal, event, or revision.
Rejected keys are not reserved in the durable journal, as with other validation
errors; replay protection binds successful recorded commands.
Revision/source checks and idempotency conflicts still run at their existing
boundaries. A matching saved request replays its recorded proposal; a matching
applied change replays its recorded result without another event. Previously
recorded no-op events remain readable and replayable. An unapplied historical
no-op proposal can be inspected but a new application is rejected unchanged.
Nothing rewrites or removes historical lineage.

The workbench separates task selection, proposal saving, and application. Task
selection only changes app context. A proposal is a durable review draft; applying
it requires explicit UI confirmation of that proposal and current revision. The
server retains final authority and rechecks eligibility at both mutation boundaries.

### Settings and app context

The canonical settings update is now `{set:{...changedFields}}`; flat arguments
are rejected. The result is `{values:{...allEffectiveFields}}`. Read retains
`{schema,values}`. Partial updates merge atomically under one store lock. Invalid
saved defaults fall back to current approved values; absent profiles omit the
profile field and schema property. Deploy only matching server/UI bytes; an older
flat-argument workbench is not compatible with this host-contract correction.

Production `HostBridge` negotiates structured or text model context and uses the
published `OpenAIExtensions.modelContext.getCurrent()`/`update()` lifecycle.
Selection includes bounded run/node IDs and observed graph revision. Restored IDs
are validated through graph reads; attached text is not workflow truth. Explicit
host removal suppresses automatic refresh reattachment. Opaque update IDs suppress
own echoes. Each mounted app keeps separate selection and attachment state; the
extension does not promise cross-instance shared selection.

Native-local is a planning preference only. The discovery catalog marks every
execution route ineligible pending exact target/authentication/entitlement and
lifecycle qualification. Existing native execution behavior is retained; this
catalog does not retroactively certify it. CAS, experimental cloud CLI, newer
Cloud and GitHub-mediated routes remain distinct and unsupported for dispatch.

### Verification and integration handoff

The opt-in `ui/test/graph_e2e.test.ts` runs the compiled Rust daemon with a temporary
SQLite ledger, an isolated clone of this repository and no native executable. It
imports the captured real issue #61, inspects through the model-filtered catalog,
selects through the production controller, publishes node context through the
actual App/AppBridge protocol, changes preference, and compares both interfaces'
revision and audit state. This is a synthetic host, not live ChatGPT acceptance.

Run after `cargo build --locked` and `npm run build`:

```sh
cd plugins/luna-factory/ui
LUNA_GRAPH_E2E=1 npm test -- --run test/graph_e2e.test.ts
```

Toolkit PR #11 and any deployed host must refresh their exact source/catalog/schema
pins to the current catalog (26 total, 18 model-visible after #77), preserve the settings wrapper and
required graph revision fields, and qualify model context/remount/removal against
the actual host. No tunnel, Executor, registration or production package is changed
by this slice. Live replacement remains blocked while original cessation is unknown.

### Context clear ordering

The bridge tracks each outstanding write as clear or attachment. While an app-originated clear is still awaiting acknowledgment, its first null notification cannot suppress a newer explicit selection queued behind that write. A second null, or a null without such a newer selection, remains a removal. No expected-clear token survives acknowledgment. Since the host's bare null has no update ID, a post-acknowledgment null is treated as removal even if a host delayed it; the bridge cannot infer undocumented causal order. Nonempty writes racing genuine removal still settle and clear again.

## Bounded owner continuation (LF-07)

Native owner reports now include a required nullable `continuation` field. Null
means stop; older reports without the field also stop. A non-null request is:

```json
{"kind":"next","task_id":"admitted-task","diagnosis":null}
```

or `kind: "repair"` with `diagnosis: {summary, check_refs}`. The server accepts a
request only from a live completed owner event with a validated current-subject
QUIESCENT report, explicit null blocker, and no pending decision. It revalidates
repository identity/authority, source, stopped owner/descendants/terminals, claim,
dependencies, effects, deadline and attempt bounds. The requested task must match
the admitted selection. Next work must be necessary, READY and never attempted.
Repair must concern the just-returned task and exact current failed observations;
its retained diagnosis is server-labelled `observed_failure`. Model text cannot
supply operator-semantic authority or reset any budget.

The existing journal records terminal settlement, diagnosis and the next dispatch
intent before `turn/start`. The attempt retains lineage and increments only the
dispatch generation. The existing observer is retained, including notifications
that arrive before acknowledgement. An unknown acknowledgement holds the claim
and requires correlation of that same dispatch. Direct recovery/startup/reconcile
never executes a report's continuation request. A subsequently observed live
completion of an exactly correlated active dispatch can request new work normally.

The model and app still read/control the same canonical run revision. No new MCP
tool, scheduler or execution backend is added. Manual continuation of a fresh
admitted task also no longer consumes repair allowance; existing decision-answer
idempotency remains unchanged. Legacy unstamped runs retain manual recovery but
cannot enter new automatic continuation. Planning-only records never dispatch.

## Campaign entity (#77)

This slice adds four model-and-app tools. Total catalog: 26; model-visible: 18.
App-only tools are unchanged. Nothing here dispatches native work, acquires a
claim or qualifies execution.

| Tool | Input | Result |
| --- | --- | --- |
| `create_factory_campaign` | `repository`, `parent:{provider,item_id,revision,display?:{number?,url?}}`, `title`, `objective`, `acceptance` (1 to 32), `non_goals`, `finish`, `profile`, `idempotency_key` | `{campaign, graph}`. The graph is created by the same planning admission as `create_factory_graph` and is planning-only. |
| `list_factory_campaigns` | optional `limit` (1 to 100, default 20) | `{campaigns}`, newest first. Read-only. |
| `get_factory_campaign` | `campaign_id` | `{campaign}`. Read-only. |
| `promote_factory_campaign` | `campaign_id`, `expected_revision`, `idempotency_key` | Always an `execution_not_qualified` tool error in this version. |

### Record

Campaigns persist in a new SQLite `campaigns` table, added with the store's
existing `CREATE TABLE IF NOT EXISTS` style. The schema version stays 2, and an
older binary ignores the table. The table keeps the idempotency key unique, the
planning run unique (a foreign key to `runs`), and the triple (repository
identity, parent provider, parent item) unique.

- `repository` is the approved alias. `repository_identity` is the graph's
  opaque local identity (the same value as `graph.repository.identity`), never a
  filesystem path. A different clone or a replaced root is a different identity.
- `parent` is validated and bounded like graph sources: `provider`, `item_id` and
  `revision` are bounded IDs. `display.number` is a positive safe integer and
  `display.url` is an `https://` URL of at most 2048 bytes. These are supplied
  source assertions: they grant no authority and never satisfy a criterion.
- `planning_run_id` is the planning graph. `run_ids` (at most 64) lists execution
  runs and is empty today.
- `finish` is capped by the repository's `max_finish`.
- `status` is `planned | promoted | finished | stopped`. Only `planned` is
  reachable. Because no promotion transition exists, this version refuses to read
  a record with another status or with `run_ids` (`campaign_record_corrupt`), and
  it also rejects a record whose indexed columns disagree with its payload.

### Determinism

The fingerprint is the SHA-256 of the typed request. An empty `display` is the
same as an absent one. The same key with the same fingerprint replays the
recorded campaign: it reruns the planning admission's replay checks (alias remap,
repository identity) and never admits a second graph. The same key with a
different fingerprint fails with `campaign_idempotency_conflict`. Any other key for
an existing (repository identity, provider, item) fails with
`campaign_parent_exists`. That check runs before any write, so a rejected request
leaves no orphan planning graph and no journal event.

The planning graph uses the derived key `campaign-plan-<sha256(key)>` and a fixed
inert budget: capacity 1, no repairs and a 30 second window. That budget is always
within operator limits and is stable for replay. Planning records never dispatch,
so execution runs will carry their own budgets.

### Projection

A campaign result contains the record fields above, without the key or the
fingerprint, plus:

- `planning`: `{run_id, revision, planning_only: true, tasks:{total, candidate, ready, running, verify, done, blocked}}`,
  read from the recorded ledger. A campaign read never writes and never refreshes the source.
- `runs`: `{id, state, updated_at}` for each linked run.
- `promotion`: `{allowed: false, reason}`.

The output schema pins `planning.planning_only` to `true` and `promotion.allowed`
to `false`, so the projection cannot claim execution. The title goes through
`safe_summary` redaction. A display URL that carries a credential-like marker is
omitted. The UI labels the parent number "reported by GitHub".

### Promotion

`expected_revision` is the planning graph's control revision (`planning.revision`).
The command rejects with `execution_not_qualified` while
`capabilities.execution.eligible` is false, and one shared function feeds both
values. That is always the case in this version. The check runs before any campaign,
graph or journal state is read, so promotion writes no event, changes no revision
and records no key. It never upgrades the planning graph. If eligibility were
enabled without a promotion transition, the command would still fail closed with
`campaign_promotion_unavailable`. Duplicate-promotion replay and the stale-revision
fencing of a successful promotion are deferred to #71. Today every attempt is the
same write-free rejection.

Revision fencing, claims, evidence and the run and replay contracts are unchanged.
A campaign's graph is an ordinary planning graph: `propose_factory_change` and
`apply_factory_change` fence it as before, and every native action rejects it.

Deployed host handoff: the toolkit and deployed host pins must move to 26 total and
18 model-visible tools. The OCI canaries (`container/acceptance_canary.py`,
`container/planning_graph_canary.py`) still pin the released 0.2.1 catalog of 22
tools and its UI hash. A release that ships this slice must update them. This
slice does not change them or any deployed service.
