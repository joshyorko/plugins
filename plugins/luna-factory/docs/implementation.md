# Luna Factory implementation checkpoints

The accepted specification is [plugins #59](https://github.com/joshyorko/plugins/issues/59).
This branch implements that product in place. It does not change finish authority
or claim live ChatGPT, tunnel, model routing, or descendant-cancellation proof from fixtures.

## Ordered acceptance gaps

1. Rust MCP resource/metadata and current native Codex protocol, then a bounded owner/worker run with execution and owner acceptance proof
2. Durable run identity, repository ownership, idempotency, bounded receipts and restart/cancel/resume behavior
3. Sidebar and thread workbench using the same runtime/history, bounded model context, exact-run links, forms and settings
4. Portable packaging parity, security/recovery tests, clean builds and exact-head CI

Work is split between one runtime/publication owner, one native-protocol worker,
and one independent packaging worker. UI implementation waits for the runtime
contract. Shared generators have one writer.

## Proof record

- Base: `5c20e80cc136a982949b516b1672bb331da6de7b`
- Development CLI: `codex-cli 0.159.2`; generate schemas from this binary
- Rust SDK source inspected: rmcp 3.5.0, MCP 2026-07-28 support
- Prior skill runtime snapshot is historical; it is not current execution proof
- Baseline repository checks need the declared Python validation dependencies
- Live native inference, private ChatGPT tunnel connection and desktop/phone UI remain unproved

No merge, release, deploy, production restart, new credential, or blanket approval
is part of this implementation.

## Runtime checkpoint

The Rust service now has SQLite admission/claims, request idempotency, exact
repository subjects, finite wall/capacity limits, bounded receipts and zero-model
status reads. It connects to native app-server over stdio or proxies an existing
operator-owned daemon. The latter keeps daemon lifetime independent of MCP.
Standalone stdio recovery refuses unknown process ownership after restart.

Native adapter tests exercise real subprocess transport with a labeled synthetic
fixture; an opt-in installed-Codex test also passed initialize/model-list only.
Neither proves a live inference run. The inherited VM Codex home cannot initialize
SQLite; the clean-config audit does not inherit authentication/provider settings.
The canonical skill input schema is supported, but actual skill loading remains
unproved. App-server 0.159.2 rejects profile switching; aliases use inherited
configuration and requested Luna effort, and unsupported overrides fail closed.

Current Rust checks: 27 tests passed, 1 opt-in native test ignored by default;
locked clippy with warnings denied and fmt pass. Runtime claim fencing, recovery
and final owner reports are under independent review. UI is being implemented
against this endpoint contract. Private ChatGPT/tunnel and phone proof remain open.

## Workbench and recovery checkpoint

The bundled UI uses the official MCP App bridge plus feature-detected OpenAI
helpers. It has sidebar/thread views, structured start/steer/stop forms, settings,
exact-run host routing, summary-to-detail evidence loading and bounded context.
Its official App/AppBridge in-memory transport test is synthetic host proof only.
The cloud browser refused the loopback preview with ERR_BLOCKED_BY_CLIENT, so
visual desktop/mobile and live ChatGPT rendering remain unproved.

Independent review drove concrete regression fixes: independent durable deadline
watchdogs, unknown-resume recovery, current-policy revalidation, exclusive service
lease, reconnecting stale daemon proxies, one event listener per run, subject
hashes that include index content without executing repository helpers, and
background-terminal evidence before claim release. All native lifecycle fixture
results remain labeled synthetic. No claim of live cancellation, observed child
routing or authenticated skill execution follows from those tests.
