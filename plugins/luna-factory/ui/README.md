# Luna Factory workbench

This is the MCP App inside the canonical Luna Factory plugin. It has no separate
backend, hosted service, inference loop, or account identity.

## Build and check

Use Node.js 22 or newer and npm from this directory:

```sh
npm ci
npm run typecheck
npm test
npm run build
```

The build emits only `dist/index.html`, with JavaScript and CSS inline. The Rust
server serves that file as `text/html;profile=mcp-app`. No remote fonts, images,
scripts, stylesheets, or network fetches are required by the resource. The small
Vite inliner fails on an unresolved or unexpected asset.

The official dependency pair is `@modelcontextprotocol/ext-apps` 1.7.5 and
`@openai/mcp-extensions` 0.1.0. The latter's published peer contract requires
ext-apps `^1.7.5`; ext-apps 2.x is not silently substituted. `package-lock.json`
pins the complete dependency graph.

## Runtime contract

- Register `App.ontoolresult` before `connect()`. Render the initial result without
  repeating the opening tool call.
- Use `App.callServerTool` for the existing Rust MCP tools. There is no HTTP API
  fallback or invented client-to-server command.
- Home groups runs into Needs you, In progress, Planned · not started, and History.
  Needs you is exactly `needsOperatorDecision`. List summaries may omit receipts.
  When the server offers the read-only `list_factory_campaigns`, Parent campaigns
  shows each campaign row, then its planning plan, then any linked runs. Linked runs
  leave the other tiers, but a linked run that needs a decision still appears in Needs
  you. Opening a campaign opens its planning run. A failed read keeps the last valid
  grouping, marked stale. A server without the tool keeps the ungrouped tiers.
  Opening a run calls `get_factory_run`, then reads its plan with `get_factory_graph`;
  both are reads. Map and Lanes share one task-and-agent selection. See
  `../docs/mission-control.md` for the design and the contract matrix.
- Tool responses are validated at the boundary. Unknown fields are stripped.
  Errors keep the last valid state. Read request tokens and server generation /
  update time fence late snapshots; repeated opening results cannot roll back a
  newer view. Pending mutations disable duplicate submissions.
- Stop waits for the server's outcome. It never optimistically claims execution
  has stopped or releases ownership. Resume uses the existing run ID. Forms expose
  no force-completion, force-unlock, native approval, or permission expansion.
- Identical uncertain start retries reuse an idempotency key. Changing the request
  creates a new key. There is no automatic retry of state-changing requests.
- Status refresh uses `refresh_factory` every 30 seconds while visible, connected,
  and not editing or waiting on an action. It is a persisted-state read, not a
  model polling loop. A stale plan is reread after the poll.
- Settings save only capacity, finish, and an approved profile alias. Repository
  access, providers, credentials, and trusted limits remain server-owned.

## Host compatibility

The OpenAI SDK helper is feature-detected for Model-App Context. Standard
`ui/update-model-context` is the fallback when advertised. Only bounded selected-run
fields are sent: run ID, revision, repository, objective, state, subject, mandatory
gap, blocker, and finish authority. A selected task adds its ID, title, state and
graph revision. A selected agent adds its display label, role, liveness and bound
task ID, never a raw thread ID. No receipts or logs are sent to model context.
Updates are serialized, and overview clears selection.

`openai/deepLink.url` is read through the official helper on initialization and
host-context changes. Only `/runs/:id` with a validated exact ID is accepted;
external URLs, traversal, encoded slashes, fragments, and ambiguous paths are
rejected. In-app links use that relative route. This portable package does not
know an assigned registered plugin ID, so it does not fabricate a ChatGPT or
Codex deep-link URL.

Native OpenAI form elicitation is a server/host capability, not an App SDK method.
The UI detects its advertised availability but always supplies complete, labeled
HTML forms. If a tool elicits native input, the MCP host owns that interaction.
This fallback also covers mobile hosts without native rich elicitation. Theme
and style variables use the standard host context.

Text-only/structured tool fallback belongs to the Rust server. Unsupported UI or
model-context features never trigger an alternate backend or inference call.

## Fixture preview and verification limits

```sh
npm run dev -- --port 4179
# Open http://127.0.0.1:4179/?preview=fixture
```

The fixture preview is development-only, visibly labeled, synthetic, and
read-only. Fixture data and preview actions are absent from the production build.
It is not evidence of a native owner run, ChatGPT connection, tunnel, or phone
integration.

Automated tests cover malformed and repeated results, late selections and
refreshes, generation fencing, empty/blocked states, pending/cancel/resume/error
flows, bounded context, exact-run routes, escaping, labeled forms, and draft
preservation. An official App/AppBridge test negotiates through the SDK's
in-memory transport and checks initial results, tool calls, OpenAI model-context
updates, and deep-link notifications. It uses a fixture host, not live ChatGPT.

The cloud-browser visual check on 2026-10-04 was blocked by
`net::ERR_BLOCKED_BY_CLIENT` for the loopback development URL. No screenshot or
native phone proof is claimed. Desktop, narrow viewport, theme, and real-host
interaction QA remain required in an environment allowed to reach the local
resource.

Sources checked: the official ext-apps package API, the published OpenAI SDK peer
contract, and [OpenAI MCP Extensions](https://github.com/openai/mcp-extensions/blob/main/docs/spec.md).
