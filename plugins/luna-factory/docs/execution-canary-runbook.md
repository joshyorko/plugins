# Operator execution qualification and disposable canary

This is a prepared procedure, not a record of a live run. Cloud fixtures do not
qualify Dakota. Source changes, CI, deployment and native execution need separate
receipts. The deployed planning graph, active config and private ledger remain
untouched by this follow-up.

## Required operator authority

Before any live step, the operator must explicitly approve the exact runtime
artifact, config and state paths, existing daemon socket, disposable workspaces,
capacity 1, one canary inference turn, its 20-second work / 30-second total budget,
and targeted containment of only canary-owned threads/descendants/terminals.
Changing process placement or container PID visibility, rollout, and a subsequent
ordinary managed canary are separate decisions. Approval for source work or a
healthy planning container is insufficient.

Do not copy credentials, mount Codex home, adopt an existing worker, launch another
app-server, modify a real qualification database by SQL, bypass the interactive
operator CLI, merge or deploy from this procedure without that authority.
Keep receipts private. Publish only reviewed hashes and redacted outcomes.

## Resolve identity and namespace first

Codex 0.165.0 initialize fields `userAgent`, `codexHome`, `platformFamily` and
`platformOs` do not attest a daemon. The new Linux binding uses SO_PEERCRED and
SO_PEERPIDFD on the connected Unix socket, requires the same effective UID, checks
pidfd/procfs correspondence and the same PID namespace, and hashes the peer's
executable against the configured Codex binary. Linux 6.5 or newer and readable
procfs are required. It records mechanism `linux-unix-peer-executable`, version
`1`, and the executable SHA-256. It does not infer a Codex version or launch
arguments from that hash. Pre-write checks refresh this binding.

An OCI container in a private PID namespace cannot attest a host daemon this way.
An explicit path mapping alone does not fix that. Stop at
`qualification_provider_mismatch` if the host PID is invisible. An operator must
review and authorize a same-namespace process placement, such as a separate
host-side Factory process with its own private state and unused loopback port, or
a separately reviewed container isolation change. This PR does not change the
existing Compose PID namespace or provide a host attestation proxy. Do not add
host PID access or a host procfs mount just to force qualification to pass.

Use the actual active service's `--config` argument as the configuration source.
Preserve its repositories, profiles, planning graph and transport settings. Never
replace it with the empty base operator.json. Back up SQLite using the online
backup API and retain the matching prior image and config before an approved
rollout. An old runtime cannot safely read promoted execution records; do not
restore an old database over newer planning or execution data.

## Declare and compare paths without inference

`native_paths` maps canonical Factory roots to absolute daemon roots. Identity
maps must be explicit too. Roots cannot overlap or traverse symlinks. Map only
the specific repository/worktree, required Git common directory, canonical skill,
Factory subject CLI binary and disposable canary. Keep state outside every
repository and writable workspace. Bind only required paths, never all of home.

Example names are placeholders, not Dakota's config:

```json
{
  "native_paths": {
    "/shared/worktree": "/host/private/luna-canary/worktree",
    "/shared/git-common": "/host/private/luna-canary/git-common",
    "/opt/luna-factory/skills/luna-factory": "/host/private/luna-tools/luna-factory",
    "/usr/local/bin/luna-factoryd": "/host/private/luna-tools/luna-factoryd",
    "/shared/canary": "/host/private/luna-canary/bootstrap"
  }
}
```

The Codex executable can be mapped separately for the comparison probe. The
kernel binding checks its actual bytes, not a mapping declaration. A changed map
changes the exact-profile fingerprint and requires a new qualification.

The following source helper makes no native connection, grant or inference. Run
`prepare` in the intended Factory namespace and `verify` on the actual daemon
host. Use the exact reviewed helper on both sides. Its request includes private
paths and byte hashes, never credentials. Output files are mode 0600 and cannot
be overwritten. Use new output names for retries.

```sh
python3 container/execution_paths.py --output "$PRIVATE/path-request.json" prepare \
  --config "$CONFIG" --factory-binary "$FACTORY_BINARY" --workspace "$BOOTSTRAP_REPO"
# Transfer only this private request through an approved local path, then on the daemon host:
python3 container/execution_paths.py --output "$PRIVATE/host-path-receipt.json" verify \
  --request "$PRIVATE/path-request.json"
```

The receipt compares repository HEAD/index/worktree bytes, required Git common
directory, canonical skill, Codex and Factory binary content. A successful
receipt covers the invoking process namespace only. The operator must establish
that `verify` ran where the existing daemon resolves paths; a cloud or image-local
receipt cannot establish host visibility. No receipt is an execution grant.

