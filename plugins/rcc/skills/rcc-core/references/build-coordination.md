# Environment Build Coordination And Prewarm

Use this reference for the optional v18.19.3 filesystem coordinator and prewarm CLI. Treat shipped source/help/runtime output as authoritative where `docs/environment-build-coordination.md` describes a broader aspirational machine contract.

## Boundary

Coordination prevents avoidable duplicate cold builds and controls staging capacity. It is not authorization to trust bytes, a distributed Action Run/Attempt fence, or a requirement for local RCC. A complete verified committed Artifact remains authoritative; claims, notifications, staging output, and reason strings do not.

The canonical build key binds specification digest, exact target platform, builder compatibility, resolution policy, trust/build policy, and Artifact schema/encoding features. Provider URLs, queue priority, Holotree space names, and downstream business objects do not belong in it.

## Shipped CLI

```text
rcc env coordinate claim
rcc env coordinate heartbeat
rcc env coordinate wait
rcc env coordinate release
rcc env coordinate prewarm
```

Every operation requires `--json`, a coordinator `--root`, key inputs, owner/TTL state where applicable, and keyed Ed25519 trust inputs. Inspect exact subcommand help on the active binary; do not copy the aspirational documentation's sample JSON as a schema.

Key operational facts in v18.19.3:

- Claim output uses `expiresAt`; it does not emit the documented `heartbeat`/`expiry` pair.
- Observed claim outcomes are `claimed`, `existing-artifact`, and `waiting`; do not invent documented outcomes that the CLI does not emit.
- CLI output currently has no top-level `schemaVersion`.
- Prewarm item keys are capitalized `Key` and `Status` because the shipped fields lack JSON tags.
- `wait` has no CLI timeout. Wrap it in an external bounded timeout/cancellation policy.
- `--capacity 0` becomes the number of requested keys; it does not disable builds.
- Retry backoff starts at 25 ms, is context-cancellable, and caps at one second.
- Concrete staged prewarm execution is Linux-only and requires bubblewrap; non-Linux execution fails closed.
- `--independent-build` is present but fails the released staging-boundary check in the installed v18.19.3 binary. Do not rely on it as a fallback without fresh proof.

Do not put credentials or production Action secrets in build inputs. Although `--provider-authorization` exists, v18.19.3 can persist the completed Artifact record in events/nondeterminism state. Do not pass a real credential through this flag; use a non-secret proof or keep the workflow local until the persistence boundary is fixed and verified.

## State And Failure Model

The coordinator uses owner plus monotonic epoch, heartbeat/expiry, and filesystem locking. Build in worker-local reserved staging, verify complete closure and policy, publish immutable objects, atomically commit the Manifest, then release/complete the matching epoch.

Required operator behavior:

- Owner death permits takeover only after expiry/epoch change.
- A stale epoch cannot replace a committed Manifest.
- Partial objects without a Manifest remain non-authoritative.
- Manifest commit before acknowledgment remains authoritative and completion is retryable.
- Distinct valid outputs for one build key are nondeterminism, not an equivalent cache hit.
- Coordinator/provider loss falls back only to verified local content, another trusted provider, or an explicitly permitted local build.
- Rolling prewarm retains old leased generations when the new generation cannot fit; it must not delete usable state to report success.

Staging disk reservation is per build, not a shared total-disk budget. Bound concurrency and capacity externally. Quarantined staging and disk-pressure behavior require inspection; do not assume quarantine releases every reservation automatically.

## Verification And CI

Separate these gates:

1. source/unit behavior for keys, fencing, recovery, nondeterminism, capacity, and cancellation;
2. released-binary JSON for claim/heartbeat/wait/release/prewarm;
3. real staged builder execution and cryptographic closure proof;
4. race-detector evidence; and
5. platform runtime evidence.

The v18.19.3 tag workflow passed its coordination black-box gate, but that gate uses internal test seams and help output. It does not prove the complete released-binary coordination CLI, keyed verifier path, race detector, or four-platform prewarm. The native four-platform receipts prove Environment Artifact lifecycle behavior, not coordinate/prewarm CLI parity.

For CI, pin the RCC version and asset SHA, isolate coordinator/build roots outside the checkout, provision Linux bubblewrap/prlimit where staged execution is expected, use external timeouts, and bind promotion receipts to the exact source commit and binary SHA. Report platform skips and unproved gates explicitly.
