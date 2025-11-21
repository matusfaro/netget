# WebRTC Client Implementation

## Overview

WebRTC (Web Real-Time Communication) client implementation for NetGet with **enhanced features**:
- **Data channels only** (no audio/video media)
- **WebSocket signaling** for automatic SDP exchange
- **Multi-channel support** for parallel data streams
- **Binary data support** with hex encoding/decoding

This implementation is well-suited for LLM control, enabling peer-to-peer messaging with automatic or manual connection setup.

## Enhancements (v2)

### 1. WebSocket Signaling Support

**Purpose**: Automatic SDP exchange via signaling server (no manual copy-paste)

**Usage**:
```bash
# Manual mode (original - user exchanges SDP)
> open_client webrtc manual "Send hello message"

# WebSocket mode (automatic - connects to signaling server)
> open_client webrtc ws://localhost:8080/alice "Send hello to bob"
```

**How it works**:
- Client connects to WebSocket signaling server
- Registers with unique peer ID (from URL path)
- Automatically sends SDP offer and receives answer
- No user intervention required for connection setup

**Events**:
- `webrtc_signaling_connected` - Triggered when connected to signaling server
- Provides `peer_id` and `server_url` to LLM

### 2. Multi-Channel Support

**Purpose**: Create multiple independent data channels per connection

**Benefits**:
- Separate streams for different data types (control, file transfer, chat, etc.)
- Per-channel state machines prevent crosstalk
- Isolated message queuing per channel

**Usage**:
```json
{
  "type": "create_channel",
  "channel_label": "file-transfer"
}
```

**Events**:
- `webrtc_channel_opened` - Triggered for each channel that opens
- `webrtc_message_received` - Includes `channel_label` to identify source

### 3. Binary Data Support

**Purpose**: Send true binary data (not just UTF-8 text)

**Encoding**: Hex encoding for binary payloads (like TCP client)

**Usage**:
```json
// Send text
{
  "type": "send_message",
  "message": "Hello, peer!"
}

// Send binary (hex-encoded)
{
  "type": "send_message",
  "message": "hex:48656c6c6f"  // "Hello" in hex
}

// Or use dedicated action
{
  "type": "send_binary",
  "hex_data": "48656c6c6f"
}
```

**Auto-detection**: Incoming messages are auto-detected as binary or text based on ASCII content.

## Architecture

### Core Components

1. **Peer Connection**: `RTCPeerConnection` manages the P2P connection
2. **Data Channels**: Multiple `RTCDataChannel` instances for parallel streams
3. **ICE/STUN**: Uses Google STUN server for NAT traversal
4. **Signaling**: Manual SDP exchange OR WebSocket-based automatic signaling
5. **Per-Channel State**: Independent state machines per channel (Idle/Processing/Accumulating)

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

### Signaling Modes

#### Manual Mode (Original)

```
1. User opens client
2. Client generates SDP offer
3. User copies offer to peer
4. Peer generates answer
5. User pastes answer back
6. Connection established
```

#### WebSocket Mode (NEW)

