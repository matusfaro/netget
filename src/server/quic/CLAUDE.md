# QUIC ("quic") Protocol Implementation

## What this is

**A raw QUIC stream server.** It accepts QUIC connections with `quinn`, advertises
ALPN `h3`, accepts bidirectional streams, and hands the raw stream bytes to the
LLM. Nothing else in NetGet offers a raw QUIC stream, and that is the capability
this protocol exists to provide: multiplexed bidirectional streams under TLS 1.3
with the model owning every byte.

**It is not an HTTP/3 server.** There is no HTTP/3 framing layer: no HEADERS/DATA
frames, no QPACK, no control or QPACK streams, no settings exchange. RFC 9114 and
RFC 9204 are not implemented, and the `h3` / `h3-quinn` crates are not used here —
the `quic` feature does not depend on them.

Consequences, in plain terms:

- `curl --http3`, browsers, and other real HTTP/3 clients **cannot talk to this
  server**. They complete the QUIC handshake and then fail on the missing control
  stream / framing. The peer has to be a raw QUIC client (`quinn::Endpoint` or
  equivalent).
- NetGet's HTTP/3 *client* (`src/client/http3/`, feature `http3`) uses the `h3`
  crate and does real HTTP/3, so **it cannot talk to this server**. They are not
  counterparts and never were; since the feature split they are not even built
  together. See "If a real HTTP/3 server is wanted" below.
- `keywords()` deliberately does **not** include `http3`, so a request for an
  HTTP/3 server does not silently resolve to a raw QUIC socket. It resolves to
  nothing, which is the honest answer — the same fix applied when `ftp` used to
  resolve to TCP.

**State**: Experimental. **Privilege**: declares `PrivilegedPort(443)`; the check
fires only when the requested port is actually below 1024.
**Transport RFC**: 9000/9001 (QUIC), via quinn v0.11.
**Feature**: `quic` (`dep:quinn`, `dep:rustls`, `dep:rustls-pemfile`, `dep:rcgen`,
`dep:webpki-roots`).

## What the model sees and controls

**Events**

| Event | When | Fields |
|---|---|---|
| `quic_connection_opened` | QUIC connection established (post-TLS) | none |
| `quic_stream_opened` | client opened a bidirectional stream | `stream_id` |
| `quic_data_received` | bytes arrived on a stream | `stream_id`, `data`, `encoding` |

**Sync actions** (available on stream/data events)

- `send_quic_data` — `data` (required)
- `wait_for_more` — accumulate rather than reply
- `close_this_stream`

**No async actions.** `send_to_stream`, `close_stream` and `list_streams` used to
be advertised and were removed: nothing consumed their results. The async path
has no stream context, so the executor serialized bytes into an `ActionResult`
that was dropped, while the model was told the action had succeeded. Do not
re-add them without an executor that owns the quinn `SendStream`.

`quic_connection_opened` carries `.with_no_actions()` rather than an empty list:
both send actions address a stream, and no stream exists yet. That marks the
absence as deliberate for `tests/event_action_declarations_test.rs`.

### Encoding asymmetry (read before writing prompts)

- **Inbound**: a payload of only printable ASCII/whitespace is delivered as text
  with `encoding: "text"`; anything else is hex-encoded with `encoding: "hex"`.
- **Outbound**: `send_quic_data.data` is written to the stream **verbatim as
  UTF-8**. Hex is *not* decoded. `"48656c6c6f"` puts those ten ASCII characters
  on the wire, not the five bytes they spell.

So binary payloads cannot be sent, and a hex-encoded inbound payload cannot be
echoed back unchanged. This asymmetry is stated in the action and event parameter
descriptions the model receives. (It is the same defect shape as `send_tcp_data`,
except here the documentation matches the executor instead of promising hex
decoding that never happens.)

## Architecture

- `QuicServer::spawn_with_llm_actions` builds a quinn `Endpoint`, forces
  `alpn_protocols = ["h3"]`, allows 100 concurrent bidi/uni streams, propagates
  bind failure with `?`, and registers the accept-loop `JoinHandle` via
  `AppState::register_server_task()`.
- TLS 1.3 is mandatory. `startup_params` may supply a cert; otherwise a
  self-signed one is generated (`tls_cert_manager::generate_default_tls_config`),
  so clients must disable certificate validation.
