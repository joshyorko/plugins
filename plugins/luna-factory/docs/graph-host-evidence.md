# Luna Factory host validation baseline

This records the independent, read-only validation before implementation. The defects and line numbers below refer to that baseline; current fixes and test results are recorded in [graph-verification.md](graph-verification.md).

Inspected repository head `1ccac51591d0466250448531632b82732ddac3fe` (PR60); root AGENTS.md applies. No product edits, host deployment, inference dispatch, or production state access performed. Public sources fetched through existing proxy, without changing network policy. Findings below are source/SDK contract evidence, not live host proof.

## Source pins

- OpenAI extensions spec pinned at commit `7e1be49daea03d7ec46ed2472f410099db2743d6`: https://github.com/openai/mcp-extensions/blob/7e1be49daea03d7ec46ed2472f410099db2743d6/docs/spec.md
- Model context SDK implementation at same commit: https://github.com/openai/mcp-extensions/blob/7e1be49daea03d7ec46ed2472f410099db2743d6/typescript/src/app/model-context.ts
- Extensions capability detection: https://github.com/openai/mcp-extensions/blob/7e1be49daea03d7ec46ed2472f410099db2743d6/typescript/src/app/extensions.ts
- Settings SDK: https://github.com/openai/mcp-extensions/blob/7e1be49daea03d7ec46ed2472f410099db2743d6/typescript/src/server/settings.ts
- NPM registry latest @openai/mcp-extensions = 0.1.0 (confirmed 2026-10-08); downloaded in memory and inspected declarations/implementation. Tarball https://registry.npmjs.org/@openai/mcp-extensions/-/mcp-extensions-0.1.0.tgz ; integrity `sha512-AkQ7hRS9DSUTBHgwVwd+BO6yh1y75SldqNTt8YISUnJODKD3Esg+zC2mq96FMmgzCTV7VRykkgzBzkF6W1zkdw==`. Peers ext-apps ^1.7.5, MCP SDK ^1.29.0; dependency zod 4.4.3; Node >=22. Current repository pins are aligned; no upgrade needed.
- ext-apps 1.7.5 tarball declarations: `dist/src/app.d.ts`, `dist/src/spec.types.d.ts`. Integrity `sha512-TjPH2S2y5UEGKhmI6+XGFuqfqOV4ppe1x6DA3txnUaEWkgtA4G5vo14jGKFZmegdkZ1H4QMLyujLvoU1BEdnAg==`.

## Confirmed settings defects

1. Native settings update contract is `{set: {changed fields}}`; omitted fields stay unchanged, no deep merge/reset. Current mcp.rs:143 flat inputSchema and http.rs:139 passthrough to lifecycle.rs:2085 reject the native envelope (`unknown_setting`). UI controller.ts:179 and test/controller.test.ts:260 codify the flat form. Adopt one canonical strict nested envelope throughout. Require nonempty set, reject unknown envelope/setting keys and invalid type/range/alias values. Capacity remains constrained by trusted config and profile by approved aliases.
2. Read outputSchema already exists (mcp.rs:180); do not report it missing. Update outputSchema is absent. The spec example declares update output `{values: object}`, and SDK `OpenAISettingsUpdateResult` requires values holding ALL effective settings after successful persistence. Add update outputSchema and return `{values}`. Current `{schema,values}` return is permissively envelope-compatible but needlessly different; UI accepting raw values is not native contract validation.
3. lifecycle.rs:2086 reads under one store lock, then 2110 acquires another lock to save, then rereads. Concurrent disjoint field updates can overwrite each other and a returned result can belong to another caller. Validate requested values first; hold one lock across read/effective defaults/merge/persist/snapshot return. Native settings updates are partial; atomic persistence alone does not make read-modify-write atomic.
4. settings() returns schema profile enum `[]` plus default `""` when config has no profiles (2071+); saved values are not revalidated after trusted config changes. Native spec requires an effective schema-conforming value for every declared property. Decide explicit empty-config behavior: omit unavailable profile field/property/value (with dynamic schema) or return a truthful settings error; do not fabricate a permitted alias. Revalidate/clamp/reset invalid saved safe defaults against current config through an explicit policy.

## Exact model context SDK surface

```ts
type ModelContextParams = Parameters<App["updateModelContext"]>[0];
type OpenAIModelContextHostState = {
  updateId: string; // nonempty, opaque, renewed when contents change
  content?: MCP.ContentBlock[];
  structuredContent?: Record<string, unknown>;
} | null;
type OpenAIModelContext = {
  getCurrent(): OpenAIModelContextHostState | undefined;
  update(params: ModelContextParams, options?: RequestOptions):
    Promise<{updateId: string} | undefined>;
};
// ModelContextParams accepts optional content and structuredContent.
```

`extensions.modelContext` is undefined unless hostCapabilities.experimental["openai/modelContext"] != null. Its update simply calls app.updateModelContext and parses the response metadata. It does NOT maintain a local current-state cache, listen independently, or select a supported payload modality. getCurrent parses the current App host context, returning undefined for absent/malformed payloads and null for explicit clear. `hostcontextchanged` listeners receive partial patches; ext-apps merges them into getHostContext BEFORE listeners execute. Initial host context does not dispatch a hostcontextchanged event, so inspect after connect too.

