# Execution qualification (#71, first slice)

Ordinary Factory execution is denied unless the local operator has recorded a current
qualification for the exact execution profile. This slice adds the persisted record,
enforces it at every native effect path, and reports it in capabilities. It does not
qualify any profile itself and does not enable execution anywhere.

## Record model

An execution profile is derived from the operator config plus a profile alias:

| Field | Source |
| --- | --- |
| `factory_version` | `CARGO_PKG_VERSION` of the running binary |
| `transport` | `native_transport` (`stdio` or `existing_daemon`) |
| `codex_binary` | Absolute, symlink-resolved `codex_binary` path |
| `codex_binary_identity` | SHA-256 of the bounded executable read, cached by device, inode, size, mtime and ctime |
| `native_socket` | Optional configured socket path |
| `native_paths` | Explicit Factory-to-daemon path mappings, omitted when empty |
| `model` | `gpt-6-luna` (`native::LUNA_MODEL`) |
| `profile` | Profile alias |
| `effort` | The profile's configured effort |

The fingerprint is the sha256 of the sorted-key JSON of these fields. Changing any of
them, including rebuilding or replacing the Codex binary, changes the fingerprint and
denies execution until the operator records a grant for the new profile.

Grants live in the existing SQLite database, in an additive
`execution_qualifications(fingerprint PRIMARY KEY, profile_alias, payload)` table
created with `CREATE TABLE IF NOT EXISTS`. `user_version` is unchanged, so older
binaries still open the database. They never read the table, and they keep their own
behavior. The payload is
`{profile, provider_identity?, scope, max_capacity, evidence_sha256, evidence_ref, recorded_at, expires_at?, revoked_at?, recorded_by_uid, tty_name}`.
For `existing_daemon`, `provider_identity` prefers the Linux kernel peer binding:
SO_PEERCRED UID, SO_PEERPIDFD lifetime, matching procfs/PID namespace and the SHA-256
of the peer executable matching the configured Codex executable. Its name is
`linux-unix-peer-executable`, version `1`; its binary hash is not an inferred Codex
version. The preexisting `serverInfo` name/version/hash parser remains available
for protocol versions reporting it. Client-influenced user-agent text, Codex home
and platform fields never participate in identity. Kernel binding requires Linux
6.5+ and same-namespace visibility. Missing/inaccessible host PIDs fail closed.
Kernel identity is refreshed at each guarded first-write poll and by the monitor.
It attests the executable, not daemon launch arguments or account entitlement.

- `scope` is `ordinary` (may authorize Factory runs) or `qualification_only` (may
  authorize only the operator canary; never `start_factory`, continuation, repair,
  steer or resume).
- `max_capacity` (1 to 8, default 1) bounds the requested run capacity.
- `evidence_sha256` and `evidence_ref` identify the evidence file. Its content is
  never echoed.
- `recorded_by_uid` and `tty_name` identify the local operator session that confirmed the write.

## Reason codes

| Code | Meaning |
| --- | --- |
| `execution_not_qualified` | No grant for this profile, or the table or database is missing, unreadable or malformed |
| `execution_profile_mismatch` | Grants exist for this alias, but none matches the current fingerprint |
| `execution_qualification_revoked` | The matching grant was revoked |
| `execution_qualification_expired` | The matching grant expired |
| `execution_qualification_scope` | The matching grant's scope doesn't cover this use |
| `execution_capacity_not_qualified` | Requested capacity exceeds the grant's `max_capacity` |
| `qualification_provider_mismatch` | Existing-daemon initialize identity is absent or differs from the grant |
| `qualification_store_exposed` | Qualification state is inside a configured repository or writable workspace root |
| `qualification_clock_skew` | Wall time moved behind the persisted high-water time or the grant timestamp |
| `qualification_write_timeout` | A guarded native write did not finish within five seconds; the transport was invalidated |

MCP errors carry the bare code. Run blockers read
`execution_not_qualified: <code>; nothing was dispatched`.

## Enforcement

Each qualification check advances a persisted wall-clock high-water value in a
separate transaction, then reads qualification through a fresh read-only connection.
The final qualification check runs while the native writer
lock is held, at the transport's first write poll, immediately before bytes are handed
to the writer.

| Path | Check |
| --- | --- |
| `Factory::start` (`start_factory`) | After request validation and before `store.admit`. A refusal creates no run row, claim or receipt, and makes no native spawn or connection. |
| `launch` | At the top, inside the `thread/start` and `turn/start` dispatch guards, and again before the `turn/start` Dispatch intent. A guard refusal settles the not-sent thread-start intent instead of leaving it unknown. |
| `dispatch_existing_owner_locked` (continuation, repair, resume and answer dispatch) | At the top, before any Dispatch or EffectIntent event or receipt. Also before the Dispatch event and inside the `turn/start` guard. |
| Automatic continuation | Before the diagnosis event. The run blocks with the honest blocker. |
| `steer_at_revision` | Before EffectIntent and inside a guarded `turn/steer` send. |
| `resume_*` | Before any native connection, recovery read, claim or event. Also before the resume EffectIntent and inside a guarded `thread/resume` send. |

The checks run after the existing read-only authority validation, so configuration-drift
codes keep their precedence. Containment (`turn/interrupt`, terminal stop, cancel),
`reconcile`, startup passive recovery, planning (`create_factory_graph`, propose and
apply) and every read stay available without qualification.

