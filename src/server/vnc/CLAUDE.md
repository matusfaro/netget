# VNC Protocol Implementation

## Overview

VNC (Virtual Network Computing) implements the RFB (Remote Frame Buffer) protocol for remote desktop access. This is an
Alpha-status implementation where the LLM controls display content generation, authentication decisions, and input event
handling.

**Protocol Compliance**: RFB Protocol 3.8 (RFC 6143)
**Transport**: TCP (port 5900 default)
**Status**: Alpha - Basic RFB handshake and framebuffer updates

## Library Choices

### Display Rendering

- **Custom display module** (`src/display/`) - Canvas-based rendering with DisplayCommand API
- **image** crate - Pixel buffer manipulation (RGB888 format)
- **imageproc** - Drawing primitives (rectangles, text, lines)

**Rationale**: No VNC server library exists in Rust that allows LLM control of display content. Custom display system
allows LLM to specify drawing commands (rectangles, text, etc.) that are rendered to pixel buffer.

### Protocol Implementation

- **Manual RFB parsing** - Custom implementation of protocol handshake
- **tokio::io** - AsyncReadExt/AsyncWriteExt for binary protocol I/O
- **Manual pixel format encoding** - RGB888 (32-bit true color)

**Rationale**: VNC protocol is well-documented and relatively simple. Manual implementation allows full control over
authentication, pixel formats, and encoding types.

### No External VNC Library

- **No vnc-rs or similar** - Existing libraries are client-focused or incomplete

**Manual Implementation Components**:

- Protocol version negotiation (RFB 003.008)
- Security type selection (None authentication)
- ClientInit / ServerInit exchange
- Pixel format specification (16-byte structure)
- Framebuffer update protocol
- Input event handling (keyboard, mouse)

## Architecture Decisions

### 1. LLM-Controlled Display Generation

**Design Philosophy**: LLM generates visual content using high-level drawing commands instead of raw pixel data.

**DisplayCommand API**:

```rust
pub enum DisplayCommand {
    FillRect { x, y, width, height, color },
    DrawText { x, y, text, color, size },
    DrawLine { x1, y1, x2, y2, color, thickness },
    DrawCircle { x, y, radius, color, filled },
    SetBackground { color },
}
```

**Rendering Pipeline**:

1. LLM returns actions with `Vec<DisplayCommand>`
2. `DisplayCanvas::new(width, height)` creates pixel buffer
3. `canvas.add_commands(commands)` queues drawing operations
4. `canvas.render()` executes commands → RGB888 image buffer
5. Image buffer sent as Raw encoding in FramebufferUpdate message

**Benefits**:

- LLM doesn't need to understand pixel formats
- High-level API (rectangles, text, not pixels)
- Flexible content generation
- Easy to extend with new drawing primitives

### 2. RFB Protocol Handshake

**Handshake Flow** (RFC 6143 section 7.1):

1. **ProtocolVersion**: Server sends "RFB 003.008\n", client echoes back
2. **SecurityTypes**: Server offers [1] (None = no authentication), client chooses
3. **SecurityResult**: Server sends 0 (OK) for None security
4. **ClientInit**: Client sends shared-flag (0 = exclusive, 1 = shared)
5. **ServerInit**: Server sends framebuffer dimensions, pixel format, name

**Security**: Currently only "None" authentication (no password). VNC-Auth (DES challenge-response) not yet implemented.

**Pixel Format**: 32-bit RGB888 (bits_per_pixel=32, depth=24, true_color=1)

- Red: 8 bits at shift 16
- Green: 8 bits at shift 8
- Blue: 8 bits at shift 0
- Little-endian byte order

### 3. Framebuffer Update Protocol

**Client Request**:

- Message type: 3 (FramebufferUpdateRequest)
- incremental: 0 (full update) or 1 (incremental)
- x, y, width, height: Region to update

**Server Response**:

- Message type: 0 (FramebufferUpdate)
- Number of rectangles: u16
- For each rectangle:
    - x, y, width, height: u16
    - encoding-type: i32 (0 = Raw)
    - pixel-data: width × height × 4 bytes (RGB888)

**Raw Encoding**: Uncompressed pixel data (BGRX byte order for 32-bit RGB)

**Future Encodings** (not implemented):

- 1 = CopyRect (copy from another region)
- 2 = RRE (Rise-and-Run-length Encoding)
- 5 = Hextile (16×16 tile encoding)
- 16 = ZRLE (Zlib Run-length Encoding)

### 4. Input Event Handling

**KeyEvent** (message type 4):

- down-flag: 1 (key press) or 0 (key release)
- padding: 2 bytes
- key: u32 (X11 keysym)

**PointerEvent** (message type 5):

- button-mask: u8 (bit flags for buttons 1-8)
- x-position: u16
- y-position: u16

