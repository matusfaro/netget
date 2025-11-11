# TURN Protocol Implementation

## Overview

TURN (Traversal Using Relays around NAT) server implementing RFC 8656 (TURN - Traversal Using Relays around NAT).
Provides relay functionality for NAT traversal when direct peer-to-peer connection fails (e.g., symmetric NAT). Extends
STUN protocol with allocation and relay capabilities.

**Compliance**: RFC 8656 (TURN), RFC 8489 (STUN), RFC 5766 (obsolete TURN)

**Protocol Purpose**: TURN relays traffic between peers when direct connection impossible due to restrictive NATs or
firewalls. Essential fallback for WebRTC, VoIP, and real-time communication.

## Library Choices

**Manual Implementation** - Complete TURN protocol built on STUN message format

- **Why**: TURN is STUN + allocation management + relay logic
- No mature Rust TURN server libraries with LLM integration
- Manual implementation provides full control over allocation policies

**Extends STUN**:

- Uses STUN message format (20-byte header + attributes)
- Adds new methods: Allocate (3), Refresh (4), CreatePermission (8), SendIndication (6)
- Reuses STUN transport (UDP on port 3478 by default)

## Architecture Decisions

### Stateful Relay Server

Unlike STUN (stateless), TURN maintains **allocation state**:

- Maps client ↔ relay address
- Tracks allocation lifetime
- Manages peer permissions
- Relays data between client and permitted peers

**Allocation Lifecycle**:

1. **Allocate Request**: Client requests relay address from TURN server
2. **Allocate Response**: Server assigns relay address (e.g., 203.0.113.5:54321), returns lifetime
3. **Relay Active**: Server forwards data between client and permitted peers
4. **Refresh Requests**: Client extends lifetime before expiration
5. **Expiration**: Allocation deleted after lifetime expires (cleanup task)

### Allocation Management

**`TurnAllocation` struct** tracks per-client state:

```rust
pub struct TurnAllocation {
    client_addr: SocketAddr,           // Client's IP:port
    relay_addr: SocketAddr,            // Assigned relay IP:port
    allocated_at: Instant,             // Allocation timestamp
    expires_at: Instant,               // Expiration time
    lifetime_seconds: u32,             // Negotiated lifetime
    permitted_peers: Vec<SocketAddr>,  // Peers allowed to send/receive
}
```

**Allocation Storage**:

- `HashMap<String, TurnAllocation>` (key = allocation_id hex string)
- Wrapped in `Arc<Mutex<>>` for concurrent access
- Cleaned up by periodic background task (every 30 seconds)

**Allocation ID**:

- Unique identifier per allocation (currently transaction ID from allocate request)
- Future: Could use random UUID for security

### TURN Message Types

**Request Methods**:

- **Allocate (0x0003)**: Request relay address allocation
- **Refresh (0x0004)**: Extend allocation lifetime
- **CreatePermission (0x0008)**: Add peer to permitted list
- **SendIndication (0x0006)**: Send data through relay (client → peer)
- **DataIndication (0x0007)**: Receive data through relay (peer → client)

**Response Classes**:

- Success Response (class=1): Method + 0x0100 (e.g., Allocate Success = 0x0103)
- Error Response (class=2): Method + 0x0110 (e.g., Allocate Error = 0x0113)

### Relay Data Flow

**Outbound (Client → Peer via TURN)**:

1. Client sends SendIndication to TURN server
2. TURN checks allocation exists and peer permitted
3. TURN relays data to peer from relay address
4. Peer sees traffic from relay address (not client)

**Inbound (Peer → Client via TURN)**:

1. Peer sends data to relay address
2. TURN checks if peer permitted for this allocation
3. TURN sends DataIndication to client
4. Client receives data with peer's address in indication

**Not Yet Implemented**: Full relay logic pending. Currently handles allocation management only.

## LLM Integration

### Action-Based Allocation Control

**Allocate Request** (`TURN_ALLOCATE_REQUEST_EVENT`):

```json
{
  "actions": [
    {
      "type": "send_turn_allocate_response",
      "allocation_id": "abc123def456",
      "relay_address": "203.0.113.5:54321",
      "lifetime_seconds": 600,
      "transaction_id": "0102030405060708090a0b0c"
    },
    {
      "type": "send_turn_allocate_error",
      "error_code": 508,
      "reason": "Insufficient Capacity",
      "transaction_id": "0102030405060708090a0b0c"
    }
  ]
}
```

