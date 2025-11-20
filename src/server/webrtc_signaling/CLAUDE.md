# WebRTC Signaling Server Implementation

## Overview

WebRTC Signaling Server implementation for NetGet. This server provides **WebSocket-based SDP and ICE candidate relay** to facilitate automatic peer-to-peer WebRTC connections. It eliminates the need for manual SDP exchange, enabling seamless WebRTC communication between peers.

## Purpose

WebRTC requires an out-of-band signaling mechanism to exchange:
1. **SDP offers** - Connection parameters from initiating peer
2. **SDP answers** - Connection response from receiving peer
3. **ICE candidates** - Network path information for NAT traversal

This signaling server acts as a message relay, forwarding these messages between peers so they can establish direct P2P connections.

## Architecture

### Core Components

1. **WebSocket Server**: Accepts WebSocket connections from peers
2. **Peer Registry**: Tracks connected peers by unique peer IDs
3. **Message Relay**: Forwards signaling messages between registered peers
4. **LLM Monitoring**: Optionally monitors and logs signaling flow

### Protocol Stack

```
Application (SDP/ICE exchange)
    ↓
WebSocket (bidirectional messaging)
    ↓
TCP
    ↓
IP
```

## Library Choices

### tokio-tungstenite (v0.21)

**Rationale**: Pure Rust async WebSocket implementation

**Pros**:
- Integrates seamlessly with tokio async runtime
- Lightweight and efficient
- No C dependencies
- Well-maintained and widely used

**Cons**:
- Requires async/await understanding
- WebSocket protocol adds overhead vs raw TCP

**Alternative Considered**: Manual TCP with custom framing - rejected for complexity

## Signaling Protocol

### Message Types

All messages are JSON objects with a `type` field:

#### 1. Register

**From**: Client → Server
**Purpose**: Register with a unique peer ID

```json
{
  "type": "register",
  "peer_id": "alice"
}
```

**Response** (implicit - connection remains open):
```json
{
  "type": "registered",
  "peer_id": "alice"
}
```

#### 2. Offer

**From**: Peer A → Server → Peer B
**Purpose**: Initiate WebRTC connection

```json
{
  "type": "offer",
  "from": "alice",
  "to": "bob",
  "sdp": {
    "type": "offer",
    "sdp": "v=0\r\no=- ... [full SDP]"
  }
}
```

#### 3. Answer

**From**: Peer B → Server → Peer A
**Purpose**: Accept WebRTC connection

```json
{
  "type": "answer",
  "from": "bob",
  "to": "alice",
  "sdp": {
    "type": "answer",
    "sdp": "v=0\r\no=- ... [full SDP]"
  }
}
```

#### 4. ICE Candidate

**From**: Peer A ↔ Server ↔ Peer B
**Purpose**: Exchange network paths

```json
{
  "type": "ice_candidate",
  "from": "alice",
  "to": "bob",
  "candidate": {
    "candidate": "candidate:... [ICE candidate string]",
    "sdpMLineIndex": 0,
    "sdpMid": "0"
  }
}
```

#### 5. Error

**From**: Server → Client
**Purpose**: Report errors

```json
{
  "type": "error",
  "message": "Peer ID already registered"
}
```

## Connection Flow

### Basic Signaling Flow

```
1. Alice connects to signaling server
   → WebSocket: ws://localhost:8080
   → Sends: {"type": "register", "peer_id": "alice"}

2. Bob connects to signaling server
   → WebSocket: ws://localhost:8080
   → Sends: {"type": "register", "peer_id": "bob"}

3. Alice initiates WebRTC connection
   → Generates SDP offer locally
   → Sends: {"type": "offer", "from": "alice", "to": "bob", "sdp": {...}}
   → Server forwards to Bob

4. Bob receives offer and responds
   → Generates SDP answer locally
   → Sends: {"type": "answer", "from": "bob", "to": "alice", "sdp": {...}}
   → Server forwards to Alice

5. ICE candidates exchanged (both directions)
   → Alice/Bob send candidates as they're discovered
   → Server forwards each to the other peer

6. WebRTC connection established
   → Peers connect directly (P2P)
   → Signaling connection can be closed (but often kept open)
```

## LLM Integration

### Events

1. **`webrtc_signaling_peer_connected`** - Peer registered with server
    - Provides peer_id and remote_addr
    - LLM can monitor which peers are online

2. **`webrtc_signaling_peer_disconnected`** - Peer disconnected
    - Provides peer_id
    - LLM can track peer lifecycle

3. **`webrtc_signaling_message_received`** - Signaling message received
    - Provides peer_id, message_type, target_peer
    - LLM can monitor signaling flow

### Actions

#### User-Triggered (Async)

- **`list_signaling_peers`** - List all connected signaling peers
- **`broadcast_message`** - Broadcast message to all peers (future)

#### Event-Triggered (Sync)

None currently - signaling server is mostly passive relay

## State Management

### Peer Tracking

Each connected peer is tracked with:
- **peer_id**: Unique identifier (chosen by peer)
- **ws_tx**: WebSocket sender for forwarding messages
- **remote_addr**: TCP address of peer
- **connection_id**: NetGet connection tracking ID

### Message Forwarding

Messages are forwarded based on the `to` field:
1. Parse incoming message
2. Look up recipient by peer_id
3. Forward message via recipient's WebSocket
4. If recipient not found, silently drop (or send error)

## Usage Patterns

### Pattern 1: Browser ↔ NetGet WebRTC Server

