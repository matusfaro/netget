# WebRTC Client Implementation

## Overview

WebRTC (Web Real-Time Communication) client implementation for NetGet. This implementation focuses on **data channels only** (no audio/video media), which is well-suited for LLM control. The client enables peer-to-peer text/binary messaging over WebRTC's reliable data channel transport.

## Architecture

### Core Components

1. **Peer Connection**: `RTCPeerConnection` manages the P2P connection
2. **Data Channel**: `RTCDataChannel` provides reliable message transport
3. **ICE/STUN**: Uses Google STUN server for NAT traversal
4. **Signaling**: Manual SDP exchange (user pastes offer/answer)

### Protocol Stack

```
Application (LLM-controlled messages)
    ↓
DataChannel (SCTP)
    ↓
DTLS (encryption)
    ↓
ICE/STUN (NAT traversal)
    ↓
UDP
```

## Library Choices

### webrtc-rs (v0.11)

**Rationale**: Pure Rust implementation of WebRTC stack

**Pros**:
- Complete WebRTC implementation (rewrite of Pion in Rust)
- Data channel support without media complexity
- Good API for creating/managing peer connections
- Active development

**Cons**:
- Relatively complex setup (MediaEngine, InterceptorRegistry)
- Manual SDP exchange required (no built-in signaling server)
- Still in early development (v0.11)

**Alternative Considered**: `datachannel-rs` (libdatachannel C++ wrapper) - rejected due to FFI complexity and external dependency

## LLM Integration

### Events

1. **`webrtc_connected`** - Triggered when data channel opens
   - Provides channel label
   - LLM can send initial message

2. **`webrtc_message_received`** - Triggered when message arrives
   - Provides message text and binary flag
   - LLM decides response action

### Actions

#### User-Triggered (Async)
- **`send_message`** - Send text message over data channel
- **`apply_answer`** - Apply remote SDP answer to complete connection
- **`disconnect`** - Close data channel and peer connection

#### Event-Triggered (Sync)
- **`send_message`** - Send reply message
- **`disconnect`** - Close connection
- **`wait_for_more`** - Don't respond yet

### Connection Flow

1. User opens WebRTC client
2. Client creates offer and generates SDP
3. SDP offer displayed to user (to send to peer)
4. User pastes SDP answer from peer
5. LLM applies answer via `apply_answer` action
6. Connection established, data channel opens
7. `webrtc_connected` event fires
8. Messages exchanged via `send_message` actions
9. `webrtc_message_received` events fire for incoming messages

## State Management

### Connection States
- **Idle**: No LLM processing in progress
- **Processing**: LLM call active for current message
- **Accumulating**: Messages queued while LLM processes

### Stored Data (protocol_data)
- `sdp_offer`: Generated SDP offer (JSON)
- `peer_connection_ptr`: Raw pointer to RTCPeerConnection (for lifecycle)
- `data_channel_ptr`: Raw pointer to RTCDataChannel (for sending)

**Safety Note**: Raw pointers are stored to maintain Arc references across async boundaries. Pointers are cleaned up when client is removed.

## Signaling Strategy

**Current**: Manual SDP exchange
- User copies SDP offer from NetGet
- User pastes SDP offer to peer (web browser, another NetGet instance, etc.)
- Peer generates SDP answer
- User pastes SDP answer back to NetGet
- Connection completes

**Future**: Could add WebSocket-based signaling server support

## Limitations

1. **No Media**: Audio/video not supported (data channels only)
2. **Manual Signaling**: Requires user to exchange SDP manually
3. **Single Data Channel**: Only one "netget" channel created
4. **Text Focus**: Binary data sent as text (UTF-8)
5. **No Renegotiation**: Connection parameters fixed at offer time
6. **Basic ICE**: Only Google STUN, no custom TURN servers

## Security Considerations

- **DTLS Encryption**: All data encrypted by default (WebRTC requirement)
- **Peer Authentication**: No built-in peer verification (trust SDP source)
- **ICE Candidate Privacy**: Local IP addresses exposed in SDP
- **STUN Server**: Trusts Google STUN (could add custom servers)

## Testing Strategy

See `tests/client/webrtc/CLAUDE.md` for testing details.

Key challenges:
- Requires two peers (NetGet + browser or two NetGet instances)
- Manual SDP exchange for E2E tests
- Could use loopback connections for automated testing

## Future Enhancements

1. **Signaling Server**: Add WebSocket-based signaling
2. **Multiple Channels**: Support multiple data channels
3. **Binary Protocol**: Efficient binary message encoding
4. **TURN Support**: Add custom TURN servers for relay
5. **ICE Restart**: Support connection renegotiation
6. **Connection Stats**: Expose RTCStats for monitoring

## Example Usage

```bash
# Open WebRTC client (generates SDP offer)
> open_client webrtc peer "Send hello message"

# SDP offer displayed - user exchanges with peer

# After peer responds, apply answer
> (LLM generates apply_answer action with pasted SDP)

# Connection established, send message
> (LLM sends message via send_message action)

# Receive messages
> (webrtc_message_received events processed by LLM)
```

## References

- [webrtc-rs Documentation](https://docs.rs/webrtc/latest/webrtc/)
- [webrtc-rs GitHub](https://github.com/webrtc-rs/webrtc)
- [WebRTC Data Channels MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Using_data_channels)
- [WebRTC Signaling MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Signaling_and_video_calling)
