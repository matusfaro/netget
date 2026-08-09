# VNC Protocol E2E Tests

## Test Overview

Three tests in `test.rs` driving the VNC server over RFB 3.8 with a custom client (`VncClient`)
implemented in the test file. There is no suitable Rust VNC client library.

## Test Strategy

**A malformed or missing update is a failure, not a note.** `test_vnc_framebuffer_update` used to
swallow both an error and a timeout with a printed "this may be expected", so it could not fail no
matter what the server sent; `test_vnc_input_events` asserted nothing at all.
`read_framebuffer_update` now returns `Err` on any deviation from the wire format — a client that
shrugs at a malformed update cannot tell a working server from a broken one.

**Assert pixels, not byte counts.** The server's fixed pattern is a gradient in BGRX order: blue
constant at 128, red rising with x, green rising with y. Sampling three corners proves the bytes are
the pattern in the right channel order, not merely the right *count* of bytes.

**Framebuffer size comes from ServerInit.** It is fixed at 800x600 in the server and no prompt
changes it, so the tests read the size off the wire rather than assuming one — and the update
rectangle is compared to ServerInit's extent, not to the extent the client requested (RFB lets the
server answer with any rectangle, and this one answers with its whole framebuffer).

**Only Raw encoding.** `read_framebuffer_update` rejects any other encoding rather than guessing at
the payload length, which would desynchronise the stream and produce a confusing later failure.

## LLM Call Budget

**Total: 3** — one startup per test. The RFB handshake, the test-pattern framebuffer, and input
event logging all happen without consulting the model. Each rule is `expect_calls(1)`.

## Test Cases

### 1. `test_vnc_handshake`

Full RFB 3.8 negotiation: server ProtocolVersion starts with `RFB `, security type None (1) is
offered and accepted, SecurityResult is 0, ClientInit/ServerInit yields **800x600**.

### 2. `test_vnc_framebuffer_update`

Requests a full update and requires exactly one Raw rectangle at the origin covering ServerInit's
extent, with exactly 4 bytes per pixel. Then samples the gradient:

- `(0, 0)` → b=128, g=0, r=0
- `(width-1, 0)` → b=128, g=0, r > 250
- `(0, height-1)` → b=128, g > 250, r=0

### 3. `test_vnc_input_events`

Sends KeyEvent press and release for keysym 97, a PointerEvent move, and a click press/release.

- The KeyEvent handler reports on `status_tx` at DEBUG, so the test requires both
  `VNC KeyEvent: down=true, key=97` and `down=false, key=97` in the server output. Absence is a
  regression, not something to note and continue past.
- PointerEvent is `trace!`-only and never reaches that stream, so it is checked **structurally**:
  each RFB client message has a fixed length, so a server that mis-parsed any of the five messages
  would leave the stream misaligned and read the next request's bytes as a message type. Asking for
  another framebuffer afterwards and getting a well-formed update back proves every one of them was
  consumed at exactly the right length.

## Expected Runtime

~1.5s for the whole suite against the mock harness.

## Not Covered

Compressed encodings (Hextile, ZRLE, Tight), incremental updates, SetPixelFormat, SetEncodings,
ClientCutText, Bell, VNC authentication (type 2), and LLM-generated framebuffer content — input
events are logged but never forwarded to the model.

## Manual Testing

```bash
./cargo-isolated.sh run --no-default-features --features vnc --release
# prompt: "listen on port 5900 via vnc"
vncviewer localhost:5900
```

Expect the gradient described above and `VNC KeyEvent:` lines in the status panel.
