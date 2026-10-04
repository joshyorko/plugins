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
