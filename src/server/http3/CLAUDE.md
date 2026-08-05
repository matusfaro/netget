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
- The `h3` and `h3-quinn` crates are not used by this server at all. **They are
  still required**, because the `http3` feature gates the client too and
  `src/client/http3/mod.rs:175-176` is built on them. Do not try to drop them
  from the feature — it is a single flag covering both halves.
- The protocol name and stack string (`ETH>IP>UDP>HTTP3`) overstate what runs.
  Treat this as "QUIC streams" when reasoning about it.

**State**: Experimental. **Privilege**: declares `PrivilegedPort(443)`; the check
fires only when the requested port is actually below 1024.
**Transport RFC**: 9000/9001 (QUIC), via quinn v0.11.

## Naming: this should be `quic`, and should not become HTTP/3

The standing recommendation is to **rename the protocol to `quic` and keep it as
a QUIC stream server**, not to implement RFC 9114 on top of it.

Why not implement HTTP/3 here:

- What runs today is a coherent, working, tested capability — multiplexed
  bidirectional QUIC streams under TLS 1.3 with the model owning every byte.
  Nothing else in NetGet offers a raw QUIC stream. Converting this to HTTP/3
  *removes* that and replaces it with a third request/response HTTP server whose
  event and action surface would be a copy of HTTP/2's. The capability traded
  away is larger than the one gained.
- The `h3` crate is pre-1.0 (0.0.x) and its server API has moved repeatedly.
  Building an Experimental protocol on it buys recurring breakage.
- It is a rewrite of `handle_stream_with_actions` plus a new event/action pair
  (`http3_request` / `send_http3_response`) plus new tests — not a patch. The
  existing `http3_stream_opened` / `http3_data_received` / `send_http3_data`
  surface and every prompt written against it would break.

If a real HTTP/3 server is wanted later, add it as a **separate** protocol beside
`quic`, modelled on `src/server/http2/h2_server.rs` (the `h3` server API is
shaped like `h2`'s) and reusing `src/server/http_common/` for request extraction,
the request filter and response building.

Note that the client/server mismatch is not on its own a defect — NetGet's TCP
client cannot meaningfully drive NetGet's DNS server either, and pairing is not a
design invariant. What makes this one grate is only the shared name, which is
what the rename fixes.

### What the rename needs, exactly

Landed already (inside this module):

- `keywords()` in `actions.rs` now leads with `quic`, so a model asked for a QUIC
  server finds this one. Keywords are descriptive text, not a lookup key, so this
  is safe on its own.
- `metadata()`, `description()` and `example_prompt()` already describe a QUIC
  stream server (`e32cf485`); nothing there claims HTTP/3.

Still required, all in files this module does not own:

| File | Change |
|---|---|
| `Cargo.toml` | rename feature `http3` → `quic`; update `all-protocols` and the two other feature lists that name it (lines ~31, ~318, ~404) |
| `src/protocol/server_registry.rs` | registry entry and the `("HTTP3", "http3")` stack-name pair (~364, ~962) |
| `src/protocol/client_registry.rs` | same two spots (~126, ~626) — the *client* keeps doing real HTTP/3, so decide whether it stays `http3` under a `quic` feature flag or gets its own |
| `src/cli/server_startup.rs` | feature-gated match arm |
| `src/server/mod.rs` | module declaration and re-export (owned here, trivial once the feature is renamed) |
| `src/server/http3/` → `src/server/quic/` | directory, event ids, action name `send_http3_data` → `send_quic_data` |
| `tests/server/http3/` | directory and the mocked action/event names |

Because the feature flag is a single gate over both client and server, the
client's `h3`/`h3-quinn` dependency survives the rename unchanged.

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
  only, and `http3` does not declare those startup parameters. Passing them is
  rejected by `StartupParams` as an undeclared key and the server does not start.
  Filter traffic with a script handler instead.
- Unidirectional streams, DATAGRAMs, 0-RTT, connection migration, stream
  priorities
- Binary payloads in either direction (see the encoding asymmetry)
- Unbounded `wait_for_more` accumulation: no size cap, so a hostile peer can grow
  the buffer indefinitely
- A live `stream_count` in `ProtocolConnectionInfo`

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
