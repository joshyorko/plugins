# Stage and run a local installation

Luna Factory has one package identity. A staged package contains the built Rust
runtime, built MCP Apps HTML, portable and compatibility manifests, and the one
canonical `skills/luna-factory/` directory. Direct skill invocation remains valid.
Staging does not establish an authenticated Codex run, ChatGPT rendering, tunnel
connectivity, or any other [live acceptance gate](package.md#evidence-required-before-release).

## Build

From the repository root, using Python 3.11+, Rust 1.88+ and Node.js/npm:

```bash
npm ci --prefix plugins/luna-factory/ui
npm run build --prefix plugins/luna-factory/ui
cargo build --locked --release --manifest-path plugins/luna-factory/server/Cargo.toml
```

Build on the machine or compatible platform where the runtime will execute. The
staging helper runs the supplied executable with `--version`, so provide only a
trusted local build. It does not build dependencies or download executables.

## Stage

Choose a new directory whose parent already exists, outside the source checkout
and target repositories. For example, after creating a private staging parent:

```bash
python3 plugins/luna-factory/scripts/package_runtime.py \
  --binary plugins/luna-factory/server/target/release/luna-factoryd \
  --ui plugins/luna-factory/ui/dist/index.html \
  --output /absolute/path/to/staging/luna-factory-0.2.0
```

The helper creates only the requested output directory and its contents. It
refuses existing destinations, source/output overlap, symlink paths, missing or
older-than-source builds, mismatched package/runtime/UI versions, a changed
canonical skill inventory, and external script/style/image assets in the HTML.
It copies an explicit file list. Source repositories, `node_modules`, Rust
`target` trees, Python caches, operator configuration and credentials are absent.
Unexpected files inside the canonical skill directory fail closed. Generated
Python `__pycache__` directories are ignored. Do not add credentials or private
registration identifiers to any selected package file.

The output layout is:

```text
luna-factory-0.2.0/
  plugin.json, mcp.json, plugin.yaml, __init__.py
  .codex-plugin/plugin.json, .claude-plugin/plugin.json, .mcp.json
  skills/luna-factory/                 one canonical skill and its references
  bin/luna-factoryd                    executable for the build machine
  ui/dist/index.html                  self-contained MCP Apps workbench
  assets/logo.svg, assets/logo.png    square workbench crescent branding
  README.md, docs/package.md, docs/local-service.md
  docs/repository-onboarding.md       bounded discovery and local approval
  runtime-receipt.json                 versions, input hashes, file hashes/modes
  SHA256SUMS                          every package file and the receipt
```

Verify the actual staged files before installation:

```bash
cd /absolute/path/to/staging/luna-factory-0.2.0
sha256sum --check SHA256SUMS
./bin/luna-factoryd --version
```

Identical selected files and build inputs produce identical receipt and checksum
bytes. The receipt includes source-input hashes and no absolute local paths,
clock timestamps, or operator data. Modification-time checks catch ordinary stale
builds; they are not cryptographic proof that a binary or HTML was built from the
recorded source. Clean builds and their build/test evidence remain necessary.
Keep sources, builds, and output parents trusted and unchanged during staging.
There is no archive, signature, download, release, or platform registration step.
If staging is interrupted, leave that partial destination alone and choose a new
empty destination. A completed package requires both receipt and valid checksums.

## Install and configure

Installation is a separate operator action. Move or copy the complete verified
package to a versioned private application directory outside target repositories,
then install that directory through the supported local host's plugin flow. Keep
all hidden compatibility files. Installing a manifest does not launch a service,
connect a ChatGPT account, or grant runtime execution authority.

Create the private operator JSON using the configuration in the
[plugin runbook](../README.md#configure-an-operator-instance). Set `skill_path` to
`/absolute/path/to/installed/luna-factory-0.2.0/skills/luna-factory/SKILL.md`.
Keep `listen` at `127.0.0.1:8787`, matching both shipped MCP maps. Keep the SQLite
file in a dedicated private state directory outside repositories and outside the
versioned package. Keep that same state path across upgrades. Use the existing
operator-owned Codex profile and credentials. Never copy credentials into the
package or a target repository.

Validate and run the installed binary explicitly:

```bash
/absolute/path/to/installed/luna-factory-0.2.0/bin/luna-factoryd doctor \
  --config /absolute/path/to/private/operator.json
/absolute/path/to/installed/luna-factory-0.2.0/bin/luna-factoryd serve \
  --config /absolute/path/to/private/operator.json \
  --ui /absolute/path/to/installed/luna-factory-0.2.0/ui/dist/index.html
```

`doctor` without `--probe-native` checks configuration and local identity. It does
not prove native execution. `serve` runs in the foreground and may reconcile
existing durable runs. Treat starting, stopping, and replacing a live service as
operator actions. Check active runs and retained claims before changing versions.
Unknown owned execution must remain blocked. Do not discard the database to
bypass a retained claim.

## Optional Linux user-service example

This is a template to review and adapt, not an installer. It runs the same binary
as the foreground command. Replace every `/absolute/path/` placeholder. The
working directory must already exist and stay outside target repositories.

```ini
[Unit]
Description=Luna Factory local MCP runtime
StartLimitIntervalSec=60
StartLimitBurst=3

[Service]
Type=simple
WorkingDirectory=/absolute/path/to/private/state
ExecStart=/absolute/path/to/installed/luna-factory-0.2.0/bin/luna-factoryd serve --config /absolute/path/to/private/operator.json --ui /absolute/path/to/installed/luna-factory-0.2.0/ui/dist/index.html
UMask=0077
Restart=on-failure
RestartSec=5
KillSignal=SIGINT
TimeoutStopSec=30

[Install]
WantedBy=default.target
```

Only the operator should place this in their user-service configuration and choose
whether to activate it. The helper never writes there and never calls `systemctl`,
reloads a service manager, or enables/starts a unit. It does not enable lingering.
The restart rate is bounded. Service-manager termination is not application-level
proof that native descendants stopped; retained claims still require normal
reconciliation. Do not use a service restart as a cancellation workaround.

This unit does not start or daemonize native Codex. If using the supported
`existing_daemon` transport, the operator must already have that independently
managed Codex daemon running and select it in private configuration. The default
`stdio` transport still owns its app-server process and retains its documented
recovery limits. Neither path copies credentials or changes authentication.

## Activate client connections

After explicit operator startup, a compatible local host connects to the same
loopback `http://127.0.0.1:8787/mcp` endpoint. The package does not add lifecycle
keys to plugin manifests. A private ChatGPT connection additionally needs the
operator's official Secure MCP Tunnel client and account-specific setup described
in [private connection guidance](package.md#private-chatgpt-connection).

Keep the daemon on loopback. The tunnel client is a separate process with separate
credentials and lifecycle; no tunnel, app ID, or authentication setup is performed
by staging or by the example unit. Test the installed package through the actual
intended host before claiming the installation/activation acceptance gate passed.