## Context lifecycle and multi-instance recommendations

- Defect: main.ts never calls modelContext.getCurrent, and applyHostContext only reads theme/styles/deepLink. Initial/re-mounted selected context is ignored, initial overview can overwrite retained host context, and a user clear is forgotten. Controller only deduplicates last outbound serialized bytes. Later state changes reattach user-cleared context.
- Extract actual production bridge/context lifecycle into a testable module; the existing protocol test instantiates its own SDK calls and cannot detect main.ts integration defects.
- Treat undefined/absent context as unknown/no extension state, explicit null as cleared, valid object as current host state. Never turn unrelated theme patches or malformed context into clear. Read initial state after connection before first outgoing publish. Restore at most a validated run ID, fetch trusted run detail, and derive bounded payload from server data rather than trusting host-supplied objective/authority text. Explicit deep link/user navigation should win over late restore. Do not use host context as mutation authorization.
- On host clear, preserve the visible run if desired but suppress automatic republishing for that selection until deliberate user select/reattach. Invalidate queued writes. Track own update IDs to recognize echoes, but never order IDs lexically/numerically: they are opaque. Late acknowledgements must not roll state back or re-enable suppressed attachment. A clear racing an already-sent write requires explicit ordering/settling behavior and a test; no client cancellation can promise an already-sent request never reached the host.
- Defect: helper-present path main.ts:29 always sends structuredContent without examining host updateModelContext modalities. Check structuredContent/text support independently of extension availability; use structured content where supported, bounded text where only text is supported, and an explicit unsupported status otherwise.
- Defect: overview on text-only fallback sends a text attachment containing `{}` (main.ts:32), not an empty attachment set. Use a clear operation represented on wire as empty context, e.g. `update({content: []})`; optional structured content should be omitted/empty appropriately. Per SDK/spec each call replaces prior context, so no stale prior block may survive. Generic structured-only fallback can remove data with empty structuredContent; test negotiated shape and no stale run ID.
- Unsupported hosts currently silently report successful publish. Degrade without disrupting run operations, but distinguish unsupported/unavailable from acknowledged context update; show useful status only as needed.
- Spec scope is one MCP App INSTANCE: each update replaces only its own context. Every thread entrypoint has a distinct instance. Global desktop entrypoints have their own composer/thread and current-thread operations target it. There is no documented cross-instance shared selection or automatic global/thread synchronization. Keep controller/context bookkeeping instance-local. Do not add server-global selectedRun, localStorage selection, or module-global cross-instance registry. Server's shared run/settings data is appropriate; per-instance selection/context is separate.
- open_factory_panel description says currently selected run, but server accepts {} and workbench(None), so no server-side selection exists. Clarify description or restore from that instance's host context; never infer another instance's selected run.

## Host limitations and claims

Pinned spec's table is EXPECTED DevDay launch support, not evidence tested in this app. Web means Work browser and excludes classic ChatGPT. Global/thread entrypoints, structured settings, display modes and model context are listed across desktop/web/iOS/Android. Deep links are not supported on Android; thumbnails not supported on iOS. File entrypoints, local files/resources and composer mentions are desktop only. OpenAI form elicitation is desktop/web only; complete HTML forms remain necessary on phones. Display modes inline/fullscreen supported, pip not supported; entrypoints use fullscreen.

main.ts:142 checks app experimental openai/elicitation, but documented capability is SERVER-facing MCP initialize request capabilities.extensions["openai/elicitation"].form. This app SDK has no native form method; current data-native-forms=host-supported should not be treated as actual host support proof. Server rich elicitation is not necessary to fix settings/context. OpenAI-registered servers require MCP 2026-07-28 MRTR for rich elicitation; direct MCP accepts legacy and MRTR. Current settings advertisement in initialize.extensions with MCP 2025-11-25 is explicitly supported, so no need to upgrade protocol merely for settings.

## Required focused tests

1. Rust MCP contract: read/update tool schemas, strict nested nonempty set, app visibility, both outputSchemas, actual host-shaped tool invocation response, malformed envelope rejected before persistence.
2. Rust settings behavior: partial patch retains all omitted fields, complete effective result, unknown/range/type/alias rejected, failed persistence not success, simultaneous disjoint updates preserve both changes; effective values valid under no-profile/config-change policy.
3. Production bridge + App/AppBridge transport: negotiate OpenAI structured, OpenAI text-only, generic structured, generic text-only, unsupported; verify payloads and absence of direct HTTP/inference fallback.
4. Initial context valid/null/absent/malformed; remount exact selection restoration; host replacement; explicit host clear; unrelated theme patch; own echo; late ack after clear; pending/queued publish invalidation; rejection then deliberate retry; no automatic reattach after periodic refresh.
5. Overview really clears both structured and text representations, including outgoing update racing clear.
6. Two independent instances (global + thread / two threads) select different runs without overwriting each other's host context; remount one retains only its own state. This is synthetic contract proof only.
7. Real supported desktop/Work web plus iOS/Android manual QA for rendering, settings, context removal/remount and navigation, recording precise host/build and limitations. Never mark fixture transport, screenshots, compile or spec table as live host/phone proof.