**Current Behavior**: Events logged to status channel, not yet sent to LLM

**Future LLM Integration**: Could send events to LLM for dynamic display updates

### 5. Connection Lifecycle

**Single Connection Pattern**:

1. Accept TCP connection
2. Perform RFB handshake
3. Send ServerInit
4. Enter message loop (handle SetPixelFormat, SetEncodings, FramebufferUpdateRequest, KeyEvent, PointerEvent)
5. Connection closes when client disconnects

**No Multi-Client Support**: Each connection is independent (no shared framebuffer)

**Stateless Display**: Framebuffer generated on-demand (test pattern fallback, LLM generation planned)

## LLM Integration

**Current Status**: Limited LLM integration

**Implemented**:

- LLM receives server startup event (could set display policy)

**Not Yet Implemented**:

- LLM-generated framebuffer content (DisplayCommand actions)
- LLM handling of KeyEvent and PointerEvent
- Dynamic display updates based on user input

**Planned Action System**:

```json
{
  "actions": [
    {
      "type": "update_framebuffer",
      "commands": [
        {"type": "fill_rect", "x": 0, "y": 0, "width": 800, "height": 600, "color": "#000000"},
        {"type": "draw_text", "x": 100, "y": 100, "text": "Hello VNC", "color": "#FFFFFF", "size": 48}
      ]
    }
  ]
}
```

**Event Types** (planned):

- `VNC_FRAMEBUFFER_REQUEST_EVENT` - Client requests display update
- `VNC_KEY_EVENT` - Client presses key
- `VNC_POINTER_EVENT` - Client moves mouse or clicks

**Scripting**: Not applicable - display content is dynamic and per-client

## Connection Management

**Connection State** (tracked in AppState):

```rust
ProtocolConnectionInfo::Vnc {
    write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
    state: ProtocolState::Idle | Processing | Accumulating,
    queued_data: Vec<Vec<u8>>,
    authenticated: bool,
    username: Option<String>,
    framebuffer_width: u16,
    framebuffer_height: u16,
    pixel_format: VncPixelFormat,
}
```

**Connection Tracking**:

- Connection ID, remote address, local address
- Bytes sent/received (framebuffer updates are large)
- Packets sent/received (each framebuffer update is a "packet")
- Connection status (Active/Closed)

**Write Half Storage**: Arc<Mutex<WriteHalf>> allows sending framebuffer updates from event handlers

## Limitations

### Not Implemented

1. **VNC Authentication** - SecurityType 2 (DES challenge-response)
2. **Compressed Encodings** - Hextile, ZRLE, Tight encoding
3. **SetPixelFormat handling** - Server ignores client's preferred format
4. **Clipboard sync** - ClientCutText not forwarded anywhere
5. **Resize events** - Framebuffer size fixed at startup
6. **Multi-client shared mode** - Each client gets independent display
7. **LLM framebuffer generation** - Currently uses test pattern fallback
8. **Input event LLM forwarding** - KeyEvent/PointerEvent not sent to LLM

### Current Capabilities

- RFB 3.8 protocol handshake
- No-authentication mode
- Raw encoding framebuffer updates
- Test pattern display generation
- Input event logging
- Connection tracking in UI

### Known Issues

- Test pattern only (no LLM-generated content yet)
- Large framebuffer updates (no compression)
- Single-threaded framebuffer rendering
- No incremental updates (always sends full frame)

## Example Prompts

### Start a VNC server

```
listen on port 5900 via vnc. Accept all connections without authentication. Use 800x600 framebuffer.
```

### VNC with custom display

```
listen on port 5900 via vnc. Show a blue background with white text saying "NetGet VNC Server" in the center.
```

### Interactive VNC (future)

```
listen on port 5900 via vnc. When user clicks the mouse, draw a red circle at that position.
```

## References

- [RFC 6143 - The Remote Framebuffer Protocol](https://tools.ietf.org/html/rfc6143)
- [RealVNC Protocol Documentation](https://github.com/rfbproto/rfbproto)
- [VNC Protocol Specification](https://www.rfc-editor.org/rfc/rfc6143.html)
- [X11 Keysyms](https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html)

## Implementation Statistics

| Module               | Lines of Code | Purpose                                          |
|----------------------|---------------|--------------------------------------------------|
| `mod.rs`             | 486           | RFB handshake, message loop, framebuffer updates |
| `actions.rs`         | ~100          | Action definitions (minimal currently)           |
| `src/display/mod.rs` | ~200          | DisplayCanvas, DisplayCommand rendering          |
| **Total**            | **~800**      | Basic VNC server implementation                  |

This is an Alpha implementation with working RFB protocol and test pattern display. Future work includes full LLM
integration for dynamic content generation and input handling.