```
Browser (JavaScript)           Signaling Server           NetGet WebRTC Server
    |                                 |                            |
    |--- register (peer: "browser") >|                            |
    |                                 |< register (peer: "netget")-|
    |                                 |                            |
    |--- offer (to: "netget") ------->|                            |
    |                                 |--- offer (from: "browser")>|
    |                                 |                            |
    |                                 |<--- answer (to: "browser")-|
    |<--- answer (from: "netget") ----|                            |
    |                                 |                            |
    |<=========== P2P Data Channel ===========>|
```

### Pattern 2: Two NetGet Instances

```
NetGet Client A      Signaling Server      NetGet Client B
    |                       |                      |
    |--- register("A") ---->|                      |
    |                       |<--- register("B") ---|
    |                       |                      |
    |--- offer(to:"B") ---->|                      |
    |                       |--- offer(from:"A") ->|
    |                       |                      |
    |                       |<-- answer(to:"A") ---|
    |<-- answer(from:"B") --|                      |
    |                       |                      |
    |<======== P2P Data Channel ========>|
```

## Implementation Details

### WebSocket Upgrade

```rust
// Accept TCP connection
let stream = listener.accept().await?;

// Upgrade to WebSocket
let ws_stream = accept_async(stream).await?;

// Split into sender/receiver
let (ws_tx, ws_rx) = ws_stream.split();
```

### Peer Registration

```rust
// Store peer with WebSocket sender
peers.insert(peer_id.clone(), Arc::new(Mutex::new(PeerConnection {
    peer_id,
    ws_tx,  // Moved ownership - can send messages later
    remote_addr,
    connection_id,
})));
```

### Message Forwarding

```rust
// Find recipient
let peer_conn = peers.get(&to_peer_id)?;

// Forward message
peer_conn.lock().await.ws_tx
    .send(Message::Text(json_msg))
    .await?;
```

## Limitations

1. **No Authentication**: Any peer can register any peer_id (first-come-first-served)
2. **No Encryption**: WebSocket is not encrypted (use wss:// in production)
3. **No Persistence**: Peer registry is in-memory only
4. **No Reconnection**: Peers must re-register if disconnected
5. **No Message Queue**: If recipient offline, message is dropped
6. **Single Server**: No clustering or load balancing

## Security Considerations

- **Peer ID Spoofing**: Malicious peer can claim any peer_id
- **DoS Risk**: Malicious peer can flood server with messages
- **Eavesdropping**: SDP contains IP addresses (privacy leak)
- **No Rate Limiting**: Unlimited message forwarding
- **No Authorization**: No control over who can signal whom

**Recommended Mitigations**:
- Add peer authentication (tokens, certificates)
- Implement rate limiting per peer
- Use TLS/WSS for encryption
- Validate peer IDs (e.g., UUID format)
- Add message size limits

## Testing Strategy

See `tests/server/webrtc_signaling/CLAUDE.md` for testing details.

Key test scenarios:
- Two peers register and exchange offers/answers
- ICE candidate forwarding
- Peer disconnection handling
- Duplicate peer ID handling
- Message forwarding errors

## Performance Considerations

- **Memory**: Each peer consumes ~1 KB (WebSocket overhead)
- **CPU**: Minimal (message forwarding is fast)
- **Connections**: Recommended limit: 1,000-10,000 concurrent peers
- **Bandwidth**: Depends on signaling traffic (typically < 10 KB/peer)

## Future Enhancements

1. **Authentication**: Token-based peer verification
2. **Persistence**: Redis/database backend for peer registry
3. **Clustering**: Multi-server signaling with shared state
4. **Message Queue**: Offline message delivery
5. **Presence**: Track online/offline status
6. **Rooms**: Group peers into rooms/channels
7. **Broadcast**: Send to all peers in a room
8. **Metrics**: Track signaling statistics
9. **Rate Limiting**: Per-peer message throttling
10. **TLS/WSS**: Encrypted WebSocket connections

## Example Usage

```bash
# Start signaling server
> open_server webrtc-signaling 0.0.0.0:8080 "WebRTC signaling relay"

# Server displays: "WebRTC Signaling server listening on 0.0.0.0:8080"

# Peer A connects (WebSocket client or NetGet)
> [EVENT] webrtc_signaling_peer_connected (peer_id: "alice", remote_addr: "127.0.0.1:54321")

# Peer B connects
> [EVENT] webrtc_signaling_peer_connected (peer_id: "bob", remote_addr: "127.0.0.1:54322")

# Alice sends offer to Bob
> [Forwarded] offer from alice to bob

# Bob sends answer to Alice
> [Forwarded] answer from bob to alice

# Connection established
> # Peers now have direct P2P connection, signaling complete

# List connected peers
> [ACTION] list_signaling_peers
> Active signaling peers: ["alice", "bob"]

# Peer disconnects
> [EVENT] webrtc_signaling_peer_disconnected (peer_id: "bob")
```

## Integration with WebRTC Server/Client

The signaling server works with:
- **WebRTC Server** (`src/server/webrtc/`): Peers can signal to NetGet server instances
- **WebRTC Client** (`src/client/webrtc/`): NetGet clients can use signaling for automatic SDP exchange
- **External Peers**: Any WebSocket client (browsers, other apps)

## References

- [WebSocket Protocol (RFC 6455)](https://tools.ietf.org/html/rfc6455)
- [WebRTC Signaling](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Signaling_and_video_calling)
- [Perfect Negotiation Pattern](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation)
- [tokio-tungstenite Documentation](https://docs.rs/tokio-tungstenite/)
