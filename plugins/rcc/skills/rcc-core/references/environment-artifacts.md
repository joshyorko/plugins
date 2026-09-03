# Environment Artifacts v1

Use this reference for RCC v18.19.3 artifact identity, compatibility, publication, acquisition, offline transport, execution, local lifecycle, and warm reuse. Read `providers-and-trust.md` for provider and trust policy details, and `build-coordination.md` for optional cold-build coordination.

## Keep The Identities Separate

An Environment Artifact is an immutable Manifest v1 identified by canonical lowercase `sha256:<64 hex>`. Its identity includes the semantic specification, legacy blueprint, exact target platform, builder compatibility, one v12 gzip catalog, Object Index, and compatibility requirements. It excludes provider URLs, credentials, source/local paths, timestamps, signatures, attestations, materialization paths, leases, and RCC process IDs.

Do not conflate:

- the Environment Artifact digest;
- the semantic Specification digest;
- the legacy 16-character BlueprintHash and v12 catalog name;
- stored-object SHA-256 over compressed bytes;
- logical-object SHA-256 over decompressed bytes;
- provider reference or storage path;
- local materialization ID/path; or
- process-scoped execution lease.

Manifest, Specification, Descriptor, Object Index, and Platform Index JSON is canonical and strict. Duplicate keys, unknown fields, trailing JSON, wrong field order/whitespace, noncanonical digests, inconsistent totals, or incomplete descriptor closure fail closed.

## Compatibility

Environment Artifacts are same-platform portable, not cross-platform. v18.19.3 release acceptance covers:

| RCC platform | OS | Architecture |
| --- | --- | --- |
| `linux_amd64` | Linux | amd64 |
| `darwin_amd64` | macOS | amd64 |
| `darwin_arm64` | macOS | arm64 |
| `windows_amd64` | Windows | amd64 |

`RCCPlatform` must equal `<os>_<arch>`. RCC also checks catalog reader and encoding, relocation version, builder compatibility, Python implementation/version/ABI, native architecture/translation policy, CPU features, filesystem behavior, and system-requirement overrides. Linux additionally checks libc and required ELF libraries.

For Linux v18.19.3, `os.minimumVersion` is the stable family value `1` and `os.kernelMinimum` is `3.15`. A producer's newer `uname` is not copied into the requirement. Compatibility and archive semantic validation occur before provider object fetch or bulk local CAS import.

Use `RCC_HOLOTREE_MODE=private` when a worker already opted into machine-wide shared Holotree but this lifecycle must stay in its private `ROBOCORP_HOME`. Absence of the variable preserves the existing shared-Holotree default.

## Command Flow

Confirm exact help on the active binary. These are the v18.19.3 shapes:

```bash
rcc env publish --environment package.yaml --provider <reference> --json
rcc env publish --robot robot.yaml --provider <reference> --json

rcc env acquire --artifact sha256:<64-hex> --provider <reference> --json
rcc env acquire --archive environment.rcca \
  --trust-carrier ./detached-trust --trust-carrier-type filesystem \
  --trust-roots trust-roots.json --strict-remote --json

rcc env export --artifact sha256:<64-hex> \
  --provider <reference> --output environment.rcca

rcc env exec --artifact sha256:<64-hex> \
  --provider <reference> --json -- python -c 'print("ready")'

rcc env exec --artifact sha256:<64-hex> \
  --inherit-streams --receipt-file output/execution.json -- <command> [args...]

rcc env lifecycle inspect --artifact sha256:<64-hex> --json
rcc env lifecycle verify --artifact sha256:<64-hex> --json
rcc env lifecycle repair --artifact sha256:<64-hex> \
  --provider <reference> --json
```

`publish`, `acquire`, and `exec` require `--json`. `export` emits a JSON result but does not accept a `--json` flag. `publish` accepts exactly one source via `--environment` (`package.yaml` or `robot.yaml`) or the compatibility alias `--robot`.

Archive import does not automatically reuse its embedded attestations for the subsequent default strict trust decision, and the `.rcca` layout is not the `ArchiveCarrier` attachment layout. Supply a separate detached filesystem/HTTP carrier with deployment-owned roots, as above. For a deliberately unsigned local archive only, use explicit `--permissive-local` instead; never use that mode for remote or production input.