```
1. User opens client with ws://server:port/peer_id
2. Client connects to signaling server
3. Client registers as peer_id
4. Client generates offer
5. Server forwards offer to target peer
6. Server forwards answer back
7. Connection established
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
- Manual SDP exchange required (unless using signaling server)
- Still in early development (v0.11)

**Alternative Considered**: `datachannel-rs` (libdatachannel C++ wrapper) - rejected due to FFI complexity

### tokio-tungstenite (v0.21)

**Rationale**: Async WebSocket client for signaling

**Pros**:
- Integrates with tokio async runtime
- Used by signaling server (consistency)
- Lightweight and efficient

**Cons**:
- Requires signaling server infrastructure

## LLM Integration

### Events

1. **`webrtc_channel_opened`** - Triggered when any data channel opens
    - Provides channel label
    - LLM can send initial message on channel
    - Supports multi-channel: each channel fires separate event

2. **`webrtc_message_received`** - Triggered when message arrives on any channel
    - Provides message text or hex-encoded binary
    - Provides channel label
    - Provides binary flag (true if hex-encoded)
    - LLM decides response action per channel

3. **`webrtc_signaling_connected`** (NEW) - Triggered when connected to signaling server
    - Provides peer_id and server_url
    - LLM can initiate offer to target peer

4. **`webrtc_connected`** (DEPRECATED) - Use `webrtc_channel_opened` instead

### Actions

#### User-Triggered (Async)

- **`send_message`** - Send text or binary message (use "hex:" prefix)
  - Optional `channel` parameter for multi-channel
  - Example: `{"type": "send_message", "message": "hex:48656c6c6f"}`

- **`send_binary`** - Send hex-encoded binary data
  - Optional `channel` parameter
  - Example: `{"type": "send_binary", "hex_data": "48656c6c6f"}`

- **`apply_answer`** - Apply remote SDP answer (manual mode only)
  - Required: `answer_json` (SDP JSON from peer)

- **`create_channel`** (NEW) - Create additional data channel
  - Required: `channel_label`
  - Example: `{"type": "create_channel", "channel_label": "file-transfer"}`

- **`send_offer`** (NEW) - Send offer to peer via signaling server
  - Required: `target_peer` (peer ID on signaling server)
  - Example: `{"type": "send_offer", "target_peer": "bob"}`

- **`disconnect`** - Close all channels and peer connection

#### Event-Triggered (Sync)

- **`send_message`** - Send reply message on same channel
  - Optional `channel` parameter (defaults to receiving channel)
  - Supports "hex:" prefix for binary

- **`disconnect`** - Close connection
- **`wait_for_more`** - Don't respond yet

### Connection Flow (WebSocket Mode)

```
1. User opens WebRTC client with ws://server:port/peer_id
2. Client connects to signaling server
3. Client registers as peer_id
4. webrtc_signaling_connected event fires
5. Client generates SDP offer automatically
6. (LLM can trigger send_offer to target peer)
7. Server forwards offer to target peer
8. Target peer sends answer back
9. Connection established
10. Data channels open
11. webrtc_channel_opened events fire
12. Messages exchanged via send_message actions
13. webrtc_message_received events fire
```

### Connection Flow (Manual Mode)

```
1. User opens WebRTC client (manual mode)
2. Client creates offer and generates SDP
3. SDP offer displayed to user
4. User pastes SDP answer from peer
5. LLM applies answer via apply_answer action
6. Connection established
7. Data channel opens
8. webrtc_channel_opened event fires
9. Messages exchanged via send_message actions
10. webrtc_message_received events fire
```

## State Management

### Per-Channel Connection States

- **Idle**: No LLM processing in progress
- **Processing**: LLM call active for current message
- **Accumulating**: Messages queued while LLM processes

**Benefits**:
- Independent state per channel
- No message loss during LLM processing
- Prevents concurrent LLM calls per channel

### Stored Data (protocol_data)

- `sdp_offer`: Generated SDP offer (JSON)
- `peer_connection_ptr`: Raw pointer to RTCPeerConnection (for lifecycle)
- `signaling_mode`: "manual" or WebSocket config (mode, url, peer_id)

**Safety Note**: Raw pointers are stored to maintain Arc references across async boundaries. Pointers are cleaned up when client is removed.

### Client Data (In-Memory)

- `memory`: LLM memory (shared across all channels)
- `channels`: HashMap of channel_label → ChannelData
  - `state`: ConnectionState (Idle/Processing/Accumulating)
  - `queued_messages`: Vec<(message, is_binary)>
  - `channel`: Arc<RTCDataChannel>

## Signaling Strategy

### Manual SDP Exchange (Original)

**Pros**:
- No infrastructure required
- Simple for two-peer connections
- Privacy (no signaling server)

**Cons**:
- User must manually exchange SDP
- Inconvenient for frequent connections
- Hard to automate

### WebSocket Signaling (NEW)

**Pros**:
- Automatic SDP exchange
- No manual copy-paste
- Scalable (many peers)
- LLM can control signaling

**Cons**:
- Requires signaling server
- Adds network dependency
- Privacy considerations (signaling server sees SDP)

**Compatibility**: Works with NetGet's WebRTC signaling server (`webrtc_signaling` protocol)

## Limitations

1. **No Media**: Audio/video not supported (data channels only)
2. **Basic Signaling**: No ICE candidate trickling (waits for full ICE gathering)
3. **No Renegotiation**: Connection parameters fixed at offer time
4. **Basic ICE**: Only Google STUN, no custom TURN servers (configurable via startup params)
5. **Channel Lifecycle**: Channels created after connection established may have timing issues
6. **No Channel Negotiation**: Target peer must be ready to receive channels

## Security Considerations

- **DTLS Encryption**: All data encrypted by default (WebRTC requirement)
- **Peer Authentication**: No built-in peer verification (trust SDP source)
- **ICE Candidate Privacy**: Local IP addresses exposed in SDP
- **STUN Server**: Trusts Google STUN (could add custom servers)
- **Signaling Server Trust**: WebSocket mode trusts signaling server (sees SDP)

**Recommended Mitigations**:
- Use TLS/WSS for signaling server connections
- Verify peer identity through application-level authentication
- Consider using TURN servers for privacy (hide IP addresses)
- Implement challenge-response over data channel

## Testing Strategy

See `tests/client/webrtc/CLAUDE.md` for testing details.

Key challenges:
- Requires two peers (NetGet + browser or two NetGet instances)
- Manual SDP exchange for E2E tests
- WebSocket mode requires running signaling server
- Could use loopback connections for automated testing

Test scenarios:
1. Manual mode: Two NetGet clients exchange SDP manually
2. WebSocket mode: Two clients via signaling server
3. Multi-channel: Create multiple channels, send on different streams
4. Binary data: Send hex-encoded data, verify decoding
5. Mixed messages: Interleave text and binary on same channel

## Performance Considerations

- **Memory**: Per-channel state (~1 KB overhead per channel)
- **CPU**: Minimal (message forwarding is fast)
- **Channels**: Recommended limit: 10-20 concurrent channels per connection
- **Bandwidth**: Depends on application (SCTP adds ~20 bytes overhead per message)
- **Latency**: P2P connection (typically < 50ms after connection established)

## Future Enhancements

1. **ICE Candidate Trickling**: Send candidates as discovered (faster connection)
2. **Connection Renegotiation**: Add/remove channels after connection established
3. **TURN Support**: Custom TURN servers for relay (privacy/NAT)
4. **Channel Lifecycle**: Better handling of dynamic channel creation
5. **Connection Stats**: Expose RTCStats for monitoring (bandwidth, latency, packet loss)
6. **Ordered/Unordered Channels**: Support unordered delivery for low-latency use cases
7. **Signaling Server Discovery**: Automatic discovery of signaling servers
8. **Multi-Peer Mesh**: Connect to multiple peers simultaneously

## Example Usage

### Manual Mode (Original)

```bash
# Open WebRTC client (generates SDP offer)
> open_client webrtc manual "Send hello message"

