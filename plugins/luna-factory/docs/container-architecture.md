# Luna Factory, Codex, and CAS have different jobs

This document explains the OCI boundary and the future runner decision. It does not authorize a new login, credential grant, or remote execution path.

## Keep the three components separate

| Component | Owns | Does not own |
| --- | --- | --- |
| Luna Factory | MCP tools, workbench resources, run and planning state, source bindings, and lifecycle policy | Codex account login or a separate task server |
| Codex CLI and app-server | Model sessions, native threads, local workspace access, and approval events | Luna Factory's durable ledger or workbench |
| Codex Action Server, or CAS | A separate MCP wrapper for Actions workflows and remote or multi-target fanout | Luna Factory's planning state or its local native Codex connection |

Luna Factory is useful for local planning without CAS. Keep CAS optional for remote worker fanout. Do not route local Luna execution through CAS by default.

Codex CLI is the local program and configuration owner. Codex app-server is its control protocol. Luna Factory 0.2.1 supports two app-server transports:

- `stdio` starts an app-server child that Luna Factory owns.
- `existing_daemon` connects to an existing Unix socket using WebSocket framing. It does not stop or own the daemon.

See `server/src/native.rs` and [Native transport verification](native-transport-verification.md). The CLI reports `codex app-server` as experimental in the installed 0.162.1 help.

## Keep the default container planning-only

The OCI default contains Luna Factory, the released workbench and skill, and a health probe. It contains no Codex executable or credential. It mounts no host Codex home, host app-server socket, or repository. The operator config has empty repository and CAS maps.

This is the right default for Dakota. It starts the service with fresh state and lets a local MCP client verify discovery and persisted planning settings. It does not claim that Codex execution works inside the container.

## A pinned Codex runner could remove the host dependency

The installed Codex CLI 0.162.1 exposes `codex login --device-auth`. A pinned CLI can run its own app-server inside a container, so Luna Factory would not need the host Codex binary or host daemon socket. This does not remove the need for an account login or a user-approved device authorization.

There are two possible runner shapes.

| Option | Benefit | Cost |
| --- | --- | --- |
| Put the pinned CLI beside `luna-factoryd` and use owned `stdio` | Fewest services. Luna Factory already owns and supervises this app-server path. | One image release couples the CLI and Factory. The container needs its own authorized `CODEX_HOME`, and the combined process tree needs stop and descendant checks. |
| Run a pinned Codex runner sidecar and share one private Unix socket | Separate CLI updates and credentials from the Factory service. Luna Factory already supports explicit `native_socket` connections. The host Codex daemon is not required when the sidecar owns its app-server. | Adds socket permissions, sidecar health and shutdown coordination, plus an account-auth volume. The workspace must be mounted into both services at the same path. |

I recommend the sidecar only as a future opt-in `runner` profile. It keeps Codex auth and updates out of the default Luna image. The profile must pin the Codex CLI version and image digest, create a new private auth volume, and use a per-run worktree mount. Do not mount the host `.codex` directory or copy host tokens.

## Decisions required before enabling a runner

**Account authorization.** The installed CLI exposes device authorization, but this setup did not run it. An operator must approve the ChatGPT account login and complete the browser or device step inside the dedicated runner context. The runner must persist that context in a private volume with mode 0700. Do not put credentials in image layers, Compose files, repository files, or logs.

**Licensing and distribution.** The OpenAI Codex repository reports Apache-2.0. A distributed CLI image still needs the matching license and notice files, plus a review of the exact binary's distribution and account terms. Keep the image private until that review is complete.

**Approvals and callbacks.** Luna Factory's native transport treats approval callbacks as events and does not answer them automatically. The runner profile needs an operator-facing approval path. Do not enable unattended approvals to make a canary pass.

**Workspaces.** Mount only a dedicated worktree for each admitted run. Use the same absolute path in the Factory and runner containers. Do not mount all of `/home`, the plugins checkout, or the preserved production ledger.

**Rate limits.** Codex work remains subject to the authorized ChatGPT account's model access and rate limits. The runner must report denials and limits as blockers. Do not substitute an API key, private Cloud endpoint, or Codex Tasks API.

**Version compatibility.** Pin the Codex CLI version, source, and image digest. Generate or inspect app-server schemas from that exact installed version. The host's current CLI is 0.162.1, but that alone does not qualify a future container image or its callbacks.

**Termination.** The owned `stdio` path can stop its child when the connection closes. A sidecar introduces a second process owner. Before releasing any repository claim, prove that the owner, app-server, and all worker descendants stopped. Keep unknown activity blocked.

## Do not treat app-server as a remote task API

The ordinary app-server is the local Codex control protocol. Its presence does not prove support for private Cloud execution, hosted Codex Tasks, or remote workspace APIs. Luna Factory does not depend on those APIs, and this OCI profile does not call them.

## References

- [Codex CLI](https://developers.openai.com/codex/cli)
- [Codex app-server](https://developers.openai.com/codex/app-server)
- [OpenAI Codex source and license](https://github.com/openai/codex)
- `server/src/native.rs`
- `docs/native-transport-verification.md`
- `docs/local-service.md`