`env exec --json` keeps child streams out of its single machine result. A protocol or interactive child must use `--inherit-streams` with a caller-selected `--receipt-file`; RCC installs that receipt atomically only after child exit/reaping. Never parse child output as the RCC receipt.

## Publish And Acquire

Publication builds the RCC environment, canonicalizes the specification, inventories its exact legacy v12 catalog/Hololib closure, negotiates missing immutable objects, publishes trust attachments when configured, and commits the Manifest only after complete closure verification. Provider failure does not invalidate the local built environment; unreferenced uploaded objects remain non-authoritative and may be collected by provider policy.

Cold acquisition:

1. validates the digest, provider, Manifest, Specification, catalog, Object Index, platform, and worker compatibility;
2. fetches and digest-verifies immutable content into local CAS;
3. commits the local Manifest only after complete closure verification;
4. installs legacy Hololib content safely;
5. rebases/registers a consumer-local v12 catalog under the consumer `ROBOCORP_HOME`; and
6. records a ready materialization and durable references.

The consumer-side catalog rebase preserves canonical provider/catalog/object identity while allowing ordinary `--no-build ht vars` and `rcc run` after the producer home disappears.

A corrupt local Artifact fails closed rather than silently falling back. A local CAS `ENOSPC` remains classified as a local cache write failure and can be retried after space recovery without changing the remote Artifact.

## Offline Archives And Bundles

Canonical `.rcca` ZIPs live below `rcc-environment/` and contain the Manifest, Object Index, optional exact Platform Index, specification, legacy blueprint, catalog, objects, and optional detached attestations. Archive identity is independent of archive filename or container metadata.

RCC rejects absolute/traversal/backslash/duplicate member names, directories, symlinks, excessive member counts, oversized payloads, and compression bombs. v18.19.3 allows an individual immutable member up to the existing cumulative 256 MiB archive budget. Import verifies semantic identity, platform, compatibility, trust attachments, and complete digests before committing any Manifest.

Robot bundles can include an artifact archive and exact platform index. Preserve source-only bundles and legacy v12 workflows; do not imply one Artifact works across platforms or that a bundle contains RCC itself. Hardened `robot unpack --force` stages and replaces the complete destination rather than merging through existing paths. Project and archive symlinks are rejected, and `ignoreFiles` applies during bundle creation.

OCI, zstd, FUSE, packfiles, reflinks, and hardlinks are not shipped Environment Artifact v1 contracts in v18.19.3.

## Warm Reuse

A valid warm acquisition verifies the local canonical closure, ready record, materialization path/identity, Python runtime identity, and rebased catalog. It returns `cacheHit: "local-materialization"` without provider, builder, solver, or package-network calls. The provider may be dead and its authorization variable absent.

Provider-free warm reuse is valid only for a complete verified local Artifact. It is not permission to ignore corruption, compatibility, or trust policy.

## Leases, Recovery, And GC

`env acquire` does not return a transferable lease. `env exec` creates a process-scoped lease immediately before spawn, binds it to strong process-start identity, protects the Manifest/content/materialization closure, forwards signals, reaps the child, and releases the lease. Linux applies parent-death supervision; Windows uses Job Objects. Existing running leases retain their recorded trust decision when revocation state changes.

Use lifecycle commands before deleting anything:

- `inspect` distinguishes absent, incomplete, ready, corrupt, leased, and provider-required state.
- `verify` checks the ready record and no-follow materialization boundary.
- `repair` restores metadata locally when possible or reacquires from a trusted provider when required.

Reconciliation removes provisional state and provably stale leases, but preserves ambiguous process identity. GC protects active/ambiguous leases, durable references, pinned/legal/local-only policy, and shared content. Issue `joshyorko/rcc#122` remains open: do not claim every lease/crash/repair/GC acceptance item is complete merely because the released lifecycle commands exist.

## Verification Boundary

For release-quality evidence, require the exact built/released binary and separate results for:

- cold A-to-B provider acquisition in different private homes;
- materialization and representative native/Python execution;
- provider-dead warm reuse without package downloads;
- compatibility rejection before provider object fetch;
- lease creation and release;
- archive transport convergence; and
- Linux amd64, Windows amd64, macOS amd64, and macOS arm64.

Compile-only or one-platform unit evidence is not cross-platform runtime acceptance.
