# HTTP/2 Protocol Implementation

HTTP/2 server built directly on the `h2` crate. `h2` owns framing, HPACK,
multiplexing and flow control; the LLM owns the response status, headers and text
body, plus optional server pushes.

**State**: Experimental — not human-reviewed against a broad client set.
**Privilege**: declares `PrivilegedPort(443)`; the check fires only when the
requested port is actually below 1024. **RFC**: 7540 / 7541.

Shared plumbing (request extraction, response building, request filter) lives in
`src/server/http_common/` — read `src/server/http_common/CLAUDE.md`.

## One server — `h2_server.rs`

`Http2Protocol::spawn()` calls `H2Server::spawn_with_push_support`
(`h2_server.rs`), because hyper's service API cannot express server push. All
request handling lives there; `mod.rs` is module declarations and the `H2Server`
re-export only.

There used to be a second, hyper-based `Http2Server` in `mod.rs`. It compiled and
was exported as `server::Http2Server`, but nothing ever routed traffic to it, so
edits to it had no effect on a running server. That bit the request filter: it
was wired only into the dead path, so `request_filter` was accepted and silently
ignored for HTTP/2 until it was added to `h2_server.rs` as well. The dead server
was removed rather than kept as a "reference implementation" — a second copy of
request handling that no test and no client ever exercises only invites the same
mistake again. **Do not reintroduce a second server here.**

The `h2c` upgrade path from HTTP/1.1 (`src/server/http/mod.rs`) also lands in
`h2_server::handle_h2_request`, carrying the HTTP/1.1 connection's filter with it.

## What the model sees and controls

**Event**: `http2_request`, one per HTTP/2 stream.

`method`, `uri` (path+query), `version`, `headers` (lowercase map), `body` (UTF-8
lossy), `body_bytes`.

**Actions**:

- `send_http2_response` — `status` (required, 100-599), `headers`, `body`
  (optional; omit for 204/304). Same executor as HTTP/1.1.
- `push_resource` — `path` (required), `status`, `headers`, `body`. Emit it in
  the same batch as the response; pushes are sent as PUSH_PROMISE + a push stream
  **before** the main response. A client that has disabled push (most modern
  browsers) rejects it; the push is then dropped with a warning and only the main
  response is delivered.

Same hard limits as HTTP/1.1: **one response per stream, sent complete; no
streaming or chunking; text bodies only, no binary payloads;** request bodies are
fully buffered before the LLM sees them. Do not set HTTP/2 pseudo-headers
(`:status`, `:path`, …) or `content-length` — `h2` handles them; illegal header
names/values are dropped rather than sent.

Unlike HTTP/1.1, the event does **not** carry a `body_is_binary` flag: a non-UTF-8
body is decoded lossily with no signal. Add it here if this protocol is promoted.

### Failure behavior

- LLM error → `500` with body `Internal Server Error`.
- No response action → empty `200`.
- Invalid status/header from the model → 500 / header dropped
  (`build_h2_response_head`), never a panic and never a dead stream.
- Errors from `send_response`/`send_data` propagate out of `handle_h2_request`
  and are logged; the peer sees a reset stream.

## Architecture

- `H2Server::spawn_with_push_support` binds through
  `create_reusable_tcp_listener`, propagates bind failure, and registers the
  accept-loop `JoinHandle` via `AppState::register_server_task()` so
  `stop_server` releases the socket.
- Optional TLS via `tls_cert_manager` (`tokio_rustls` in front of the `h2`
  handshake). **ALPN is never advertised**, so a browser will not select HTTP/2
  over TLS on its own — clients must pick `h2` explicitly, or use cleartext h2c.
- One task per connection, one task per stream. Streams on a connection are
  processed concurrently; the Ollama lock still serializes the LLM calls.
- The request filter is built **once per server** in
  `spawn_with_push_support` (not per connection), and
  `RequestFilter::warnings()` is forwarded to the status channel — parsing is
  fail-open, so a typo means more LLM traffic, not less.
- Handling mode priority is the generic one: script → static → LLM, through
  `call_llm` → `try_execute_event_handler`.

### Connection state

One `ConnectionId` per TCP connection (not per stream), `ProtocolConnectionInfo`
initialized to `{"recent_requests": []}`. As with HTTP/1.1, **nothing updates it
afterwards** — byte/packet counters and `last_activity` stay at their initial
values.

## Testing

`tests/server/http2/e2e_test.rs` — 3 mocked scenarios, declared in
`tests/server/http2/mod.rs`.

```bash
./cargo-isolated.sh test --no-default-features --features http2 \
    --test server::http2::e2e_test -- --test-threads=100
```

**Gaps**: no coverage of server push, of the request filter on the HTTP/2 path,
of TLS, or of the h2c upgrade from HTTP/1.1.

## Example prompts

```
listen on port 8080 via http2
For GET /, return JSON {"message": "Hello HTTP/2"}
For POST /api/users, parse the JSON body and return 201
```

Deterministic variant (no LLM call):

```json
"event_handlers": [{
  "event_pattern": "http2_request",
  "handler": { "type": "static", "actions": [
    { "type": "send_http2_response", "status": 200,
      "headers": {"Content-Type": "application/json"},
      "body": "{\"message\": \"Hello from HTTP/2!\"}" }
  ]}
}]
```

## Not implemented

ALPN negotiation · stream prioritization control · streaming/chunked responses ·
binary bodies · LLM control over connection lifetime · per-connection stats ·
`body_is_binary` signalling · trailers.

## References

- RFC 7540 (HTTP/2), RFC 7541 (HPACK)
- [h2](https://docs.rs/h2/latest/h2/)