A guard refusal is typed (`qualification::ExecutionDenied`). The guard runs after the
transport owns its write lock and is rechecked on every poll until the first bytes are
accepted. A revocation committed before that guarded write begins prevents the write; a
write already handed to the operating system may complete. A guarded write that has not
completed within five seconds is aborted and invalidates the transport; its run is
blocked with `qualification_write_timeout`.

While an execution run has an active native turn, the monitor checks qualification every
ten seconds and before every observed spawn, worker-start, or new-turn event. When a grant
is revoked, expires, or no longer matches, the monitor requests interrupts for active
owner and descendant turns and records `qualification_revoked`. A worker first reported
after that check is used only as a local interrupt target; it is not recorded as Luna-owned.

## Capabilities

`capabilities.execution` is `{eligible, reason, profiles:[{alias, status, reason, max_capacity}]}`.
`eligible` is true only when some configured profile has a current `ordinary` grant.
Path fields are never included. `max_capacity` is null unless the grant is current.

## Operator runbook (CLI only; there is no MCP write path)

```
luna-factoryd qualification-profile --config C --profile A
luna-factoryd record-qualification --config C --profile A --evidence FILE \
  --scope ordinary|qualification_only [--max-capacity 1] [--expires-in-days N]
luna-factoryd revoke-qualification --config C --fingerprint FP
luna-factoryd qualifications --config C
luna-factoryd qualification-canary --config C --profile A --workspace DIR [--output FILE]
```

`record-qualification` refuses unless all of these hold:

- The evidence JSON is readable and at most 1 MiB.
- Its `profile_fingerprint` equals the current fingerprint.
- The profile has no `codex_profile` override.
- The executable SHA-256 is available.
- For `existing_daemon`, a current kernel peer binding or the existing protocol
  `serverInfo` binding is available. A Codex 0.165.0 response lacking `serverInfo`
  requires the kernel binding; userAgent is never a substitute.
- The state directory is outside configured repositories, discovery roots, CAS working
  directories, `CODEX_HOME`, and `CODEX_WORKSPACES_ROOT` when configured.
- The command has TTY stdin and stdout, can open `/dev/tty`, and has no `CODEX_*`
  sandbox or thread marker. It prints the grant summary and reads the profile-name
  confirmation from `/dev/tty`, never stdin.

Scope `ordinary` also requires `qualification_run` to have:

- `harness: "luna-factory.qualification-canary.v1"`
- `outcome: "passed"`
- ordered `started_at` and `finished_at` values
- a non-empty `checks` list in which every check passed
- `cessation.verified: true`
- `cessation.scope: "native_owner_descendants_and_reported_terminals"` and
  `cessation.process_tree: "unverified"`

Recording again for the same fingerprint replaces the grant.

The database file and its directory are checked at record time and server startup. A
grant is not accepted when qualification state is under a configured or approved
repository or known Codex writable root. Qualification-state errors fail closed.

## Threat model

The CLI confirmation requires a local interactive operator session, a matching typed
profile name, and records the effective UID and TTY. Sandbox markers disable record and
revoke. Production effect methods require an opaque authorization created by the
qualification module; the public native API has no unguarded execution path.

This does not make hand-authored evidence independently trustworthy. Its digest and
reference preserve what the operator asserted; a manually written evidence file remains
an unverified operator assertion. Fixture tests of the canary do not qualify a real
profile. Recording a real grant still requires the interactive operator procedure.

## Canary harness contract

`qualification-canary` requires a current `qualification_only` grant for the exact
profile. The workspace must be an absolute, symlink-free, disposable git repository
root with no remotes. It must not overlap any configured or approved repository root
or discovery root, must not share their Git identity, and must not contain the
database. The canary uses no repository alias, admits no run and isn't reachable via MCP.

After interactive operator confirmation, the harness runs one capacity-1 native
owner turn with a 20-second work budget and 30-second total budget. It requests
one fixed file in the disposable repository, independently hashes its content,
rejects extra worktree changes, and checks cessation before producing evidence.
The qualification-only grant is checked again before native writes. It never
admits an ordinary run or starts an existing plan.

Cessation evidence covers the owner, reported descendant threads and reported
background terminals, including active command items. It explicitly records
`process_tree: "unverified"`; detached subprocesses outside native terminal
observations remain unverified. It does not claim operating-system process-tree
cessation. A failed or uncertain check produces failed evidence, never a grant.

For existing-daemon execution every repository/worktree, canonical skill, subject
CLI binary and disposable canary path requires explicit `native_paths` mapping,
including same-path deployments. Local files are validated in Factory's namespace;
only the declared daemon paths enter native frames. The mapping is profile-bound,
not proof of host visibility. See the prepared [operator canary runbook](execution-canary-runbook.md)
and `container/execution_paths.py` for a read-only comparison in both namespaces.
Cloud fixtures cannot prove Dakota's host view. The current private-PID-namespace
OCI service remains unqualified unless an operator separately approves and proves
a compatible process placement. This source change does not alter deployment.

## Remaining for full #71

- Campaign promotion now admits a new executable snapshot from one approved
  revision, with a durable digest and atomic claim/link. It never upgrades a
  planning record. Its ordinary execution grant and all original effect guards
  still apply; source fixtures do not qualify a real campaign.
- Live subscription inference through the canary on the operator runtime, with recorded evidence.
- Real callback ownership: proof that native events are bound to the owned thread and turn.
- Crash windows between intent, send and acknowledgement under qualification.
- Independent native checks beyond `file_sha256`.
- Verified cessation of the owner and every descendant on the operator runtime.
- After a guard refusal at `turn/start`, the persisted Dispatch attempt stays
  `turn_start_pending`. It is never replayed, and recovery treats it conservatively.
