# TURN Client Implementation

## Overview

TURN (Traversal Using Relays around NAT) client implementing RFC 8656. Connects to TURN servers to obtain relay addresses for NAT traversal when direct peer-to-peer connections fail.

**Compliance**: RFC 8656 (TURN), RFC 8489 (STUN)

**Protocol Purpose**: TURN relays traffic between peers when direct connection is impossible due to restrictive NATs or firewalls. Essential fallback for WebRTC, VoIP, and real-time communication.

## Library Choices

**Manual Implementation** - Complete TURN client protocol built on STUN message format
- **Why**: Full control over TURN client behavior for LLM integration
- No mature Rust TURN client libraries with async/await support
- Manual implementation allows custom LLM action integration

**Extends STUN**:
- Uses STUN message format (20-byte header + attributes)
- TURN methods: Allocate (3), Refresh (4), CreatePermission (8), SendIndication (6), DataIndication (7)
- UDP transport (default TURN port 3478)

## Architecture Decisions

### UDP-Based Client

TURN uses UDP for control and data:
- Single UDP socket bound to random port
- All TURN messages sent to/from server address
- Responses matched by transaction ID

**Connection Flow**:
1. Bind UDP socket to `0.0.0.0:0` (random port)
2. Send connected event to LLM
3. LLM can trigger Allocate request
4. Receive Allocate Response with relay address
5. Create permissions for peers
6. Send/receive data via relay using SendIndication/DataIndication

### State Machine

**Per-Client State** (`ClientData`):
```rust
struct ClientData {
    state: ConnectionState,           // Idle/Processing/Accumulating
    queued_events: Vec<Event>,        // Events queued during Processing
    memory: String,                   // LLM conversation memory
    relay_address: Option<SocketAddr>, // Allocated relay address
}
```

**State Transitions**:
- **Idle**: No LLM call in progress, process events immediately
- **Processing**: LLM call active, queue incoming events
- **Accumulating**: Continue queuing events until LLM returns

This prevents concurrent LLM calls on the same client.

### TURN Message Construction

All TURN messages follow STUN format:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|0 0|     STUN Message Type     |         Message Length        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Magic Cookie                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
|                     Transaction ID (96 bits)                  |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Attributes (TLV)                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

**Message Types (Client → Server)**:
- `0x0003`: Allocate Request
- `0x0004`: Refresh Request
- `0x0008`: CreatePermission Request
- `0x0016`: SendIndication
- `0x0017`: DataIndication

