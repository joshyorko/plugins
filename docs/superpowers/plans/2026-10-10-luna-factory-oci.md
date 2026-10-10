# Luna Factory OCI Deployment Implementation Plan

> **For agentic workers:** This plan is executed by the native owner in this thread; no workers are dispatched.

**Goal:** Provide a rootless Podman deployment for the immutable Luna Factory 0.2.1 release, with persistent private state, loopback-only publishing, and a planning-only canary.

**Architecture:** Keep the published archive and its UI/skill immutable. Add an opt-in source listener mode that binds the container interface but accepts only the explicitly configured loopback published authority. Build a derivative OCI image from the exact release source plus that narrow patch, using digest-pinned Rust/runtime bases and the verified release UI/skill. Compose mounts operator config read-only and SQLite state separately; the default canary has no Codex binary, profiles, repositories, CAS targets, credentials, or tunnel.

**Tech Stack:** Luna Factory Rust MCP server, Python stdlib artifact/canary scripts, Podman rootless, Docker Compose specification, pinned OCI base images.

**Spec:** User request in the October 10 Dakota Luna Factory OCI deployment turn.

## Global Constraints

- Base source is `ebe2753ed032347456ef9b1c193646469bf17c96`; never modify the `v0.2.1` tag or published artifact.
- Keep the restored host service on `127.0.0.1:18787` and its Codex override unchanged.
- Never mount the original ledger, host Codex home/socket, host credentials, repositories, or protected tunnel configuration.
- Use a new empty private SQLite directory and a freshly verified free host port; publish only on `127.0.0.1`.
- Run the canary with zero inference, worker launch, CAS dispatch, and tunnel changes.
- CAS remains optional; container planning must work with no CAS targets.

## Review Focus

- A non-loopback container listener must reject any Host/Origin except the exact configured loopback publish origin.
- Release verification must fail closed on archive, provenance, source, binary, UI, or internal checksum mismatch.
- Container state must survive force-recreate with SQLite WAL enabled; config must be a separate read-only mount.
- Rootless UID mapping must keep bind-mounted config/state owner-only and writable only where required.
- The image must not gain host Codex credentials, Unix sockets, repository access, or network routes to protected services.

---

### Task 1: Gate container-published HTTP authority

**Files:**
- Modify: `plugins/luna-factory/server/src/config.rs`
- Modify: `plugins/luna-factory/server/src/http.rs`
- Test: `plugins/luna-factory/server/tests/http_security.rs`

**Interfaces:**
- Add optional `published_origin: Option<String>` to the strict operator config.
- Preserve loopback-only binding when absent. When present, require an unspecified container bind address and an explicit `http://` loopback authority with a port.
- Pass the validated origin to the HTTP guard; accept only its exact Host and optional exact Origin.

- [ ] Extend HTTP security tests first for a valid forwarded loopback authority and rejection of alternate host/port/origin.
- [ ] Run the focused test and confirm it fails because the current guard rejects the published authority.
- [ ] Implement config validation and request guard with unchanged defaults.
- [ ] Run focused Rust tests and invalid-config tests.

### Task 2: Build a verified, pinned OCI image

**Files:**
- Create: `plugins/luna-factory/container/Containerfile`
- Create: `plugins/luna-factory/container/build_image.py`
- Create: `plugins/luna-factory/tests/test_container_build.py`

**Interfaces:**
- Builder accepts the release archive, external provenance/checksum files, and output image tag.
- Builder verifies pinned source/artifact/binary/UI/skill hashes, safely extracts the release bundle, then invokes rootless Podman.
- Runtime image contains the derivative server binary, byte-identical released UI/skill, Python health probe, and no Codex executable.

- [ ] Add failing tests for exact pins, mismatched inputs, archive traversal/link members, and internal checksum failure.
- [ ] Run tests and confirm the expected verifier gaps.
- [ ] Implement minimal fail-closed verification and build context creation.
- [ ] Build with digest-pinned Rust 1.99 and Python 3.13 Trixie images.

### Task 3: Declare least-privilege persistent runtime

**Files:**
- Create: `plugins/luna-factory/container/compose.yaml`
- Create: `plugins/luna-factory/container/operator.example.json`
- Create: `plugins/luna-factory/container/healthcheck.py`

**Interfaces:**
- Bind-mount private config read-only and private state read-write at distinct paths.
- Use UID/GID 65532 mapped with rootless `keep-id`; drop capabilities, enable no-new-privileges, read-only rootfs, tmpfs `/tmp`, and bounded memory/CPU/PIDs/restarts.
- Publish one explicit configurable host port on `127.0.0.1`; use an isolated private network and MCP initialize health check.

- [ ] Validate Compose syntax and Podman provider support before canary startup.
- [ ] Confirm no host service, tunnel, Codex socket, credential, or repository mount appears in the resolved deployment.

### Task 4: Run isolated planning-only canary and document operations

**Files:**
- Create: `plugins/luna-factory/container/acceptance_canary.py`
- Create: `plugins/luna-factory/docs/container-deployment.md`

**Interfaces:**
- Canary performs MCP initialize/tools/resources/UI checks and a bounded settings write/read only; it never calls start/resume/cancel, native, or CAS execution tools.
- Documentation covers build, start, recreate, persistent backup, rollback, current host-service preservation, tunnel/UI gates, and Codex/CAS/Luna architecture.

- [ ] Add canary guards rejecting execution tool names and verify a unique disposable profile setting.
- [ ] Run the canary on a separately verified free loopback port and empty state directory.
- [ ] Force-recreate the container and prove the same setting remains, SQLite integrity passes, and inference/worker counters remain zero.
- [ ] Run focused tests and `bin/check`; inspect the branch diff and exact image/package digests.
