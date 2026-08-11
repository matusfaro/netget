# WebSocket (RFC 6455) Client

**Status**: Experimental · **Feature**: `websocket` · **Privilege**: `None`
**Scheme**: `ws://` only — see [Not implemented](#not-implemented)

## Library choice

`tokio-tungstenite` **0.21** `connect_async`, the same version the server side uses and the same
one already in the tree for `webrtc`. It performs the whole upgrade: generating the 16-byte
`Sec-WebSocket-Key` nonce, verifying the server's `Sec-WebSocket-Accept`, and — the part a client
can get wrong invisibly — masking every outgoing frame with a fresh 32-bit key as RFC 6455 §5.3
requires. Many servers tolerate unmasked client frames, so a bug there would not surface against
a lenient peer; `tests/client/websocket/e2e_test.rs` therefore asserts the mask bit explicitly.

What this module owns is the NetGet-shaped part: turning startup parameters into the request, and
turning received frames into events the model answers with actions.

## Vocabulary mirrors the server

`send_websocket_text`, `send_websocket_binary` (same `data` + `encoding` pair),
`send_websocket_ping` and `close_websocket` mean the same thing on both sides, so a prompt
written for one reads correctly on the other. `decode_outbound_payload` and
`encode_inbound_payload` are imported from `crate::server::websocket::actions` rather than
duplicated — one implementation, one behaviour, and the symmetry cannot drift between the two
sides.

## Events

| Event | When | Actions |
|---|---|---|
| `websocket_client_connected` | handshake done; carries the negotiated subprotocol | send text/binary/ping, close, disconnect |
| `websocket_client_text_message` | a text message from the server | the above plus `wait_for_websocket_data` |
| `websocket_client_binary_message` | a binary message; `data` + `encoding` round-trip | same |
| `websocket_client_closed` | the server closed | **`with_no_actions()`** — nothing can go on the wire after the closing handshake, so the common actions (`show_message`, `set_memory`, `append_to_log`) are the only honest vocabulary |

Pings from the server are answered automatically and pongs are logged, neither raises an event.

## Startup parameters

All three are declared and read.

| Parameter | Effect |
|---|---|
| `path` | request path (and query string), default `/`. A leading `/` is added if missing. |
| `subprotocols` | array offered in `Sec-WebSocket-Protocol`, most preferred first |
| `headers` | extra request headers. `Sec-WebSocket-Key`/`-Version`/`-Accept`, `Connection`, `Upgrade` and `Host` are **ignored with a WARN** if supplied — the library computes them, and overwriting the key would break the accept-key check it then performs. |

`remote_addr` accepts `host:port` (combined with `path`) or a full `ws://…` URL. A `wss://` or
`https://` address fails immediately with an explanatory error rather than being silently
downgraded.

## Connection state machine

Same Idle → Processing → Accumulating machine as the server, with the same
`wait_for_websocket_data` semantics (hold this message, join the next of the same kind onto it).
One writer task owns the `SplitSink`; every action sends a `WsOut` down an `mpsc` channel, so no
lock is held across an `.await`.

## Validation

Against a **hand-written WebSocket server in `tests/client/websocket/e2e_test.rs`** that parses
the upgrade itself and recomputes `Sec-WebSocket-Accept` with `sha1` + `base64` directly. It
asserts that the client sends a 16-byte base64 nonce and `Sec-WebSocket-Version: 13`, that the
`path` and `subprotocols` startup parameters reach the wire in order, that **every** client frame
carries the mask bit, and that a binary message whose bytes are neither valid UTF-8 nor printable
comes back byte-for-byte when the event's `data` and `encoding` are fed straight into
`send_websocket_binary`.

## Not implemented

- **`wss://`.** The `tokio-tungstenite` dependency is built without a TLS backend (no
  `native-tls` / `rustls-tls` feature), and enabling one is a `webrtc`-wide dependency decision.
- **permessage-deflate (RFC 7692)** and every other extension.
- **Reconnection.** A closed connection stays closed; the model must open a new client.
- The repo-wide client limitation applies: `remove_client()` does not stop the network loop
  (`register_client_task` is called, so `stop_client` aborts it, but see
  `CLIENT_PROTOCOL_FEASIBILITY.md` for the general state of clients).
