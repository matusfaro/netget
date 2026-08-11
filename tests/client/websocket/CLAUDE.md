# WebSocket Client E2E Tests

## Strategy: a hand-written server as the peer

The client is built on `tokio-tungstenite`, so testing it against the NetGet WebSocket server
would test that crate against itself. Instead `e2e_test.rs` contains a **hand-written WebSocket
server** (~180 lines): it parses the upgrade request itself, recomputes `Sec-WebSocket-Accept`
from the RFC algorithm with `sha1` + `base64` directly, and reads the client's frames byte by
byte.

The byte-level reading is the point. A WebSocket *client* can violate RFC 6455 §5.3 — "every
client-to-server frame MUST be masked with a fresh 32-bit key" — completely invisibly, because
plenty of servers accept unmasked frames anyway. Only a peer that inspects the mask bit will ever
notice. `Observed.all_frames_masked` is that assertion.

## The one test

`test_websocket_client_against_hand_written_server` asserts, in order:

1. the request is `GET … HTTP/1.1` with `Upgrade: websocket`, `Connection: Upgrade` and
   `Sec-WebSocket-Version: 13`
2. `Sec-WebSocket-Key` decodes to **exactly 16 bytes** of base64 (§4.1)
3. the `path` startup parameter reaches the request line (`/ws`)
4. the `subprotocols` startup parameter reaches `Sec-WebSocket-Protocol` **in order**
   (`chat, superchat`)
5. **every** client frame carries the mask bit
6. the client sends the text frame the `websocket_client_connected` handler asked for
7. **the binary round-trip**: the server sends `00 ff fe 01 80 7f c3 28` (not valid UTF-8, not
   printable); the mock feeds the event's own `data` and `encoding` straight back into
   `send_websocket_binary`; the bytes that come back are compared byte-for-byte

Step 7 uses `respond_with_actions_from_event`, which is the only way to prove symmetry — a static
mock would hardcode the encoded form and could pass while the two directions disagreed.

The server then sends a close frame with code 1000 so `websocket_client_closed` fires.

## LLM call budget: 4

`open_client`, `websocket_client_connected`, `websocket_client_binary_message`, and
`websocket_client_closed` (declared `expect_at_least(0)` — the process may be torn down before
the close event is processed, and that is not what this test is about).

The test ends with `client.verify_mocks().await?`.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features websocket \
    --test client websocket -- --test-threads=100
```

Expected runtime: ~1.5s. The hand-written server binds 127.0.0.1:0; nothing external is
contacted.

## Known gaps

- `wss://` is not testable — the client has no TLS backend by design.
- The `headers` startup parameter (and the ignore-list for handshake headers) is not covered.
- Fragmented **inbound** messages are not exercised on the client side, only the server side.
- `wait_for_websocket_data` on the client has no assertion.
