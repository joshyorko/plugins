# Environment Artifact CI And Release Evidence

Use this for RCC v18.19.3 Artifact publication/acquisition or prewarm in CI. Read the RCC core artifact, provider/trust, and build-coordination references for command contracts.

## Pin And Isolate

- Pin the RCC version and release-asset SHA-256; never use `releases/latest` in a release gate.
- Isolate `ROBOCORP_HOME`, provider root, coordinator root, build staging, receipt paths, and test outputs outside source-controlled paths.
- Keep authorization in masked environment variables and trust roots in deployment-owned inputs. Do not persist provider authorization in provenance, SBOMs, logs, artifacts, or coordination events.
- On Linux staged prewarm, provision and test bubblewrap/prlimit/user-namespace prerequisites explicitly. Report non-Linux prewarm as unproved rather than inferred from lifecycle acceptance.
- Give coordinate `wait` an external timeout and cancellation path.

## Gates

Report each gate independently:

1. source/unit and race tests;
2. binary build plus exact SHA;
3. cold producer-to-consumer lifecycle in different private homes;
4. trust and canonical receipt validation;
5. provider-dead warm reuse without package-network access;
6. compatibility rejection before object fetch;
7. native execution and lease release on every supported platform;
8. released-binary coordinate/prewarm behavior, when used;
9. tag target, hosted release, asset digest, and downstream package handoff.

The v18.19.3 tag workflow at SHA `4148c2b...` passed build, release-candidate verification, Linux amd64, Windows amd64, macOS amd64, macOS arm64, and release publication. Platform jobs contain intentional OS-specific skips; macOS amd64 code-signing evidence is `unsigned-or-unverified`. Do not summarize those as all-platform code signing.

The tag's coordination black-box receipt covers internal scenarios and help output. It does not prove the complete released-binary claim/heartbeat/wait/release/prewarm JSON path, keyed verifier path, race detector, or four-platform prewarm. Keep those gates `unproved` unless the target workflow runs them directly.

The workflow's self-host compatibility pin is v18.18.1, not the immediately preceding v18.19.2. Describe it as two-generation/N-2 evidence, not v18.19.2 compatibility proof.

## Failure Handling

- Never promote a partial object set; only a complete verified committed Manifest is authoritative.
- Preserve the old leased generation if new prewarm is capacity-degraded.
- Treat coordinator notification/reason strings as advisory; verify Artifact closure and trust.
- On disk exhaustion, preserve the `ENOSPC` classification and retry only after bounded cleanup/recovery.
- Collect receipts and logs before cleanup, but redact credentials and keep source/build/runtime/release statuses distinct.