**Refresh Request** (`TURN_REFRESH_REQUEST_EVENT`):

```json
{
  "actions": [
    {
      "type": "send_turn_refresh_response",
      "lifetime_seconds": 600,
      "transaction_id": "..."
    }
  ]
}
```

**CreatePermission Request** (`TURN_CREATE_PERMISSION_REQUEST_EVENT`):

```json
{
  "actions": [
    {
      "type": "send_turn_create_permission_response",
      "peer_address": "198.51.100.10:5000",
      "transaction_id": "..."
    }
  ]
}
```

**SendIndication** (`TURN_SEND_INDICATION_EVENT`):

```json
{
  "actions": [
    {
      "type": "relay_turn_data",
      "allocation_id": "abc123",
      "peer_address": "198.51.100.10:5000",
      "data_base64": "...",
      "message": "Relaying data"
    }
  ]
}
```

### Event Types

1. **`TURN_ALLOCATE_REQUEST_EVENT`**
    - Triggered: Client requests relay allocation
    - Context: peer_addr, local_addr, transaction_id, existing_allocations
    - LLM decides: Allocate (with relay_address, lifetime) or Deny (error code)

2. **`TURN_REFRESH_REQUEST_EVENT`**
    - Triggered: Client requests lifetime extension
    - Context: peer_addr, transaction_id, existing_allocations
    - LLM decides: Extend lifetime or reject

3. **`TURN_CREATE_PERMISSION_REQUEST_EVENT`**
    - Triggered: Client adds peer to permission list
    - Context: peer_addr, transaction_id, existing_allocations
    - LLM decides: Allow peer or reject

4. **`TURN_SEND_INDICATION_EVENT`**
    - Triggered: Client sends data through relay
    - Context: peer_addr, data, existing_allocations
    - LLM decides: Relay data or drop

## Connection and State Management

**Per-Client State** (`ProtocolConnectionInfo::Turn`):

```rust
Turn {
    allocation_ids: Vec<String>,       // All allocation IDs for this client
    relay_addresses: Vec<String>,      // Assigned relay addresses
}
```

**Global Allocation State** (`TurnServer`):

```rust
pub struct TurnServer {
    allocations: Arc<Mutex<HashMap<String, TurnAllocation>>>,
}
```

**Cleanup Task**:

- Spawned at server startup
- Runs every 30 seconds
- Deletes allocations where `expires_at <= now`
- Logs expired allocations

**Connection Lifecycle**:

1. Client sends Allocate Request → Create allocation entry
2. Server assigns relay address → Store in allocations HashMap
3. Client sends Refresh Requests → Update `expires_at`
4. Client sends CreatePermission → Add peer to `permitted_peers`
5. Allocation expires or client sends Refresh with lifetime=0 → Delete allocation

## Protocol Compliance

### Supported Features

- ✅ Allocate Request/Response (RFC 8656 Section 6.2)
- ✅ Refresh Request/Response (RFC 8656 Section 7)
- ✅ CreatePermission Request/Response (RFC 8656 Section 9)
- ✅ Allocation lifetime management and expiration
- ✅ XOR-RELAYED-ADDRESS attribute (0x0016)
- ✅ LIFETIME attribute (0x000D)
- ✅ Error responses (e.g., 508 Insufficient Capacity)

### Not Yet Fully Implemented

- ⚠️ **Data Relay**: SendIndication/DataIndication not forwarding data yet
- ⚠️ **Channel Binding**: ChannelBind/ChannelData methods (RFC 8656 Section 11)
- ❌ **TCP Allocations**: Only UDP relay addresses
- ❌ **Dual-Stack Allocations**: IPv4-only (no REQUESTED-ADDRESS-FAMILY)
- ❌ **Authentication**: REALM, NONCE, MESSAGE-INTEGRITY attributes
- ❌ **Mobility**: MOBILITY-TICKET for client IP changes

### Protocol Compliance Gaps

**RFC 8656 Features Not Implemented**:

- Alternate Server (ALTERNATE-SERVER)
- Reservation tokens (RESERVATION-TOKEN)
- Even/odd port allocation (EVEN-PORT, RESERVE-NEXT-HIGHER-PORT)
- Don't Fragment (DONT-FRAGMENT)
- Bandwidth negotiation (BANDWIDTH)