- One task per connection, one per stream. Per-stream state machine
  (Idle → Processing → Accumulating) with a queue, mirroring the TCP
  implementation; data arriving during an LLM call is queued and merged.
- `call_llm` is used for every event, so script and static handlers run without
  an LLM call.
- Stream lookups after re-acquiring the lock are fallible by nature (the peer can
  reset a stream mid-call) and are handled with `let … else`, not `unwrap`.

### Connection state

One `ConnectionId` per QUIC connection plus a separate id per stream (both drawn
from the same unified id counter, so stream ids appear as connection ids in the
UI). **Only the connection is in the server's connection map** — stream ids are
not, so statistics must be recorded against `connection_id`; an update keyed by a
stream id silently does nothing.

Byte/packet counters and `last_activity` are maintained per stream read and per
stream write, threading `connection_id` down through
`handle_stream_with_actions` into `handle_data_with_actions`. Inbound bytes are
counted in the read loop, before dispatch, so data that is queued rather than
processed is still counted exactly once. This keeps `cleanup_old_connections`
(10s idle threshold, `src/state/server.rs`) from evicting a live QUIC connection
whose stream is merely slow. See `src/server/http/CLAUDE.md` for who reads them.

**Still a gap**: `ProtocolConnectionInfo` starts as `{"stream_count": 0}` and is
never updated — the count does not move as streams open and close.

## Not supported

- HTTP/3 framing, QPACK, real HTTP/3 clients (see above)
- `request_filter` / `filtered_response` — that mechanism is HTTP/1.1 + HTTP/2
  only, and `quic` does not declare those startup parameters. Passing them is
  rejected by `StartupParams` as an undeclared key and the server does not start.
  Filter traffic with a script handler instead.
- Unidirectional streams, DATAGRAMs, 0-RTT, connection migration, stream
  priorities
- Binary payloads in either direction (see the encoding asymmetry)
- Unbounded `wait_for_more` accumulation: no size cap, so a hostile peer can grow
  the buffer indefinitely
- A live `stream_count` in `ProtocolConnectionInfo`

## If a real HTTP/3 server is wanted

Add it as a **separate** protocol beside `quic`. Do not convert this one.

Converting would delete the only raw-QUIC capability NetGet has and replace it
with a third request/response HTTP server whose event and action surface would be
a copy of HTTP/2's — the capability traded away is larger than the one gained.
The `h3` crate is also pre-1.0 (0.0.x) with a server API that has moved
repeatedly, which buys recurring breakage for an Experimental protocol.

Model a new HTTP/3 *server* on `src/server/http2/h2_server.rs` (the `h3` server
API is shaped like `h2`'s) and reuse `src/server/http_common/` for request
extraction, the request filter and response building. It would gate on a feature
pulling in `h3`/`h3-quinn` — which the existing `http3` client feature already
does — and would give `src/client/http3/` a real counterpart for the first time.

The current client/server mismatch is not on its own a defect: NetGet's TCP
client cannot meaningfully drive NetGet's DNS server either, and pairing is not a
design invariant. What used to make this one grate was the shared name, which the
`http3` → `quic` split fixed.

## Testing

`tests/server/quic/e2e_test.rs` — 3 mocked scenarios (echo, custom response,
multiple streams), declared in `tests/server/quic/mod.rs`. They use a raw
`quinn::Endpoint` client, which is why they pass despite the absence of HTTP/3
framing; nothing in the suite exercises a real HTTP/3 client.

```bash
./cargo-isolated.sh test --no-default-features --features quic \
    --test server -- --test-threads=100 quic
```

## Example prompts

```
listen on port 4433 via quic
When you receive data on any stream, echo it back
```

```
listen on port 4433 via quic
Each stream carries one line of text. Reply with the line uppercased,
then close the stream.
```

Deterministic variant (no LLM call):

```json
"event_handlers": [{
  "event_pattern": "quic_data_received",
  "handler": { "type": "static", "actions": [
    { "type": "send_quic_data", "data": "ACK\n" }
  ]}
}]
```

## References

- RFC 9000/9001 (QUIC) — what this implements
- RFC 9114 (HTTP/3), RFC 9204 (QPACK) — what this does **not** implement
- [quinn](https://docs.rs/quinn/)
