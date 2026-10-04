# Luna Factory

Luna Factory combines one canonical orchestration skill, a local Rust control plane, and an MCP Apps workbench. A native Codex Luna owner does the reasoning and delegates through native Codex workers. The service keeps durable run identity, repository claims, lifecycle state, limits, and bounded evidence.

Use `$luna-factory` directly in Codex when you want the skill alone. Starting a runtime-managed run requires an explicit request to use the Luna Factory runtime. Installing the plugin does not grant execution authority.

This is a local/private application package. Build and protocol tests do not establish that the live Codex, ChatGPT, or private tunnel acceptance gates have passed. See [package and operator boundaries](docs/package.md) for the gate checklist.

## Build from this checkout

Requirements: Rust 1.88 or newer, Node.js/npm for the UI, Git, and a supported authenticated Codex CLI. Use the operator's existing Codex profile; do not copy credentials into this checkout.

From the repository root:

```bash
npm ci --prefix plugins/luna-factory/ui
npm run build --prefix plugins/luna-factory/ui
cargo build --locked --manifest-path plugins/luna-factory/server/Cargo.toml
```

The executable is `plugins/luna-factory/server/target/debug/luna-factoryd`; the UI build produces `plugins/luna-factory/ui/dist/index.html`. A release build can use `--release` and the corresponding `target/release/` path. Use the checked-in UI and Rust lockfiles for repeatable dependency resolution.

## Stage one installable package

Use the [local package and service guide](docs/local-service.md) to stage the built
runtime, self-contained UI, canonical skill and manifests together. The staging
helper verifies versions, input freshness and checksums. It does not install or
activate a service, register an app, or copy operator configuration/credentials.

```bash
python3 plugins/luna-factory/scripts/package_runtime.py \
  --binary plugins/luna-factory/server/target/release/luna-factoryd \
  --ui plugins/luna-factory/ui/dist/index.html \
  --output /absolute/path/to/new/luna-factory-0.2.0
```

## Configure an operator instance

Create a private JSON file outside every target repository. Replace the example absolute paths with real paths on the operator machine:

```json
{
  "listen": "127.0.0.1:8787",
  "database": "/home/operator/.local/state/luna-factory/runs.sqlite",
  "codex_binary": "/absolute/path/to/codex",
  "skill_path": "/absolute/path/to/plugins/luna-factory/skills/luna-factory/SKILL.md",
  "repositories": {
    "my-project": {
      "root": "/absolute/path/to/my-project",
      "max_finish": "local_candidate"
    }
  },
  "profiles": {
    "default": {
      "effort": "low"
    }
  },
  "limits": {
    "capacity": 2,
    "repair_attempts": 2,
    "wall_seconds": 1800
  }
}
```

The `default` alias inherits the existing Codex configuration and requests the listed effort. Additional aliases can choose a different allowed effort while keeping that inherited configuration. Codex 0.159.2 does not provide a verified profile-switching path here: omit the optional `codex_profile` field or set it to `null`; non-null values must be rejected rather than guessed. Remote callers can select only approved aliases. Do not put provider URLs, tokens, passwords, or arbitrary executable arguments in remote requests or plugin settings.

Keep `max_finish` at `local_candidate` unless the operator intends to allow `push` or `pr`. A run's explicit finish request still cannot exceed that cap. The service does not grant merge, release, or deploy authority. The SQLite path must stay outside the allowlisted repositories and should remain stable across upgrades.

Check the local configuration, then serve it:

```bash
plugins/luna-factory/server/target/debug/luna-factoryd doctor \
  --config /absolute/path/to/operator.json
plugins/luna-factory/server/target/debug/luna-factoryd serve \
  --config /absolute/path/to/operator.json
```

The daemon serves the built `ui/dist/index.html` from the plugin root by default. To select an installed build explicitly, pass `--ui /absolute/path/to/ui/dist/index.html` to `serve`. This local CLI option is not a remote tool argument.

The checked-in MCP maps connect to `http://127.0.0.1:8787/mcp`. Keep the listener on loopback. The daemon and the installed plugin run in the same local environment; installing a manifest does not build or launch the service.

## Open the workbench

Install this repository's `luna-factory` package in a compatible local host, then connect its MCP endpoint. For private ChatGPT access, run the official Secure MCP Tunnel client beside the daemon and create the private developer-mode connection. Follow the [connection guidance](docs/package.md#private-chatgpt-connection); no account-specific registration or tunnel identity is shipped here.

The workbench uses the same service and run history as the MCP tools. Start from an approved repository alias, explicit acceptance, finish authority, and bounded limits. Inspect status and evidence without model calls. Steer, stop, and resume act on the existing run. A request to interrupt is not proof that owned execution has stopped; retained claims and blockers must remain visible until reconciliation proves it.

## Verify a change

From the repository root:

```bash
python3 -m pip install -r requirements-dev.txt
bin/check
cargo test --locked --manifest-path plugins/luna-factory/server/Cargo.toml
cargo clippy --locked --manifest-path plugins/luna-factory/server/Cargo.toml -- -D warnings
cargo fmt --manifest-path plugins/luna-factory/server/Cargo.toml -- --check
npm test --prefix plugins/luna-factory/ui
npm run build --prefix plugins/luna-factory/ui
```

Also run the [live acceptance checklist](docs/package.md#evidence-required-before-release). Record blocked and unproved checks explicitly. Protocol fixtures and local UI tests cannot prove an authenticated owner/worker run, descendant cancellation, ChatGPT entrypoint rendering, or tunnel reconnect behavior.

The [canonical skill](skills/luna-factory/SKILL.md), [routing policy](skills/luna-factory/references/routing-and-evidence.md), and [skill evals](skills/luna-factory/references/evals.md) remain the semantic reference. The service enforces deterministic boundaries around that policy.

## Reconnect and cancellation limits

For an already running operator-owned Codex daemon, set `native_transport` to
`existing_daemon`. Optionally set `native_socket` to its existing absolute control
socket path. The runtime uses `codex app-server proxy`; it does not create,
restart or configure that daemon. This lets the native execution lifetime remain
independent of Luna Factory's HTTP service. The default `stdio` transport owns its
app-server process and fails closed after a restart when its old process lifetime
cannot be established.

State storage must be in a dedicated private directory outside repositories.
One service holds an exclusive file lease. Recovery restores deadlines and
performs read-only native reconciliation, without starting a turn. Resume rejects
revoked/remapped repositories, changed profiles/skill content and reduced limits.

A successful interrupt and idle owner/worker threads do not prove a background
terminal exited. The service retains bounded command identities and requires
terminal command evidence before releasing a claim. Unknown processes keep the
run blocked. The installed native protocol does not yet have a proven direct
termination path for agent-owned background terminals. Do not substitute
`command/exec/terminate`: its IDs belong to separate connection-scoped commands.
There is no force-unlock endpoint. This boundary still requires live operator
proof, even when synthetic lifecycle tests pass.

Source subjects hash the Git HEAD, staged index inventory and raw tracked and
untracked file bytes. Repository diff/textconv/clean helpers are not executed.
Subjects reject submodules pending explicit support, more than 25,000 files and
more than 512 MiB of candidate bytes. Those are fail-closed capability limits.
Luna defaults are pinned in native owner/child configuration. The native protocol
has no proved pre-spawn model allowlist; canonical policy remains responsible for
explicit child choices and actual routing is unverified unless native telemetry
establishes it. This is still a draft product, not a completed live acceptance.