## Bootstrap qualification and fixed harness

1. Create a new mode-0700 disposable Git repository with an empty initial commit,
   no remote, no untracked files and no shared Git identity. It must be outside
   all approved repositories, discovery roots, working roots and the state DB.
   Do not reuse the saved planning repository. Map and probe it first.
2. Inspect the exact existing daemon's supported schema and authentication with
   a passive `account/read` request using `refreshToken:false`. Retain sanitized
   evidence of ChatGPT account/subscription authentication and exact Luna model
   access. A catalog entry alone does not prove entitlement. API-key/Bedrock or
   unknown authentication is a stop condition. Do not login, refresh tokens,
   switch providers or introduce an API-billed fallback. Protocol reference:
   [Codex app-server](https://learn.chatgpt.com/docs/app-server).
3. In an approved operator TTY outside any Codex/sandbox session, run
   `qualification-profile --config "$CONFIG" --profile "$PROFILE"` on the exact
   candidate binary. Check the resolved executable hash, provider binding,
   transport, mapping, effort and fingerprint against the reviewed artifact.
   This is a passive connection, not inference or entitlement proof.
4. Bind the private operator bootstrap evidence JSON to `profile_fingerprint`.
   Record only `qualification_only`, capacity 1, short expiry through the normal
   CLI confirmation. This does not authorize ordinary runs:

   ```sh
   "$FACTORY_BINARY" record-qualification --config "$CONFIG" --profile "$PROFILE" \
     --evidence "$PRIVATE/bootstrap-evidence.json" --scope qualification_only \
     --max-capacity 1 --expires-in-days 1
   ```

5. With the separate explicit one-turn authority, invoke the existing harness:

   ```sh
   "$FACTORY_BINARY" qualification-canary --config "$CONFIG" --profile "$PROFILE" \
     --workspace "$BOOTSTRAP_REPO" --output "$PRIVATE/native-canary.json"
   ```

It creates only `luna-factory-qualification-canary.txt` containing exactly
`Luna Factory qualification canary v1` and a newline. It forbids networking and
subagents, independently checks file content/hash and extra worktree changes,
restores its fixed file, and observes the owned native threads and terminals.
It admits no ordinary run or claim and never promotes a stored plan.

Failure, timeout, denied approval, entitlement uncertainty, unexpected worktree
change or lost ownership is failed/uncertain evidence. Preserve it and stop. The
harness can interrupt only its known owned turns and targeted terminals. Never
retry an uncertain native mutation as a new job. Use read-only reconciliation and
the operator's native UI. Never claim an independently verified OS process tree:
cessation remains `native_owner_descendants_and_reported_terminals`, with
`process_tree: "unverified"`. Detached/unreported subprocesses remain unproved.

## Separate ordinary managed canary

A passed fixed harness alone does not complete #71. Review the sanitized account,
subscription, path, routing, adapter start/observe/steer/stop/reconcile and callback
ownership evidence, including intent/send/ack crash-window behavior. Only then
may an explicitly authorized operator record an `ordinary` grant from the genuine
harness output, with capacity 1 and short expiry. The CLI requires the actual
harness result and preserves the supplied evidence digest. Never substitute a
fixture or manually change an outcome to passed. Revoke the bootstrap grant on
failure or when the procedure stops without ordinary approval.

For a separately approved runtime-managed test, use a second disposable approved
repository and a new campaign. Preserve the existing saved plan and graph. Read
the current graph and review every candidate, dependency, target, canonical title
and unchanged acceptance contract. Promote only that exact revision, using a new
key and explicit budget `{capacity:1, repair_attempts:0, wall_seconds:60}`. Without
an explicit budget, the fixed planning budget is capacity 1, zero repairs and 30
seconds. Native-local is the only supported execution preference.

Retain the new executable run ID, `approved_plan` lineage/digest, exact source
subject, owned thread/turn IDs, dispatch intents and acknowledgements, callback
correlation, claim acquire/retain/release, independently checked file predicate,
cessation result and ordinary grant revocation. A duplicate promotion must return
the recorded run with no extra thread/turn, including after restart. Stale or
changed-source approvals must dispatch nothing. Unknown outcomes retain the claim.
Do not manufacture a crash or revoke unrelated profiles on a live service; use a
separately authorized isolated exercise for destructive crash-window checks.

The finish receipt must list source SHA, CI head/checks, deployed SHA and image
digest, exact profile/provider identity, authentication/entitlement evidence,
actual job outcome, claim and cessation state, unresolved process-tree/callback
limits and any remaining operator decision. A tools list or healthy container is
connection/status proof only.