**Impact**: Sufficient for basic TURN relay testing. Not production-ready for WebRTC at scale.

## Limitations

### Current Limitations

1. **No Actual Data Relay**
    - Allocation tracking works
    - SendIndication/DataIndication events generated
    - But data not forwarded between client and peer
    - **Status**: Relay forwarding logic pending

2. **No TCP Support**
    - Only UDP relay addresses
    - RFC 8656 allows TCP allocations (REQUESTED-TRANSPORT)

3. **No Authentication**
    - Anyone can allocate
    - Production TURN servers require credentials (to prevent abuse)

4. **No IPv6**
    - Only IPv4 relay addresses
    - REQUESTED-ADDRESS-FAMILY (0x0017) not supported

5. **No Channel Binding**
    - All data uses SendIndication/DataIndication (higher overhead)
    - Channel Binding reduces header size for frequent peer communication

6. **Simple Allocation IDs**
    - Currently uses transaction ID as allocation ID
    - Security issue: predictable IDs
    - Should use random UUIDs

### Security Considerations

**Open Relay Risk**: Without authentication, anyone can allocate relay addresses and consume server resources.

**Resource Exhaustion**: No limits on allocations per client. Attacker could exhaust relay address pool.

**Amplification Attack**: TURN can amplify traffic (client sends 1 packet, server relays to N peers). Requires rate
limiting.

## Performance Considerations

**Stateful Overhead**: Each allocation consumes memory. Typical deployment: 10,000-100,000 concurrent allocations.

**Cleanup Task**: Periodic cleanup adds O(N) cost every 30 seconds (N = number of allocations). Acceptable for <10,000
allocations.

**LLM Latency**: 500ms-5s per allocation request. Acceptable for TURN (allocations long-lived, not latency-sensitive).

**Relay Throughput**: Once implemented, relay adds ~1ms forwarding overhead per packet. Thousands of packets/second
possible.

## Example Prompts

### Basic TURN Relay

```
Start a TURN relay server on port 0 with 600 second allocations. Assign relay
addresses from 203.0.113.0/24 pool.
```

### TURN with Short Lifetimes (Testing)

```
Start a TURN relay server on port 0 with very short 5 second allocation lifetime.
```

### TURN Rejecting Allocations

```
Start a TURN relay server on port 0 that rejects all allocations with error 508
Insufficient Capacity.
```

### TURN with Permission Tracking

```
Start a TURN relay server on port 0 that tracks all CreatePermission requests and
logs which peers are permitted for each allocation.
```

### TURN with Automatic Refresh

```
Start a TURN relay server on port 0. When clients send Refresh requests, always
extend lifetime by 600 seconds.
```

## Use Cases

### WebRTC Fallback

**Typical ICE Flow**:

1. Try direct connection (STUN reveals public IP)
2. If symmetric NAT or firewall blocks direct connection → Use TURN relay
3. All media flows through TURN server (adds latency and bandwidth cost)

### Gaming (P2P Multiplayer)

**When Direct Connection Fails**:

- Players behind symmetric NAT can't establish P2P
- TURN relays game state updates
- Higher latency than direct, but still playable for turn-based games

### VoIP (SIP/RTP)

**Last Resort for Audio/Video**:

- Direct RTP connection preferred (low latency)
- TURN relay used when firewalls block UDP or symmetric NAT prevents hole punching

## Integration with STUN

**TURN is STUN Extension**:

- Uses same message format (STUN header)
- Uses same magic cookie (0x2112A442)
- Shares attribute format (type-length-value)
- Can coexist on same port (server distinguishes by method field)

**Unified Server**: NetGet could run STUN + TURN on same UDP port. Method field (Binding=1, Allocate=3) disambiguates.

## References

- RFC 8656: Traversal Using Relays around NAT (TURN)
- RFC 8489: Session Traversal Utilities for NAT (STUN)
- RFC 5766: Traversal Using Relays around NAT (obsolete TURN)
- RFC 5766bis: TURN Extensions (various RFCs)
- WebRTC TURN Usage: https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection
- TURN Protocol Specification: https://datatracker.ietf.org/doc/html/rfc8656
