# Environment Artifact Providers And Trust

Use this reference for RCC v18.19.3 named providers, the loopback cache server, enterprise HTTP policy, detached artifact trust, and the boundary with legacy `rccremote`.

## Provider Profiles

The built-in `local` filesystem provider lives below `$ROBOCORP_HOME/artifacts/v1/provider`. It is separate from the consumer content cache and materializations.

Named profiles support only type `http` in v18.19.3:

```bash
rcc provider add office --type http \
  --url https://cache.example/ \
  --authorization-env RCC_PROVIDER_OFFICE_AUTHORIZATION --json
rcc provider list --json
rcc provider inspect office --json
rcc provider test office --json
rcc provider remove office --json
```

Names are lowercase and match `[a-z0-9][a-z0-9._-]{0,62}`; `local` is reserved. Re-adding an identical profile is idempotent. Changing one requires `--replace`. Add, replace, and remove mutate `$ROBOCORP_HOME/settings.yaml` under a lock while preserving unrelated settings, so inspect the active home/profile first.

Profiles store the authorization environment-variable name, never its value. The value supplied at runtime must be the complete `Authorization` header. Keep it out of arguments, committed files, receipts, logs, provenance, and SBOMs. Missing credentials fail before a request; inspect output reports only whether the variable is present.

Provider references can also be direct HTTP(S) URLs. Prefer named profiles for maintained environments so the credential reference and endpoint policy remain explicit.

## HTTP Policy

Provider base URLs must be exact roots. RCC rejects userinfo, query, fragment, non-root paths, unsupported schemes, and non-loopback plain HTTP. HTTPS is required for remote hosts. Redirects are not followed, and credentials are not forwarded across origins.

Providers inherit RCC's configured proxy, `network.no-proxy`, custom CA, TLS minimum, timeout, and user-agent settings. `no-proxy` supports exact hosts, domains, ports, `*`, and CIDR matching. Diagnose the configured transport and provider separately; never disable TLS verification or embed credentials to get a probe passing.

The v1 HTTP contract is immutable and digest-addressed. It exposes health/capabilities/protocol, object negotiation and transfer, Manifest commit/read, detached trust attachments, and bounded administration. Object range/resume is not shipped: Range receives 416 and clients restart the full object. Provider authentication is separate from Artifact trust.

## Loopback Cache Server

```bash
rcc cache serve --root ./provider \
  --listen 127.0.0.1:0 --backend filesystem --json

rcc cache serve --root ./provider-journal \
  --listen 127.0.0.1:0 --backend journal \
  --max-bytes <bytes> --max-objects <count> \
  --max-manifests <count> --max-uploads <count> \
  --requests-per-second <count> --json
```

`--json` is mandatory. Startup emits `url`, `root`, and `listen`. Both backends expose provider health, capabilities, protocol, quotas, cleanup, audit, GC, repair, backup, and restore. Filesystem storage is strict CAS; the journal backend persists append-only records plus sidecar objects and recovers committed transactions after restart.

The server has no authorization option and rejects non-loopback listeners. Its security boundary is loopback. Do not advertise it as an authenticated remote service or defeat this boundary; external exposure requires a separately designed and authorized reverse-proxy/service boundary.

Zero limits mean unlimited. Quota exhaustion is explicit (HTTP 507) and rate limiting uses HTTP 429. Do not assume advertised protocol v2 means ranged transfer: v18.19.3 reports v2-compatible-v1 while retaining full-restart-only objects.

## Trust Model

Artifact integrity answers which immutable bytes exist. Trust answers who built them, from which declared inputs and builder policy, and whether this worker may execute them.

Trust attachments are detached from Manifest identity:

- canonical provenance binds source/resolution/builder facts;
- deterministic SBOM binds sorted components to the Artifact digest;
- Ed25519 signature envelopes bind the canonical Artifact digest;
- revocation snapshots identify revoked Artifact or signer IDs; and
- verification receipts retain policy/decision/signer/attachment/revocation facts.

Strict remote trust is the default and fails closed on missing, malformed, future, or stale revocation state. `--permissive-local` is an explicit local-only policy for unsigned local artifacts. Never use permissive mode to make a remote or production Artifact pass.

```bash
rcc env trust verify \
  --artifact sha256:<64-hex> \
  --platform linux_amd64 \
  --builder <builder-id> \
  --provenance provenance.json \
  --sbom sbom.json \
  --signatures signatures.json \
  --trust-roots trust-roots.json \
  --revocations revocations.json \
  --verification-time <RFC3339> \
  --strict-remote --json
```

Deployment-owned Ed25519 public keys come from `--trust-roots`; provider authentication/signatures are not trust roots. Carrier selection on publish/acquire/exec supports filesystem, archive, and HTTP forms. Publication stages required trust material before committing a visible provider Manifest. New leases re-evaluate the configured carrier and record their decision; existing running leases are not silently killed by a later revocation.

### v18.19.3 standalone verifier caveat

The standalone `env trust verify` command does not canonical-parse `--artifact` before permissive-local evaluation. The released binary can emit `valid:true` for malformed text such as `sha256:ABC`. Validate lowercase `sha256:<64 hex>` before invoking it and do not treat such a receipt as Artifact proof. `env acquire`, `env export`, `env exec`, and lifecycle commands do canonical-parse their artifact identities. Track this as an upstream correctness gap; do not normalize malformed input silently.

## Legacy `rccremote`

`rccremote` remains a separate released binary and compatibility-level-A protocol using `/parts`, `/delta`, and `/force`, shared Holotree, `RCC_REMOTE_ORIGIN`, and `RCC_REMOTE_AUTHORIZATION`. It has no Manifest v1 provider profiles, journal backend, quotas, trust attachments, or v1 health/capability contract.

Do not call `rcc cache serve` a replacement or alias for `rccremote`. Preserve provider-free local v12 workflows and debug the protocol actually in use.
