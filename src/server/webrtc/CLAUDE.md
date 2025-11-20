# WebRTC Server Implementation

## Overview

WebRTC (Web Real-Time Communication) server implementation for NetGet. This implementation focuses on **data channels only** (no audio/video media), which is well-suited for LLM control. The server enables peer-to-peer text/binary messaging with multiple concurrent peers over WebRTC's reliable data channel transport.

## Architecture

### Core Components

1. **Peer Connection Management**: Manages multiple `RTCPeerConnection` instances (one per peer)
2. **Data Channels**: `RTCDataChannel` provides reliable message transport per peer
3. **ICE/STUN**: Uses Google STUN server for NAT traversal (configurable)
4. **Signaling**: Manual SDP exchange (user pastes offers, server generates answers)

### Protocol Stack

```
Application (LLM-controlled peer messages)
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

1. **`webrtc_peer_connected`** - Triggered when a peer's data channel opens
    - Provides peer_id and channel label
    - LLM can send initial message to new peer

2. **`webrtc_message_received`** - Triggered when message arrives from peer
    - Provides peer_id, message text, and binary flag
    - LLM decides response action

3. **`webrtc_offer_received`** - Triggered when user pastes SDP offer (manual mode)
    - Provides peer_id and sdp_offer
    - LLM can auto-accept or review offers

4. **`webrtc_peer_disconnected`** - Triggered when peer closes connection
    - Provides peer_id and disconnect reason
    - LLM can clean up peer-specific state

### Actions

#### User-Triggered (Async)

- **`accept_offer`** - Accept an SDP offer from peer and generate answer
- **`send_to_peer`** - Send message to specific peer by ID
- **`close_peer`** - Close connection to specific peer
- **`list_peers`** - List all active peer connections

#### Event-Triggered (Sync)

- **`send_message`** - Send reply message
- **`disconnect`** - Close current peer connection
- **`wait_for_more`** - Don't respond yet

### Connection Flow

1. User opens WebRTC server
2. Server displays: "Ready to accept peer connections (paste SDP offers)"
3. User receives SDP offer from peer (via external channel)
4. User pastes SDP offer → `webrtc_offer_received` event fires
5. LLM responds with `accept_offer` action
6. Server generates SDP answer and displays to user
7. User sends SDP answer to peer
8. Connection established, data channel opens
9. `webrtc_peer_connected` event fires
10. Messages exchanged via `send_message` / `send_to_peer` actions
11. `webrtc_message_received` events fire for incoming messages

## State Management

### Peer Tracking

Each peer is tracked with:
- **peer_id**: Unique identifier (user-defined or auto-generated)
- **peer_connection**: Arc<RTCPeerConnection>
- **data_channel**: Arc<RTCDataChannel>
- **connection_id**: ConnectionId for NetGet tracking
- **memory**: Per-peer LLM memory string
- **state**: Connection state machine (Idle/Processing/Accumulating)
- **queued_messages**: Messages queued during LLM processing

### Connection States

- **Idle**: No LLM processing in progress
- **Processing**: LLM call active for current message
- **Accumulating**: Messages queued while LLM processes

### Stored Data (protocol_data)

- `server_data_ptr`: Raw pointer to WebRtcServerData (for action execution)

**Safety Note**: Raw pointer is stored to maintain Arc reference across async boundaries. Pointer is NOT dropped when retrieved for action execution.

## Multi-Peer Support

The server can handle **multiple concurrent peers**:
- Each peer has independent peer_connection and data_channel
- Each peer has separate LLM memory (per-peer context)
- Each peer has independent state machine (no cross-peer blocking)
- LLM sees peer_id in events to distinguish messages

**Example Multi-Peer Scenario**:
```
Peer A connects → peer_id "alice"
Peer B connects → peer_id "bob"

[EVENT] webrtc_message_received from "alice": "Hello"
[ACTION] send_message to "alice": "Hi Alice!"

