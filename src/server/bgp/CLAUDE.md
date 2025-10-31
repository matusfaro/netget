# BGP Server Implementation

## Overview

Border Gateway Protocol (BGP-4) server implementing RFC 4271 with a 6-state FSM. The LLM controls routing policy decisions, peer authentication, and route advertisements.

**Status**: Alpha (fully implemented, needs extensive testing)
**Protocol Spec**: [RFC 4271 (BGP-4)](https://datatracker.ietf.org/doc/html/rfc4271)
**Port**: TCP 179

## Library Choices

### No Library - Manual Protocol Implementation

**Why no library**:
- No mature Rust BGP server library exists
- Available crates focus on parsing or client operations
- BGP protocol is complex but well-specified (RFC 4271)
- Manual implementation provides full LLM control over routing decisions

**What we implement manually**:
- BGP message construction (OPEN, UPDATE, NOTIFICATION, KEEPALIVE)
- 6-state FSM (Idle, Connect, Active, OpenSent, OpenConfirm, Established)
- Message parsing and validation
- Hold timer negotiation
- Error handling with NOTIFICATION messages

**Why not alternatives**:
- `bgp-rs` - Parsing library, not a server
- `bgpkit-parser` - MRT dump parser, not for live BGP
- External BGP daemon (bird, frr) - Violates NetGet architecture

## Architecture Decisions

### TCP-Based Protocol

BGP uses TCP for reliability:
```rust
let listener = create_reusable_tcp_listener(listen_addr).await?;
```

Each peer connection handled in separate async task.

### 6-State Finite State Machine

BGP session lifecycle (RFC 4271 Section 8):

1. **Idle**: Initial state, no TCP connection
2. **Connect**: TCP connection established, waiting to send OPEN
3. **Active**: TCP connection failed, attempting to reconnect
4. **OpenSent**: Sent OPEN message, waiting for peer's OPEN
5. **OpenConfirm**: Received OPEN, sent KEEPALIVE, waiting for peer's KEEPALIVE
6. **Established**: Full peering established, can exchange UPDATEs

Current implementation handles:
- ✅ Connect (TCP accepted)
- ✅ OpenSent (send OPEN after receiving peer's OPEN)
- ✅ OpenConfirm (send KEEPALIVE after peer's KEEPALIVE)
- ✅ Established (can receive UPDATEs)

Not implemented:
- ❌ Idle → Connect transition (server waits for incoming connections)
- ❌ Active state (no reconnection logic)

### BGP Message Format

All BGP messages have 19-byte header:
```
+------------------+------------------+
| Marker (16 bytes, all 0xFF)          |
+------------------+------------------+
| Length (2 bytes) | Type (1 byte)    |
+------------------+------------------+
```

**Message types**:
1. OPEN - Session establishment
2. UPDATE - Route advertisements
3. NOTIFICATION - Error reporting
4. KEEPALIVE - Session maintenance
5. ROUTE-REFRESH - Request route re-advertisement (not implemented)

### Hold Timer Negotiation

Server and peer negotiate hold time (minimum of both values):
```rust
if hold_time > 0 {
    self.hold_time = self.hold_time.min(hold_time);
    self.keepalive_time = self.hold_time / 3;
}
```

Default: 180 seconds hold time, 60 seconds keepalive.

### AS Number Handling

Currently uses 16-bit AS numbers (0-65535):
- Supports private ASNs (64512-65534)
- 32-bit AS numbers (RFC 6793) not yet implemented

## LLM Integration

### Startup Parameters

Server configured with:
```json
{
  "as_number": 65001,
  "router_id": "192.168.1.1"
}
```

Extracted from LLM-generated startup prompt.

### Async Actions (User-triggered)

1. **list_peers**: View all BGP sessions
2. **shutdown_peer**: Gracefully shut down specific peer
3. **advertise_route**: Manually advertise route to peers
4. **withdraw_route**: Withdraw previously advertised route

### Sync Actions (Network event triggered)

1. **send_bgp_open**: Send OPEN message with AS and router ID
2. **send_bgp_keepalive**: Send KEEPALIVE to maintain session
3. **send_bgp_update**: Send route advertisements/withdrawals
4. **send_bgp_notification**: Send error NOTIFICATION and close session
5. **accept_peer**: Accept peering request
6. **reject_peer**: Reject peering with NOTIFICATION

### Event Types

- `bgp_open`: Peer sent OPEN message
- `bgp_update`: Peer sent route UPDATE
- `bgp_notification`: Peer sent NOTIFICATION (error or graceful shutdown)

**Event data example** (OPEN):
```json
{
  "connection_id": "conn_12345",
  "peer_as": 65000,
  "hold_time": 180,
  "router_id": "192.168.1.100",
  "remote_addr": "192.0.2.10:12345"
}
```

## Connection Management

### Per-Session State

Each BGP session tracked with:
```rust
struct BgpSession {
    stream: TcpStream,
    connection_id: ConnectionId,
    session_state: BgpSessionState,
    peer_as: Option<u32>,
    hold_time: u16,
    keepalive_time: u16,
    router_id: String,
    local_as: u32,
}
```

### Message Loop

Each session runs event loop reading BGP messages:
```rust
loop {
    let mut header_buf = vec![0u8; 19];
    stream.read_exact(&mut header_buf).await?;

    // Validate marker
    // Parse length and type
    // Read message body
    // Handle based on type and current state
}
```

### Connection Lifecycle

1. **Accept**: TCP connection accepted, spawn session task
2. **Receive OPEN**: Peer sends OPEN → validate → send OPEN response
3. **Receive KEEPALIVE**: Peer sends KEEPALIVE → transition to Established
4. **Exchange KEEPALIVEs**: Periodic keepalives maintain session
5. **Receive NOTIFICATION**: Peer closes session → log and cleanup

## State Management

### Server State

```rust
pub struct BgpServer;  // Stateless, each session independent
```

Server spawns independent session tasks, no global state.

### Session State Enum

```rust
pub enum BgpSessionState {
    Idle,
    Connect,
    Active,
    OpenSent,
    OpenConfirm,
    Established,
}
```

Stored per-session in `BgpSession`.

### Protocol Connection Info

```rust
ProtocolConnectionInfo::Bgp {
    session_state: BgpSessionState,
    peer_as: Option<u32>,
    router_id: String,
    hold_time: u16,
}
```

## Limitations

### Partial Implementation

**Implemented**:
- ✅ TCP connection handling
- ✅ OPEN message exchange
- ✅ KEEPALIVE exchange
- ✅ Session FSM (Connect → OpenSent → OpenConfirm → Established)
- ✅ NOTIFICATION error handling
- ✅ Basic UPDATE parsing

**Not Implemented**:
- ❌ Route processing (UPDATE messages parsed but not acted upon)
- ❌ RIB (Routing Information Base) management
- ❌ Route advertisements (can send UPDATE structure, but no route storage)
- ❌ Path attributes parsing (UPDATE body is hex-encoded for LLM)
- ❌ Route filtering/policy
- ❌ 32-bit AS numbers (RFC 6793)
- ❌ Multiprotocol extensions (RFC 4760)
- ❌ Route refresh (RFC 2918)
- ❌ Hold timer enforcement (no timeout logic)

### No Routing Table

Server doesn't maintain routing table:
- UPDATE messages sent to LLM as hex-encoded data
- LLM cannot make informed routing decisions without route storage
- Cannot re-advertise learned routes to other peers

### No Multi-Peer Support

Each session independent:
- No route propagation between peers
- No best path selection
- No loop prevention (AS_PATH checking)

### Testing Limitations

BGP protocol is complex:
- Full testing requires multiple interconnected BGP speakers
- E2E tests cover basic peering but not route exchange
- No tests for route convergence, path selection, or policy

## Examples

### Server Startup

```
netget> Listen on port 179 via BGP. You are AS 65001 with router ID 192.168.1.1.
```

Server output:
```
[INFO] BGP server listening on 0.0.0.0:179
[INFO] BGP configured with AS 65001 and router ID 192.168.1.1
→ BGP server ready on 0.0.0.0:179
```

### Peer Connection

Client connects and sends OPEN:
```
[INFO] BGP connection conn_12345 from 192.0.2.10:12345
→ BGP connection conn_12345 from 192.0.2.10:12345
```

LLM receives OPEN event:
```json
{
  "event": "bgp_open",
  "data": {
    "connection_id": "conn_12345",
    "peer_as": 65000,
    "hold_time": 180,
    "router_id": "192.168.1.100"
  }
}
```

LLM responds by sending OPEN:
```json
{
  "actions": [
    {
      "type": "send_bgp_open",
      "my_as": 65001,
      "hold_time": 180,
      "router_id": "192.168.1.1"
    }
  ]
}
```

Server sends OPEN:
```
[INFO] BGP OPEN sent: AS=65001, hold_time=180s
[DEBUG] BGP session transitioned to OpenConfirm
```

### Keepalive Exchange

Peer sends KEEPALIVE:
```
[DEBUG] BGP KEEPALIVE received
✓ BGP session conn_12345 established with AS65000
```

Server responds with KEEPALIVE:
```
[TRACE] BGP KEEPALIVE sent
```

### UPDATE Message

Peer sends UPDATE:
```
[TRACE] BGP UPDATE received: 128 bytes
```

LLM receives UPDATE event:
```json
{
  "event": "bgp_update",
  "data": {
    "connection_id": "conn_12345",
    "peer_as": 65000,
    "update_data": "00000000..." // Hex-encoded UPDATE body
  }
}
```

LLM can analyze hex data (requires protocol expertise).

### NOTIFICATION (Graceful Shutdown)

Peer sends NOTIFICATION with error code 6 (Cease):
```
[ERROR] BGP NOTIFICATION received: code=6, subcode=0
```

Session closes gracefully.

### Error Handling

Invalid OPEN version:
```
[ERROR] BGP invalid message: Unsupported BGP version: 3
```

Server sends NOTIFICATION:
```
[ERROR] BGP NOTIFICATION sent: code=2, subcode=1
```

## Use Cases

### Learning BGP Protocol

- Understand BGP message flow
- Experiment with session establishment
- Practice BGP configuration

### Honeypot/Monitoring

- Detect unauthorized BGP peering attempts
- Log BGP reconnaissance
- Monitor for BGP hijacking attempts

### Testing

- Test BGP client implementations
- Validate BGP message parsing
- Simulate BGP peer behavior

### NOT for Production Routing

BGP server should **not** be used for production routing:
- No routing table or best path selection
- No route propagation or filtering
- No policy enforcement
- No hold timer or session management

For production routing, use established BGP implementations (BIRD, FRR, Quagga).

## References

- [RFC 4271 - BGP-4](https://datatracker.ietf.org/doc/html/rfc4271)
- [RFC 6793 - 32-bit AS Numbers](https://datatracker.ietf.org/doc/html/rfc6793)
- [RFC 4760 - Multiprotocol Extensions](https://datatracker.ietf.org/doc/html/rfc4760)
- [RFC 2918 - Route Refresh](https://datatracker.ietf.org/doc/html/rfc2918)
- [BGP Tutorial](https://www.cisco.com/c/en/us/support/docs/ip/border-gateway-protocol-bgp/26634-bgp-toc.html)
