# RCC Dagger MCP Bridge

Use this when an agent needs access to a local Dagger module through MCP, especially Josh's RCC Dagger runner.

This bridge is optional. If Docker or Dagger is not available, use the regular `rcc` binary directly from the active project context instead of blocking on this MCP server. For RCC command failures, prefer `$rcc-core` and plain `rcc` CLI checks such as `rcc version`, `rcc diagnostics --quick`, and the exact failing `rcc ...` command.

There are two separate paths. Do not mix them up:

- Launcher script: this repo's `plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp`.
- Dagger module: current working directory when it contains `dagger.json` and `.dagger/`, an override supplied by `RCC_DAGGER_REPO`, or no module when neither exists.

The bridge does not embed RCC source code or pin a repo by default. It starts Dagger from the agent host and points Dagger at the current Dagger module when one is present. `RCC_DAGGER_REPO` is only for a fixed checkout override.

## Runtime Boundary

- Host: the MCP client starts the launcher script from this `plugins` checkout.
- Dagger: runs the module from current working directory when it contains `dagger.json` and `.dagger/`.
- No module: if current directory is not a Dagger module and no override is set, the launcher starts `dagger mcp --no-mod --env-privileged` so Dagger can expose core API tools without a repo module.
- Optional override: `RCC_DAGGER_REPO=/path/to/module` pins the module path when current-directory behavior is not wanted.
- Function work: runs in Dagger containers defined by the selected checkout's `.dagger/` module.

Do not force a repo path unless the user asks for a fixed module. If the user wants “whatever directory I am in,” register only the launcher command and let cwd select the module. If there is no module in cwd, let Dagger start without one.

No-module mode is privileged by Dagger design. Treat it as host/Docker-capable, not a harmless read-only helper.

The launcher filters Dagger's stdio stream for strict MCP clients. Dagger `mcp`
can emit engine progress lines on stdout before JSON-RPC responses even with
`--silent`; those lines are forwarded to stderr, while JSON-RPC lines remain on
stdout.

## Codex Registration

On Josh's Bluefin host, prefer current-directory registration:

```bash
codex mcp add rcc-dagger \
  -- /var/home/kdlocpanda/second_brain/Areas/plugins/plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp
```

For a fixed RCC-only registration, add the override:

```bash
codex mcp add rcc-dagger \
  --env RCC_DAGGER_REPO=/var/home/kdlocpanda/second_brain/Projects/automation-control-plane/rcc \
  -- /var/home/kdlocpanda/second_brain/Areas/plugins/plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp
```

For another machine, change the script path and any optional module override to local absolute paths.

Codex loads MCP server definitions when a session starts, so start a new Codex session after adding or changing this server.

## Common Failure

If Codex says:

```text
MCP client for `rcc-dagger` failed to start: MCP startup failed: No such file or directory (os error 2)
```

check the launcher path first. That error means Codex could not exec the configured command.

If the launcher starts but prints `Dagger module path must contain dagger.json and .dagger/`, the launcher path is fine but the override path points at the wrong repo.

If Docker is unavailable or the Dagger engine cannot start, stop using this bridge for RCC work. Fall back to the normal `rcc` binary in the active project or install/fix `rcc` through the `$rcc-core` path.

Verify registration:

```bash
codex mcp get rcc-dagger
```

Verify current-directory module behavior from any Dagger module checkout:

```bash
dagger functions
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
