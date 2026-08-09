# QUIC Protocol E2E Tests

## Test Overview

Tests the QUIC server (`src/server/quic/`) — raw bidirectional QUIC streams under
TLS 1.3, **not** HTTP/3. Validates that the LLM can drive stream multiplexing and
per-stream data handling over an encrypted QUIC transport.

Because the server implements no RFC 9114 framing, these tests speak raw QUIC via
`quinn::Endpoint`. That is why they pass despite the absence of HTTP/3 framing —
and it is also the coverage gap: **nothing in the suite exercises a real HTTP/3
client**, because nothing could.

## Test Strategy

- **Isolated test servers**: each test spawns a separate NetGet instance with its
  own mocked LLM
- **Quinn client**: `quinn::Endpoint` for the client side, ALPN `h3`
- **Self-signed certificates**: server generates a self-signed cert, client uses
  `SkipServerVerification`
- **Stream-focused**: individual stream operations and multiplexing
- **Fast validation**: 10-second timeout per operation, 15 seconds for concurrent
  streams

## LLM Call Budget

- `test_quic_echo()`: 3 mocked calls (startup + connection opened + stream data)
- `test_quic_custom_response()`: 3 mocked calls (startup + connection opened +
  PING)
- `test_quic_multiple_streams()`: 5 mocked calls (startup + connection opened +
  3 concurrent streams)
- Comfortably under the ~10-call-per-suite guideline

Each test starts its own server for clean state; consolidating onto one server
would save a few seconds at the cost of isolation.

## Mock Expectations

Ordering matters: the mock system uses the **first** matching rule, so
event-specific rules come first and the startup catch-all
(`on_custom(|ctx| !ctx.instruction.contains("Event ID:"))`) comes last.

| Event | Response |
|---|---|
| `quic_connection_opened` | `message` only — no stream exists yet, so no send action is available |
| `quic_stream_opened` | `message` only |
| `quic_data_received` | `send_quic_data` (plus `close_this_stream` in the PING test) |

The multiple-streams test uses `respond_with_actions_from_event` to echo each
stream's own payload back, rather than a static string, so the three concurrent
streams are distinguishable.

Startup uses `"base_stack": "QUIC"`, which `ServerRegistry::resolve` matches as
the exact canonical protocol name. `"quic"` also resolves, via `keywords()`.
`"http3"` deliberately does **not** — see `src/server/quic/CLAUDE.md`.

Every test ends with `server.verify_mocks().await?`.

## Scripting Usage

Scripting disabled — action-based responses only. These scenarios are simple echo
and command/response patterns that action responses cover directly.

## Client Library

- **quinn v0.11** — same library as the server, so stream semantics match
- **rustls** — TLS 1.3, mandatory for QUIC
- **webpki-roots** — root store the test builds before replacing the verifier

All three come from the `quic` feature; the tests need no `h3`/`h3-quinn`.

## Expected Runtime

Mocked: a few seconds per test (server startup 1-2s + QUIC handshake + immediate
mock responses). Against a real model (`--use-ollama`), roughly 25-30s per test.

## Test Cases

### 1. Echo (`test_quic_echo`)

- **Client**: opens a bidirectional stream, sends `Hello, QUIC!`
- **Expected**: exact echo of sent bytes
- **Purpose**: basic stream round-trip and LLM data handling

### 2. Custom Response (`test_quic_custom_response`)

- **Client**: opens a stream, sends `PING`
- **Expected**: `PONG`, then the stream is closed by the server
- **Purpose**: command parsing plus `close_this_stream`

### 3. Multiple Streams (`test_quic_multiple_streams`)

- **Client**: opens 3 bidirectional streams concurrently, distinct payload on
  each
- **Expected**: each stream receives an echo of its own data
- **Purpose**: stream multiplexing and concurrent per-stream state machines

## Known Issues

### 1. TLS handshake timing

QUIC requires the TLS 1.3 handshake before any data. Under load the 10-second
timeout is the first thing to trip. Re-run before treating it as a failure.

### 2. Certificate verification skip

Tests use `SkipServerVerification`, a `ServerCertVerifier` that always returns
`Ok(...)`. Test-only; real clients should validate.

### 3. ALPN must match

The server forces `alpn_protocols = ["h3"]`. The client must set the same or the
connection fails. The `h3` ALPN value here is historical and does **not** imply
HTTP/3 framing.

### 4. `send.finish()` is required

Without it the receiver waits indefinitely. Always `write_all()` then `finish()`.

### 5. Sequential LLM processing

With `--ollama-lock` (default in tests) concurrent streams are processed
sequentially. Correctness is unaffected; the 15-second timeout in test 3 exists
to absorb it.

## Coverage Gaps

1. **Real HTTP/3 client** — impossible against this server by design
2. **Unidirectional streams** — only bidirectional are tested
3. **Connection migration**, **0-RTT**, **DATAGRAMs** — untested
4. **Flow control** and stream priorities — untested
5. **Large payloads** (multi-MB) — untested
6. **Half-close semantics** — no explicit test
7. **Binary payloads** — untestable end-to-end: inbound non-printable data
   arrives hex-encoded but `send_quic_data` writes verbatim, so it cannot be
   echoed back (see the encoding asymmetry in `src/server/quic/CLAUDE.md`)
8. **`wait_for_more`** — declared, never exercised
9. **`stream_count`** in `ProtocolConnectionInfo` — known to be permanently 0,
   so there is nothing to assert yet

## Debug Tips

**Connection timeouts**: confirm the server logged `QUIC server listening`, check
the port is free, raise the timeout on a loaded machine.

**TLS errors**: verify ALPN is `h3`, that `SkipServerVerification` is installed,
and that the rustls crypto provider was installed
(`rustls::crypto::ring::default_provider().install_default()`).

**Stream errors**: verify `send.finish()` is called, look for LLM errors in the
server log, and confirm the mock emits `send_quic_data` (not the old
`send_http3_data`).

## References

- [RFC 9000: QUIC Transport](https://datatracker.ietf.org/doc/html/rfc9000)
- [RFC 9001: QUIC TLS](https://datatracker.ietf.org/doc/html/rfc9001)
- [Quinn Documentation](https://docs.rs/quinn/)
