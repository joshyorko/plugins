# RCC Dagger MCP Bridge

Use this when an agent needs access to a local Dagger module through MCP, especially Josh's RCC Dagger runner.

This bridge is optional. If Docker or Dagger is not available, use the regular `rcc` binary directly from the active project context instead of blocking on this MCP server. For RCC command failures, prefer `$rcc-core` and plain `rcc` CLI checks such as `rcc version`, `rcc diagnostics --quick`, and the exact failing `rcc ...` command.

There are two separate paths. Do not mix them up:

- Launcher script: this repo's `plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp`.
- Dagger module: the versioned module under `plugins/rcc/dagger`, or an explicit override supplied by `RCC_DAGGER_REPO`/`RCC_DAGGER_MODULE`.

The plugin bundles the RCC Dagger module so marketplace installs expose RCC methods from any working directory without a separate RCC checkout. Its default RCC binary is pinned to released v18.19.3 and verified against the published Linux SHA-256; callers can still supply an explicit version, whose integrity they must verify separately. `RCC_DAGGER_REPO` is only for testing or intentionally using a different checkout.

## Runtime Boundary

- Host: the MCP client starts the launcher script from this `plugins` checkout.
- Dagger: runs the plugin-bundled module by default.
- Optional override: `RCC_DAGGER_REPO=/path/to/module` or `RCC_DAGGER_MODULE=/path/to/module` selects a different module containing `dagger.json` and `.dagger/`.
- Function work: runs in Dagger containers defined by the selected checkout's `.dagger/` module.

Do not set an override for normal plugin use. The launcher deliberately does not fall back to Dagger's generic privileged `--no-mod` surface because that would make a healthy MCP connection appear to provide RCC methods when it does not.

The launcher filters Dagger's stdio stream for strict MCP clients. Dagger `mcp`
can emit engine progress lines on stdout before JSON-RPC responses even with
`--silent`; those lines are forwarded to stderr, while JSON-RPC lines remain on
stdout.

## Codex Registration

Normal plugin installation requires no custom MCP registration or RCC path environment variable. For a standalone launcher registration:

```bash
codex mcp add rcc-dagger \
  -- /var/home/kdlocpanda/second_brain/Areas/plugins/plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp
```

For development against a different RCC checkout, add the override:

```bash
codex mcp add rcc-dagger \
  --env RCC_DAGGER_REPO=/var/home/kdlocpanda/second_brain/Projects/automation-control-plane/rcc \
  -- /var/home/kdlocpanda/second_brain/Areas/plugins/plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp
```

For another machine, change the script path and any optional development override to local absolute paths.

Codex loads MCP server definitions when a session starts, so start a new Codex session after adding or changing this server.

## Common Failure

If Codex says:

```text
MCP client for `rcc-dagger` failed to start: MCP startup failed: No such file or directory (os error 2)
```

check the launcher path first. That error means Codex could not exec the configured command.

If the launcher starts but prints `Dagger module path must contain dagger.json and .dagger/`, the launcher path is fine but the override path points at the wrong repo.

If `ListMethods` exposes only Dagger core methods and omits `rcc`, `rcc-with-output`, and `run-robot-tests`, verify that the installed RCC plugin is version `0.1.2` or newer and restart Codex. Version `0.1.1` could silently start the generic no-module surface outside an RCC checkout. Plugin v0.2.0 updates the module's default binary to RCC v18.19.3.

If Docker is unavailable or the Dagger engine cannot start, stop using this bridge for RCC work. Fall back to the normal `rcc` binary in the active project or install/fix `rcc` through the `$rcc-core` path.

Verify registration:

```bash
codex mcp get rcc-dagger
```

Verify the bundled module directly from the plugin checkout:

```bash
dagger functions --mod plugins/rcc/dagger
```

## Dagger MCP Surface

Dagger's MCP server exposes a generic method interface:

- `ListMethods`
- `SelectMethods`
- `CallMethod`
- `ChainMethods`
- `ReadLogs`

Agents should call `ListMethods` first, then select and call the RCC module methods they need. In the current RCC Dagger module, the useful methods are:

| Method            | Use                                                                             |
| ----------------- | ------------------------------------------------------------------------------- |
| `rcc`             | Run an RCC command in the Dagger container and return stdout.                   |
| `rcc-with-output` | Run an RCC command and return a directory from the container.                   |
| `run-robot-tests` | Run the RCC Robot Framework acceptance suite through the Dagger test container. |

The Dagger docs note that externally exposed MCP currently supports modules with no required constructor arguments. The RCC module fits that shape.
