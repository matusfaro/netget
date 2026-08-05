# VNC Protocol Implementation

**Status**: `DevelopmentState::Incomplete` — hidden from the LLM by
`ProtocolMetadataV2::is_available_to_llm()`.

RFB 3.8 (RFC 6143) over TCP, default port 5900 — not privileged, so
`privilege_requirement` is `None`. The protocol handshake is real and works against real
viewers. Nothing above the handshake is LLM-controlled.

## Why it is Incomplete

`VncServer::spawn_with_llm_actions` takes the `OllamaClient` as `_llm_client` and drops it.
Grep the directory: no `Event::new`, no `EventType`, no `call_llm`. `get_event_types()` is not
implemented, so it returns the empty trait default.

Every `FramebufferUpdateRequest` is answered by `send_test_framebuffer` with a hardcoded
red/green gradient. Ask for "a blue background with white text saying Hello" and you get the
gradient. Key events, pointer events and clipboard text are logged at DEBUG/TRACE and dropped.

This is the same shape as the TURN demotion: the transport works, the feature does not. A user
reading `Experimental` alongside `llm_control: "Framebuffer content, authentication, input
events"` — the previous metadata — would reasonably expect the model to be driving the
display. It never was.

## Actions

**None.** `get_async_actions()` and `get_sync_actions()` both return empty, and
`execute_action()` returns an error for every input.

Five actions used to be declared: `vnc_auth_success`, `vnc_auth_deny`, `vnc_render_display`,
`send_framebuffer_update`, `disconnect_vnc_client`. All were unreachable — no event advertised
them (there are no events) and the connection loop never calls `execute_action` — and three of
the five returned `NoAction` even if they had been reached. `vnc_render_display` advertised
"Render display content in response to update request" and did nothing at all.

## Events

**None.** Four `&'static str` constants (`VNC_AUTH_REQUEST_EVENT`, `VNC_UPDATE_REQUEST_EVENT`,
`VNC_KEY_EVENT`, `VNC_POINTER_EVENT`) used to sit at the top of `actions.rs`. They were plain
strings, not `EventType`s, and nothing referenced them. Removed.

## What works

- **ProtocolVersion / Security / SecurityResult / ClientInit / ServerInit** — full RFB 3.8
  handshake, verified by the custom client in `tests/server/vnc/test.rs` and by real viewers.
- **Security type "None" (1) only.** Any client that connects is admitted. There is no
  VNC-Auth (type 2) and no password. `update_vnc_connection_auth(..., true, None)` is called
  unconditionally after the handshake — it records "authenticated", it does not decide it.
- **FramebufferUpdate, Raw encoding** — correct message framing, 32-bit BGRX pixels matching
  the announced pixel format.
- **Connection accounting** — one `ConnectionState` per connection, removed on disconnect.
- The accept loop's `JoinHandle` is registered via `AppState::register_server_task()`, so
  `stop_server` releases the socket.

## The rendering half that exists

`crate::display` (`src/display/`) is complete and protocol-agnostic: `DisplayCanvas::new(w, h)`
plus `add_commands(Vec<DisplayCommand>)` plus `render()` produces an RGBA buffer via tiny-skia,
and `DisplayCommand` is an externally-tagged serde enum (`{"DrawText": {...}}`) that
deserializes straight from LLM JSON. `VncServer::send_framebuffer_update` already renders such
a buffer and ships it as a Raw rectangle.

**It has no caller.** That is the whole gap.

## Making it real

1. Declare an `EventType` for `vnc_framebuffer_update_request` carrying `incremental`, `x`,
   `y`, `width`, `height`, and — critically — attach the render action with
   `.with_actions(vec![VNC_RENDER_DISPLAY_ACTION.clone()])`. Without that the model is handed
   no protocol vocabulary and can only fail (`EventType::has_no_usable_actions`).
2. In the message-type-3 arm of `message_loop`, build the event and `call_llm(...)` so script
   and static handlers get their chance before the model does.
3. Deserialize the returned `commands` array into `Vec<DisplayCommand>` and call
   `send_framebuffer_update`. Keep `send_test_framebuffer` as the fallback when the LLM call
   fails, so a viewer never hangs waiting for a frame that will not come.
4. Optionally raise `vnc_key_event` / `vnc_pointer_event` as informational events declared with
   `.with_no_actions()`.

**Note before doing this**: `tests/server/vnc/test.rs` currently asserts
`expect_calls(1)` on a startup-instruction mock and documents "FramebufferUpdateRequest: 0 LLM
calls". Adding a per-request LLM call changes that budget, so the test and
`tests/server/vnc/CLAUDE.md` must be updated in the same change.

## Fixed in this pass

- **Framebuffer write was one awaited `write_u8` per byte** — 1,920,000 awaited writes per
  800x600 frame, each a syscall on an unbuffered socket, all while holding the write lock. The
  frame is now serialized into one `Vec<u8>` and written with a single `write_all`.
- **The requested region was echoed back as the rectangle size.** A client could request a
  65535x65535 region and the server would try to produce it, contradicting the 800x600 it had
  announced in ServerInit. The full announced framebuffer is now always sent (RFB permits the
  server to choose the rectangle), via the new `FRAMEBUFFER_WIDTH`/`FRAMEBUFFER_HEIGHT`
  constants.
- **ClientCutText allocated from a client-controlled u32.** `vec![0u8; length as usize]` ran
  before any payload was read, so a 7-byte message could request a 4 GiB allocation. Capped at
  `MAX_CUT_TEXT_LEN` (1 MiB); over the cap the connection is closed.
- **The accept loop spun on error.** `Err` logged and continued, so a persistent accept failure
  (EMFILE, socket closed underneath) burned a core forever. It now breaks, like every other
  TCP protocol here.
- **Startup is honest.** A WARN goes to both the log and the status channel saying the
  instruction is ignored and the display is a fixed test pattern.

## Limitations

- No LLM control of anything (above).
- Framebuffer fixed at 800x600; no startup parameter, no resize, no `DesktopSize`
  pseudo-encoding.
- `SetPixelFormat` is parsed and ignored — the server always sends 32bpp BGRX, so a client
  that negotiates 8- or 16-bit colour renders garbage.
- `SetEncodings` is parsed and ignored — Raw only. No CopyRect, RRE, Hextile, ZRLE or Tight,
  and no compression, so every frame is width x height x 4 bytes on the wire.
- No incremental updates: `incremental=1` gets the same full frame.
- No VNC-Auth, no TLS, no VeNCrypt.
- Clipboard is one-way and discarded; no `ServerCutText`.
- Each connection is independent; there is no shared framebuffer and the shared-flag in
  ClientInit is read and ignored.

## Manual verification

```bash
./cargo-isolated.sh run --no-default-features --features vnc --release
# then: "listen on port 5900 via vnc"
vncviewer localhost:5900     # or Screen Sharing on macOS, TigerVNC, RealVNC
```

Expect: connection succeeds without a password, an 800x600 window, a gradient (red increasing
left-to-right, green top-to-bottom, blue constant at 128). Keystrokes and mouse movement appear
in `netget.log` at DEBUG/TRACE and change nothing on screen. That is the whole feature set.

## References

- [RFC 6143 — The Remote Framebuffer Protocol](https://tools.ietf.org/html/rfc6143)
- [rfbproto](https://github.com/rfbproto/rfbproto)