[EVENT] webrtc_message_received from "bob": "Hi"
[ACTION] send_message to "bob": "Hello Bob!"
```

## Signaling Strategy

**Current**: Manual SDP exchange

1. Peer generates SDP offer
2. User pastes offer to NetGet server
3. Server generates SDP answer
4. User copies answer and sends to peer
5. Peer applies answer, connection established

**Future Enhancements**:
- WebSocket-based signaling server (automatic SDP relay)
- Integration with WebRTC Signaling Server protocol (separate implementation)
- Support for custom signaling backends

## Limitations

1. **No Media**: Audio/video not supported (data channels only)
2. **Manual Signaling**: Requires user to exchange SDP manually (UX friction)
3. **Text Focus**: Binary data sent as text (UTF-8), hex encoding for true binary
4. **No Renegotiation**: Connection parameters fixed at offer/answer time
5. **Basic ICE**: Only Google STUN by default, no custom TURN servers yet
6. **No Trickle ICE**: ICE candidates gathered before SDP answer (simpler but slower)

## Security Considerations

- **DTLS Encryption**: All data encrypted by default (WebRTC requirement)
- **Peer Authentication**: No built-in peer verification (trust SDP source)
- **ICE Candidate Privacy**: Server's local IP addresses exposed in SDP answer
- **STUN Server**: Trusts Google STUN (can be configured to use custom servers)
- **DoS Risk**: Malicious peers can send many offers (rate limiting recommended)

## Testing Strategy

See `tests/server/webrtc/CLAUDE.md` for testing details.

Key challenges:
- Requires two peers (NetGet client + browser or two NetGet instances)
- Manual SDP exchange for E2E tests (automated in test framework)
- Mock mode simulates peer connections without real WebRTC

## Performance Considerations

- **Memory**: Each peer consumes ~1-2 MB (RTCPeerConnection overhead)
- **CPU**: DTLS handshake is CPU-intensive (once per peer connection)
- **Connections**: Recommended limit: 50-100 concurrent peers
- **Bandwidth**: Data channels are reliable (SCTP), similar to TCP

## Future Enhancements

1. **Automatic Signaling**: WebSocket-based signaling server integration
2. **Multiple Channels**: Support multiple data channels per peer
3. **Binary Protocol**: Efficient binary message encoding (Protobuf, MessagePack)
4. **Custom TURN**: Add custom TURN servers for relay
5. **ICE Restart**: Support connection renegotiation
6. **Connection Stats**: Expose RTCStats for monitoring (bandwidth, RTT, packet loss)
7. **Peer Groups**: Group peers by topic/channel for broadcast messaging
8. **Authentication**: Token-based peer authentication before accepting offers

## Example Usage

```bash
# Open WebRTC server
> open_server webrtc 0.0.0.0:0 "WebRTC peer server accepting connections"

# Server displays: "Ready to accept peer connections (paste SDP offers)"

# User receives SDP offer from peer and pastes it
# (Imagine user typed: paste_offer peer-alice <SDP JSON>)

# LLM detects offer event
> [EVENT] webrtc_offer_received (peer_id: "peer-alice", sdp_offer: "{...}")

# LLM responds with accept_offer action
> [ACTION] accept_offer (peer_id: "peer-alice")

# Server generates SDP answer and displays
> SDP Answer (send to peer-alice):
> {
>   "type": "answer",
>   "sdp": "v=0\r\no=- ... [full SDP]"
> }

# User sends answer to peer, connection established
> [EVENT] webrtc_peer_connected (peer_id: "peer-alice", channel_label: "netget")

# Peer sends message
> [EVENT] webrtc_message_received (peer_id: "peer-alice", message: "Hello server!")

# LLM responds
> [ACTION] send_message (message: "Hello peer-alice! Welcome to NetGet.")

# Another peer connects
> [EVENT] webrtc_offer_received (peer_id: "peer-bob", sdp_offer: "{...}")
> [ACTION] accept_offer (peer_id: "peer-bob")
> [EVENT] webrtc_peer_connected (peer_id: "peer-bob")

# LLM can send to specific peer
> [ACTION] send_to_peer (peer_id: "peer-alice", message: "Bob just connected!")

# List all peers
> [ACTION] list_peers
> Active WebRTC peers: ["peer-alice", "peer-bob"]
```

## Integration with WebRTC Signaling Server

Once the WebRTC Signaling Server protocol is implemented, the server can operate in two modes:

**Manual Mode** (default):
- User pastes SDP offers
- Server displays SDP answers
- User manually relays signaling messages

**Automatic Mode** (with signaling server):
- Server registers with signaling server
- Signaling server forwards offers automatically
- Server sends answers to signaling server
- Server receives answers from signaling server
- No user intervention needed

## References

- [webrtc-rs Documentation](https://docs.rs/webrtc/latest/webrtc/)
- [webrtc-rs GitHub](https://github.com/webrtc-rs/webrtc)
- [WebRTC Data Channels MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Using_data_channels)
- [WebRTC Signaling MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Signaling_and_video_calling)
- [WebRTC Security](https://webrtc-security.github.io/)