# SDP offer displayed - user exchanges with peer
# After peer responds, apply answer
> (LLM generates apply_answer action with pasted SDP)

# Connection established, send message
> (LLM sends message via send_message action)

# Receive messages
> (webrtc_message_received events processed by LLM)
```

### WebSocket Mode (NEW)

```bash
# Open WebRTC client with signaling server
> open_client webrtc ws://localhost:8080/alice "Connect to bob"

# Signaling connection established
> [EVENT] webrtc_signaling_connected (peer_id: "alice", server_url: "ws://localhost:8080/")

# Send offer to peer "bob"
> (LLM generates send_offer action with target_peer: "bob")

# Connection established automatically
> [EVENT] webrtc_channel_opened (channel_label: "netget")

# Send text message
> (LLM generates send_message action)

# Send binary data
> (LLM generates send_binary action with hex_data: "48656c6c6f")
```

### Multi-Channel (NEW)

```bash
# Create additional channel
> (LLM generates create_channel action with channel_label: "file-transfer")

# Channel opens
> [EVENT] webrtc_channel_opened (channel_label: "file-transfer")

# Send on specific channel
> (LLM generates send_message with channel: "file-transfer")

# Receive on specific channel
> [EVENT] webrtc_message_received (channel_label: "file-transfer", message: "...", is_binary: false)
```

## Integration with Signaling Server

The WebRTC client integrates with NetGet's WebRTC Signaling Server:

**Server Protocol**: `webrtc_signaling` (see `src/server/webrtc_signaling/CLAUDE.md`)

**Message Format**: JSON messages over WebSocket
- Register: `{"type": "register", "peer_id": "alice"}`
- Offer: `{"type": "offer", "from": "alice", "to": "bob", "sdp": {...}}`
- Answer: `{"type": "answer", "from": "bob", "to": "alice", "sdp": {...}}`
- ICE Candidate: `{"type": "ice_candidate", "from": "alice", "to": "bob", "candidate": {...}}`

**Workflow**:
1. Client connects to signaling server WebSocket
2. Client sends registration message
3. Client creates SDP offer
4. Client sends offer message to server (to be forwarded to peer)
5. Server forwards offer to target peer
6. Target peer sends answer back
7. Server forwards answer to client
8. Connection established

## References

- [webrtc-rs Documentation](https://docs.rs/webrtc/latest/webrtc/)
- [webrtc-rs GitHub](https://github.com/webrtc-rs/webrtc)
- [WebRTC Data Channels MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Using_data_channels)
- [WebRTC Signaling MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Signaling_and_video_calling)
- [Perfect Negotiation Pattern](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation)
- [tokio-tungstenite Documentation](https://docs.rs/tokio-tungstenite/)
