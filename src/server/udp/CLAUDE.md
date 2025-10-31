# UDP Protocol Implementation

## Overview
UDP (User Datagram Protocol) server implementing connectionless datagram handling where the LLM has full control over UDP packet responses. This is the foundation for UDP-based protocols like DNS, DHCP, NTP, SNMP, and custom datagram protocols.

**Status**: Beta (Core Protocol)
**RFC**: RFC 768 (User Datagram Protocol)

## Library Choices
- **tokio::net::UdpSocket** - Async UDP socket from Tokio runtime
- **Manual datagram handling** - LLM receives raw datagram bytes and constructs responses

**Rationale**: UDP is simpler than TCP - no connection management, no stream splitting. A single socket handles all peers. The LLM directly controls datagram content.

## Architecture Decisions

### 1. Connectionless Nature
UDP has no connections, but NetGet models each datagram as a "connection" for UI consistency:
- Each received datagram creates a new `ConnectionId`
- Connection represents "recent peer" for that datagram
- No persistent state between datagrams from the same peer

This design allows the TUI to display UDP activity similar to TCP connections.

### 2. Single Socket for All Peers
Unlike TCP (one socket per connection), UDP uses a single socket for all peers:
- `UdpSocket::recv_from()` receives from any peer
- `UdpSocket::send_to(data, peer_addr)` sends to specific peer
- No need to track socket state per peer

### 3. Stateless Processing
Each datagram is processed independently:
- No state machine (unlike TCP's Idle/Processing/Accumulating)
- No data queueing (each datagram is independent)
- LLM called once per datagram
- No `wait_for_more` support (UDP is message-oriented, not stream-oriented)

### 4. Peer Tracking
For UI purposes, recent peers are tracked:
- `ProtocolConnectionInfo::Udp` contains `recent_peers: Vec<(SocketAddr, Instant)>`
- Each datagram updates the peer's last activity time
- Old peers can be pruned (though not currently implemented)

### 5. Maximum Datagram Size
Buffer size is 65535 bytes (maximum UDP datagram size including headers):
- IP MTU typically 1500 bytes, so most datagrams are much smaller
- Large datagrams may be fragmented by network layer
- LLM receives entire datagram (no partial reads like TCP)

### 6. Dual Logging
Like TCP, all operations use dual logging:
- **DEBUG**: Datagram summary with 100-char preview
- **TRACE**: Full payload (text as string, binary as hex)
- Both go to `netget.log` and TUI Status panel

## LLM Integration

### Action-Based Response Model
The LLM responds to UDP events with actions:

**Events**:
- `udp_datagram_received` - Datagram received from peer
  - Parameters: `peer_address`, `data_length`, `data_preview`

**Available Actions**:
- `send_udp_response` - Send datagram to peer (text or hex)
- `send_to_address` - Send datagram to arbitrary address (async action)
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Response
```json
{
  "actions": [
    {
      "type": "send_udp_response",
      "data": "PONG"
    },
    {
      "type": "show_message",
      "message": "Echoed PING"
    }
  ]
}
```

### Data Format
- **Text data**: Sent as-is in the `data` field
- **Binary data**: Sent as hex string (e.g., `"48656c6c6f"` for "Hello")
- **Received data**: Full datagram passed to LLM as `data_preview` (truncated to 200 bytes in event)

## Connection Management

### Pseudo-Connection Lifecycle
1. **Receive**: `UdpSocket::recv_from()` receives datagram and peer address
2. **Register**: Create new `ConnectionId` and add to `ServerInstance`
3. **Process**: Spawn async task to call LLM and generate response
4. **Send**: Use same socket to send response via `send_to()`
5. **Track**: Connection remains in UI until pruned

### Connection Data Structure
```rust
ProtocolConnectionInfo::Udp {
    recent_peers: Vec<(SocketAddr, Instant)>, // Track recent peer activity
}
```

Unlike TCP, no write_half or queued_data - UDP is stateless.

### State Updates
- Connection state tracked in `ServerInstance.connections`
- Each datagram increments `packets_received` and `bytes_received`
- Response increments `packets_sent` and `bytes_sent`
- UI updates via `__UPDATE_UI__` message

## Known Limitations

### 1. No Connection Affinity
Each datagram is treated as a new "connection" with a new `ConnectionId`. There's no way to associate multiple datagrams from the same peer unless the LLM maintains state via `update_instruction`.

### 2. No Fragmentation Handling
If a datagram exceeds network MTU and is fragmented, the OS reassembles it. NetGet sees only the complete datagram. No visibility into fragmentation.

### 3. No Delivery Guarantees
UDP is unreliable by nature:
- No acknowledgments
- No retransmission
- Packets may be lost, duplicated, or reordered
- LLM responses may never reach the client

### 4. No Rate Limiting
Server processes every received datagram:
- No protection against UDP flood attacks
- No throttling of LLM calls
- Can be overwhelmed by high packet rates

### 5. No Multi-packet Responses
LLM can send only one response datagram per received datagram. No built-in support for protocols that require multiple responses (though the LLM could use `send_to_address` async action for additional sends).

### 6. Peer Tracking Never Pruned
Recent peers accumulate forever in `ProtocolConnectionInfo::Udp`. No automatic cleanup of old peers.

## Example Prompts

### Echo Server
```
listen on port 9 via udp
When you receive any data, echo it back to the sender
```

### PING/PONG Server
```
listen on port 8000 via udp
When you receive "PING", respond with "PONG"
When you receive anything else, respond with "Unknown command"
```

### Binary Protocol
```
listen on port 9001 via udp
When you receive a 4-byte big-endian integer, respond with the integer + 1
Use hex encoding for binary data
```

### Stateless Request/Response
```
listen on port 5000 via udp
Parse incoming JSON requests
Respond with JSON: {"status": "ok", "echo": <received_data>}
```

## Performance Characteristics

### Latency
- One LLM call per received datagram
- Typical latency: 2-5 seconds per datagram with qwen3-coder:30b
- No connection setup overhead (unlike TCP)

### Throughput
- Limited by LLM response time (same as TCP)
- Datagrams processed concurrently (each on separate tokio task)
- No queueing mechanism (UDP is fire-and-forget)

### Concurrency
- Unlimited concurrent datagrams (bounded by system resources)
- All datagrams share the same socket
- Ollama lock serializes LLM API calls across all datagrams

### Packet Loss
- If LLM response takes too long, client may timeout
- No retransmission - client must implement retry logic
- Lost packets have no impact on server state (stateless)

## Comparison with TCP

| Feature | TCP | UDP |
|---------|-----|-----|
| Connection | Stateful, persistent | Stateless, per-datagram |
| State Machine | Idle/Processing/Accumulating | None (stateless) |
| Data Queueing | Yes | No (each datagram independent) |
| Stream Splitting | Required (ReadHalf/WriteHalf) | Not needed (single socket) |
| Reliability | Guaranteed delivery | Best effort |
| Order | In-order delivery | May be reordered |
| Use Case | Protocols requiring reliability | Fast, stateless protocols |

## References
- [RFC 768: User Datagram Protocol](https://datatracker.ietf.org/doc/html/rfc768)
- [Tokio UdpSocket](https://docs.rs/tokio/latest/tokio/net/struct.UdpSocket.html)
- [UDP on Wikipedia](https://en.wikipedia.org/wiki/User_Datagram_Protocol)
