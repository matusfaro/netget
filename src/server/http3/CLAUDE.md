# HTTP/3 ("http3") Protocol Implementation

## What this actually is

**A raw QUIC stream server, not an HTTP/3 server.** It accepts QUIC connections
with `quinn`, advertises ALPN `h3`, accepts bidirectional streams, and hands the
raw stream bytes to the LLM. There is **no HTTP/3 framing layer**: no
HEADERS/DATA frames, no QPACK, no control or QPACK streams, no settings exchange
(RFC 9114 / RFC 9204 are not implemented).

Consequences, in plain terms:

- `curl --http3`, browsers, and other real HTTP/3 clients **cannot talk to this
  server**. They complete the QUIC handshake and then fail on the missing control
  stream / framing.
- NetGet's own HTTP/3 *client* (`src/client/http3/`) uses the `h3` crate and does
  real HTTP/3, so **the NetGet http3 client cannot talk to the NetGet http3
  server**. They are not counterparts.
- The `h3` and `h3-quinn` crates are pulled in by the `http3` feature but are not
  used by this server at all.
- The protocol name and stack string (`ETH>IP>UDP>HTTP3`) overstate what runs.
  Treat this as "QUIC streams" when reasoning about it.

**State**: Experimental. **Privilege**: declares `PrivilegedPort(443)`; the check
fires only when the requested port is actually below 1024.
**Transport RFC**: 9000/9001 (QUIC), via quinn v0.11.

Making this a real HTTP/3 server means driving `h3::server` over the quinn
connection and mapping request/response to the same event/action shape as HTTP/2,
which is a rewrite of `handle_stream_with_actions`, not a patch.

## What the model sees and controls

**Events**

| Event | When | Fields |
|---|---|---|
| `http3_connection_opened` | QUIC connection established (post-TLS) | none |
| `http3_stream_opened` | client opened a bidirectional stream | `stream_id` |
| `http3_data_received` | bytes arrived on a stream | `stream_id`, `data`, `encoding` |

**Sync actions** (available on stream/data events)

- `send_http3_data` — `data` (required)
- `wait_for_more` — accumulate rather than reply
- `close_this_stream`

**No async actions.** `send_to_stream`, `close_stream` and `list_streams` used to
be advertised and were removed: nothing consumed their results. The async path
has no stream context, so the executor serialized bytes into an `ActionResult`
that was dropped, while the model was told the action had succeeded. Do not
re-add them without an executor that owns the quinn `SendStream`.

### Encoding asymmetry (read before writing prompts)

- **Inbound**: a payload of only printable ASCII/whitespace is delivered as text
  with `encoding: "text"`; anything else is hex-encoded with `encoding: "hex"`.
- **Outbound**: `send_http3_data.data` is written to the stream **verbatim as
  UTF-8**. Hex is *not* decoded. `"48656c6c6f"` puts those ten ASCII characters
  on the wire, not the five bytes they spell.

So binary payloads cannot be sent, and a hex-encoded inbound payload cannot be
echoed back unchanged. This asymmetry is stated in the action and event parameter
descriptions the model receives. (It is the same defect shape as `send_tcp_data`,
except here the documentation now matches the executor instead of promising hex
decoding that never happens.)

## Architecture

- `Http3Server::spawn_with_llm_actions` builds a quinn `Endpoint`, forces
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
UI). `ProtocolConnectionInfo` starts as `{"stream_count": 0}` and, as with the
other HTTP protocols, is never updated afterwards; byte/packet counters stay at
zero.

## Not supported

- HTTP/3 framing, QPACK, real HTTP/3 clients (see above)
- `request_filter` / `filtered_response` — that mechanism is HTTP/1.1 + HTTP/2
  only, and `http3` does not declare those startup parameters. Passing them will
  hit `StartupParams`' undeclared-key panic. Filter traffic with a script handler
  instead.
- Unidirectional streams, DATAGRAMs, 0-RTT, connection migration, stream
  priorities
- Binary payloads in either direction (see the encoding asymmetry)
- Unbounded `wait_for_more` accumulation: no size cap, so a hostile peer can grow
  the buffer indefinitely
- Per-connection statistics

## Testing

`tests/server/http3/e2e_test.rs` — 3 mocked scenarios (echo, custom response,
multiple streams), declared in `tests/server/http3/mod.rs`. They use a raw
`quinn::Endpoint` client, which is why they pass despite the absence of HTTP/3
framing; nothing in the suite exercises a real HTTP/3 client.

```bash
./cargo-isolated.sh test --no-default-features --features http3 \
    --test server::http3::e2e_test -- --test-threads=100
```

## Example prompts

```
listen on port 4433 via http3
When you receive data on any stream, echo it back
```

```
listen on port 4433 via http3
Each stream carries one line of text. Reply with the line uppercased,
then close the stream.
```

Deterministic variant (no LLM call):

```json
"event_handlers": [{
  "event_pattern": "http3_data_received",
  "handler": { "type": "static", "actions": [
    { "type": "send_http3_data", "data": "ACK\n" }
  ]}
}]
```

## References

- RFC 9000/9001 (QUIC) — what this implements
- RFC 9114 (HTTP/3), RFC 9204 (QPACK) — what this does **not** implement
- [quinn](https://docs.rs/quinn/)
