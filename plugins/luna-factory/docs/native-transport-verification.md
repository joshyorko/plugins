# Existing-daemon transport correction

The published 0.2.0 runtime treated `codex app-server proxy --sock …` as a JSONL
transport. That command copies raw bytes. The native control socket expects an
HTTP WebSocket upgrade followed by RFC 6455 text frames, so JSONL `initialize`
fails before model discovery.

## Protocol evidence

- Installed development CLI `0.159.0-alpha.3` documents proxy as “Proxy stdio
  bytes to the running app-server control socket.” This is CLI help evidence,
  not a probe of an operator daemon.
- Official `openai/codex` commit
  `4aa94dce270de668eff6e2fa8585c82385e84455`:
  [`app-server-daemon/src/client.rs`](https://github.com/openai/codex/blob/4aa94dce270de668eff6e2fa8585c82385e84455/codex-rs/app-server-daemon/src/client.rs)
  connects a Unix stream and uses `client_async("ws://localhost/", stream)`;
  [`unix_socket.rs`](https://github.com/openai/codex/blob/4aa94dce270de668eff6e2fa8585c82385e84455/codex-rs/app-server-transport/src/transport/unix_socket.rs)
  accepts WebSocket upgrades; CLI proxy delegates to `codex_stdio_to_uds::run`.
- CAS `bf0b3823e033b9b5abd86904e0a565d6b3586206` independently uses
  `unix_connect` locally and a WebSocket handshake over its remote raw proxy.

## Corrected boundary

`existing_daemon` now connects directly to a Unix socket. An explicit trusted
`native_socket` avoids subprocess discovery. Without it, only bounded
`app-server daemon version` runs; the response must describe a running daemon
and an absolute socket path. No start, restart, shutdown, profile, approval,
provider, or authentication command is issued. Non-Unix platforms fail with an
explicit unsupported-transport error instead of silently selecting stdio.

The existing stdio path still owns only its own app-server child. Daemon
connections own only their socket. Closing or dropping one client never kills
the daemon or establishes that daemon-owned execution has stopped.

Both transports share request correlation, bounded pending requests, event
routing, request timeouts, and sanitized errors. WebSocket frames/messages and
write buffers are bounded. The pinned WebSocket library bounds its HTTP upgrade
input to 64 KiB and 512 reads; the runtime also bounds discovery and handshake
time. RPC approval callbacks remain events and receive no automatic answer.
Protocol ping/pong is handled independently of those callbacks.

The dispatch guard runs under the actual WebSocket mutex after underlying sink
readiness, immediately before synchronous `start_send`. The mutex is held only
within a poll, never across an await. An interrupted or failed partial write
physically closes the socket, discards buffered frames, aborts its reader, and
fails pending requests. A later ping cannot finish an aborted request. Existing
Factory durable intent and unknown-claim rules continue to govern recovery;
this connector never retries inference or releases ownership.

## Regression proof and limits

The original JSONL-over-raw-proxy framing fixture failed with
`HttparseError(Token)` and `native initialize failed`. Direct Unix WebSocket
initialization and catalog reads pass against the same listener. An intermediate
split-sink implementation also failed a cancellation regression: resuming peer
reads and sending ping completed an aborted large RPC. The final whole-socket
implementation passes that regression by physically discarding the connection.

Tests use disposable sockets, synthetic scenarios, and temporary repositories.
They cover read-only doctor, running-daemon discovery and its bounds, malformed
and oversized frames, callbacks without approval, deadline refusal after native
preflight, socket shutdown/drop, interrupted writes, and existing lifecycle,
claims, and recovery behavior over real WebSocket framing. They do not prove
real owner/worker execution, native host approvals, or ChatGPT/tunnel acceptance.
No production run, service, credentials, or claim was touched by these tests.