**Key Attributes**:
- `0x000D`: LIFETIME (32-bit seconds)
- `0x0012`: XOR-PEER-ADDRESS (XOR'd peer IP:port)
- `0x0013`: DATA (relay payload)
- `0x0016`: XOR-RELAYED-ADDRESS (allocated relay address)
- `0x0019`: REQUESTED-TRANSPORT (UDP = 17)

### XOR Address Encoding

TURN uses XOR'd addresses for privacy (prevents middle-box tampering):

**XOR-MAPPED-ADDRESS/XOR-RELAYED-ADDRESS/XOR-PEER-ADDRESS**:
- Port: `xor_port = port ^ 0x2112`
- IPv4: `xor_ip[i] = ip[i] ^ magic_cookie[i]` (magic cookie = 0x2112A442)
- IPv6: `xor_ip[0..3] = ip[0..3] ^ magic_cookie[0..3]`, `xor_ip[4..15] = ip[4..15] ^ transaction_id[0..11]`

The client must XOR-decode relay addresses from responses and XOR-encode peer addresses in requests.

## LLM Integration

### Action-Based Control

**Available Actions**:

1. **allocate_turn_relay**: Request relay address allocation
   ```json
   {
     "type": "allocate_turn_relay",
     "lifetime_seconds": 600
   }
   ```

2. **create_permission**: Grant permission for peer to send/receive
   ```json
   {
     "type": "create_permission",
     "peer_address": "192.168.1.100:5000"
   }
   ```

3. **send_turn_data**: Send data to peer via relay
   ```json
   {
     "type": "send_turn_data",
     "peer_address": "192.168.1.100:5000",
     "data_hex": "48656c6c6f"
   }
   ```

4. **refresh_allocation**: Extend or delete allocation
   ```json
   {
     "type": "refresh_allocation",
     "lifetime_seconds": 600  // 0 = delete
   }
   ```

5. **disconnect**: Disconnect from TURN server
   ```json
   {
     "type": "disconnect"
   }
   ```

### Event Types

**TURN_CLIENT_CONNECTED_EVENT**:
- **Triggered**: When UDP socket bound and ready
- **Context**: `remote_addr` (TURN server address)
- **LLM Action**: Typically triggers `allocate_turn_relay`

**TURN_CLIENT_ALLOCATED_EVENT**:
- **Triggered**: Allocate Response received
- **Context**: `relay_address`, `lifetime_seconds`, `transaction_id`
- **LLM Action**: May trigger `create_permission` for known peers

**TURN_CLIENT_DATA_RECEIVED_EVENT**:
- **Triggered**: DataIndication received from peer
- **Context**: `peer_address`, `data_hex`, `data_length`
- **LLM Action**: May trigger `send_turn_data` response

**TURN_CLIENT_PERMISSION_CREATED_EVENT**:
- **Triggered**: CreatePermission Response received
- **Context**: `peer_address`
- **LLM Action**: Confirmation that peer can now send/receive

**TURN_CLIENT_REFRESHED_EVENT**:
- **Triggered**: Refresh Response received
- **Context**: `lifetime_seconds`
- **LLM Action**: Schedule next refresh or proceed with data

### Example LLM Flow

**User Instruction**: "Connect to TURN server at localhost:3478, allocate a relay, and grant permission for peer 192.168.1.100:5000"

**LLM Behavior**:
1. **Connected Event** → LLM generates `allocate_turn_relay` action
2. **Allocated Event** (relay: 203.0.113.5:54321) → LLM generates `create_permission` for 192.168.1.100:5000
3. **Permission Created Event** → LLM confirms "Relay ready at 203.0.113.5:54321, peer 192.168.1.100:5000 permitted"
4. **Data Received Event** (from peer) → LLM processes and may generate `send_turn_data` response

## Limitations

### Current Limitations

1. **No Long-Term Credentials**
   - No authentication support (MESSAGE-INTEGRITY, REALM, NONCE)
   - Cannot connect to production TURN servers requiring auth
   - **Impact**: Works with NetGet TURN server, not public TURN services

2. **IPv4 Only**
   - XOR address encoding supports both IPv4 and IPv6
   - But no REQUESTED-ADDRESS-FAMILY attribute
   - **Impact**: Cannot request specific address family

3. **UDP Transport Only**
   - No TCP or TLS allocations
   - REQUESTED-TRANSPORT always UDP (17)
   - **Impact**: Cannot use TURN-TCP or TURN-TLS variants

4. **No Channel Binding**
   - All data uses SendIndication/DataIndication (16+ byte overhead)
   - ChannelBind/ChannelData not implemented (lower overhead)
   - **Impact**: Higher bandwidth usage for frequent peer communication

5. **No Automatic Refresh Scheduling**
   - LLM must manually trigger refresh before expiration
   - No background task to keep allocation alive
   - **Impact**: Allocation may expire if LLM forgets to refresh

6. **Simple Transaction ID Matching**
   - Responses matched only by parsing message type
   - No transaction ID correlation for request/response pairing
   - **Impact**: May misattribute responses in high-traffic scenarios

### Security Considerations

**No Message Integrity**: Without MESSAGE-INTEGRITY, server cannot verify client authenticity. Open relay abuse risk.

**Predictable Transaction IDs**: Uses `rand::random()` for transaction IDs. Secure for testing, not cryptographically strong.

**No TLS**: Control messages sent in cleartext over UDP. Vulnerable to eavesdropping.

## Performance Considerations

**Latency**: TURN adds relay hop latency (~10-50ms depending on server location). Acceptable for non-real-time apps, noticeable for VoIP/gaming.

**Bandwidth Overhead**: Each relayed packet adds STUN headers:
- SendIndication: ~36 bytes overhead (20 STUN + 8 XOR-PEER-ADDRESS + 8 DATA header)
- DataIndication: ~36 bytes overhead

**LLM Latency**: 500ms-5s per action. Acceptable for TURN (allocations long-lived, not latency-sensitive).

**UDP Reliability**: TURN uses UDP. Lost allocate/refresh requests must be retried by LLM. No automatic retries.

## Example Prompts

### Basic TURN Allocation
```
Connect to TURN server at localhost:3478 and allocate a relay address for 600 seconds.
```

### TURN with Peer Permission
```
Connect to TURN server at localhost:3478, allocate relay, and grant permission for peer 192.168.1.100:5000.
```

### TURN Data Relay
```
Connect to TURN server at localhost:3478, allocate relay, create permission for peer 192.168.1.100:5000, and when data arrives, echo it back.
```

### TURN with Manual Refresh
```
Connect to TURN server at localhost:3478, allocate relay with 60 second lifetime, and refresh every 45 seconds to keep it alive.
```

## Use Cases

### WebRTC Fallback

**Typical ICE Flow**:
1. Try direct connection via STUN (reveals public IP)
2. If symmetric NAT blocks direct → Use TURN relay
3. All media flows through TURN (adds latency and bandwidth cost)

**NetGet TURN Client Use**:
- Test WebRTC TURN fallback behavior
- Simulate restricted NAT scenarios
- Verify TURN server allocation policies

### P2P Gaming

**When Direct Connection Fails**:
- Players behind symmetric NAT use TURN relay
- Game state updates relayed through server
- Higher latency than direct, but enables connectivity

### VoIP (SIP/RTP)

**Last Resort for Audio/Video**:
- Direct RTP preferred (low latency)
- TURN relay when firewalls block or symmetric NAT prevents hole punching

## Integration with NetGet TURN Server

**Client/Server Testing**:
1. Start NetGet TURN server: `open_server turn 0`
2. Start NetGet TURN client: `open_client turn <server_addr>`
3. LLM controls both sides for allocation policy testing

**Relay Testing**:
- Allocate relay from NetGet server
- Create permissions
- Send data via SendIndication
- Verify DataIndication delivery (when server relay logic complete)

## References

- RFC 8656: Traversal Using Relays around NAT (TURN)
- RFC 8489: Session Traversal Utilities for NAT (STUN)
- WebRTC TURN Usage: https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection
- TURN Protocol Specification: https://datatracker.ietf.org/doc/html/rfc8656
