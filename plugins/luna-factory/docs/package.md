# Package and operator boundaries

Luna Factory ships as one plugin at `plugins/luna-factory/`. Its one canonical policy is `skills/luna-factory/SKILL.md`. The Rust runtime and workbench use that policy; they do not introduce a second planner or a second run history. Direct `$luna-factory` use remains supported without starting the runtime.

## Canonical files

- `plugin.json` owns portable identity and `extensions.com.openai.interface` owns OpenAI listing metadata
- `mcp.json` owns the portable MCP connection
- `.codex-plugin/plugin.json` and `.mcp.json` preserve Codex compatibility
- `skills/luna-factory/` owns the skill, references, runtime audit, and evals
- `server/` contains the Rust control plane
- `ui/` contains the MCP Apps workbench

The OpenAI extension replaces the compatibility overlay as a whole. The marketplace builder reads the portable interface when that extension exists. It falls back to the compatibility interface only when the extension is absent. `bin/check` rejects interface drift between both manifests. Other plugins without an inline extension retain their compatibility behavior. See the [OpenAI package contract](https://developers.openai.com/plugins/build/plugins).

Keep shared identity, version, descriptions, keywords, and the OpenAI interface synchronized. After changing these files or `marketplaces/catalog.json`, run from the repository root:

```bash
python3 scripts/build_marketplaces.py
python3 scripts/build_runtime_views.py
python3 scripts/build_hermes_plugins.py
bin/check
```

The generated marketplace, Claude manifest, Hermes manifest, and skill symlinks are views of the canonical package. Do not author a second skill in those views. The Hermes compatibility shim registers skills; it does not start the daemon or establish MCP Apps compatibility.

## Local transport contract

The default endpoint is `http://127.0.0.1:8787/mcp`. Start the daemon yourself using the operator configuration. The MCP manifest describes a connection, not a service supervisor.

Portable `mcp.json` uses the published Agent Plugins 1.0.0 shape:

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
  "mcpServers": {
    "luna-factory": {
      "type": "streamable-http",
      "url": "http://127.0.0.1:8787/mcp"
    }
  }
}
```

The compatibility map uses `type: "http"` for the same server name and URL, with `mcpServers: "./.mcp.json"` in the Codex manifest. The validator checks transport and endpoint parity. Portable URLs cannot expand environment variables. HTTP is valid here because the address is loopback. Do not replace it with a public listener or put credentials in manifest headers. See the [Agent Plugins MCP specification](https://agent-plugins.org/specification#72-mcp-servers).

If the operator changes the port, both local connection maps must match that endpoint. Keep machine-specific changes in the installed/private package, not in the distributed source. The daemon's own configuration remains authoritative for repository aliases, runtime profiles, and limits.

## Private ChatGPT connection

Use the official [Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels) and [tunnel-client](https://github.com/openai/tunnel-client). Run the client in the same trust boundary as the daemon, forwarding to the loopback MCP URL. Its credentials belong in the operator's private credential mechanism. This repository contains no tunnel identity, API key, registered app ID, or `.app.json` mapping.

Start with `tunnel-client help quickstart` for the installed client. Configure its HTTP target as `http://127.0.0.1:8787/mcp`, then verify the chosen profile with `tunnel-client doctor --profile <profile> --explain`. Keep the client running while connecting and testing the developer-mode app. Do not add a parallel tunnel protocol or inference gateway to Luna Factory.

Tunnel access and ChatGPT developer-mode access require separate permissions. Use the intended ChatGPT workspace and Platform organization when creating the private connection. If a registration mapping is needed, add the real assigned ID only to that operator's private package. App registration changes and credential grants require the operator's approval.

Secure MCP Tunnel currently supports private testing, not public plugin distribution. A public listing needs a supported public HTTPS endpoint and separate publication review. Installing this repository package does not prove public-directory eligibility.

## Authority and storage

The operator config allowlists canonical repository roots and trusted Codex profiles. Remote requests choose aliases, not paths, executable names, provider URLs, or credentials. Store the SQLite database outside target repositories and keep it across daemon updates. Codex retains its native thread history; the database stores bounded run state and evidence pointers.

A runtime action still needs explicit user authority. Installing the plugin, reading status, or invoking the skill does not authorize starting a runtime-managed run. The supported finish choices are local candidate, push, and PR, constrained by the operator's policy and the specific request. Merge, release, deploy, force-unlock, and force-CONVERGED are outside this issue.

Inherit the selected local Codex configuration, including its existing supported provider route. Approved runtime aliases currently vary effort only; non-null `codex_profile` switching remains unsupported on the verified 0.159.2 protocol and must fail closed. A blocked approval or unavailable route is a blocker. Never change sandbox/approval settings or substitute a provider to make a smoke test pass.

## Evidence required before release

A passing build or a served HTML resource is not live product acceptance. Record the exact source revision, Codex version, generated schema identity, runtime/UI build identity, requested versus observed model/effort, and the relevant native run/thread/turn IDs. Redact credentials and keep private prompts and logs outside the repository.

Run these checks against a disposable allowlisted Git repository:

1. Initialize MCP, discover tools, read the MCP Apps HTML resource, and confirm both standard UI metadata and OpenAI entrypoints survive the actual connection
2. Start one bounded objective with explicit acceptance and finish authority. Observe one native owner using the canonical skill, one bounded native worker, execution evidence, owner verification, and a durable result
3. Repeat an identical start key and payload. Confirm the same run returns; change the payload with that key and confirm conflict
4. Read/list/refresh status and confirm those requests produce zero model calls
5. Steer the existing owner, reject stale-generation steering, and preserve the run's authority and limits
6. Cancel active owned work and verify descendants have stopped before the repository claim is released. Unknown activity must remain blocked or interrupted
7. Restart the service, reconcile persisted runs, and resume the same native thread without duplicate execution or reset budgets
8. Disconnect/reconnect the tunnel and app. Confirm the same run history remains and no useful execution is implicitly cancelled
9. Open the real ChatGPT global workbench and thread panel. Check selected-run context, exact-run navigation, controls, error states, and narrow-screen layout on supported desktop and phone surfaces
10. Exercise permission refusal, an unavailable model/profile, repository claim conflicts, and the actual one-decision NEEDS_INPUT flow

Mark each gate passed, failed, or unproved with its evidence and resumption condition. A missing account, permission, installed runtime capability, or client surface is an unproved gate. Preserve the canonical [skill eval limitations](../skills/luna-factory/references/evals.md); this package work does not retroactively pass them.

MCP Events are a separate optional gate. Only claim delivery after the private developer-mode/tunnel path actually delivers the bounded event. Ordinary MCP reads remain available when event transport is unsupported.
