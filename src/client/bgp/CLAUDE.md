# BGP Client Implementation

## Overview

This BGP client connects to BGP peers to query routing information in passive monitoring mode. It implements RFC 4271 (
Border Gateway Protocol 4) for session establishment and message exchange, but focuses on querying and monitoring rather
than active route announcement.

## Library Choices

- **Manual Protocol Implementation**: BGP wire protocol is implemented manually for full LLM control
- **No External BGP Library**: Uses raw TCP sockets with custom message encoding/decoding
- **Standard Library Only**: Relies on Tokio for async I/O, no specialized BGP crates needed

### Why Manual Implementation?

The BGP client uses a manual implementation rather than an existing BGP library because:

1. **LLM Control**: Full control over message construction and FSM state transitions
2. **Query Mode**: Only needs OPEN, KEEPALIVE, UPDATE, and NOTIFICATION handling
3. **Simplicity**: BGP message format is straightforward (fixed header + variable body)
4. **Learning**: Peer routing information without implementing full routing table (RIB)

## Architecture

### Connection Model

The BGP client follows a standard TCP-based connection model:

```
User → open_client → TCP connection → BGP OPEN → BGP session established
                                          ↓
                                    LLM Integration
```

### BGP Session States

The client implements a simplified BGP FSM (Finite State Machine):

1. **Connect**: TCP connection established
2. **OpenSent**: Sent BGP OPEN message
3. **OpenConfirm**: Received peer's OPEN, sent KEEPALIVE
4. **Established**: Session established, can exchange UPDATEs

### Message Flow

```
Client                           Peer
  |                               |
  |--- TCP SYN ------------------>|
  |<-- TCP SYN-ACK ---------------|
  |--- TCP ACK ------------------>|
  |                               |
  |--- BGP OPEN ----------------->|  (local_as, router_id, hold_time)
  |<-- BGP OPEN ------------------|  (peer_as, peer_router_id, hold_time)
  |--- BGP KEEPALIVE ------------>|
  |<-- BGP KEEPALIVE -------------|
  |                               |
  |<-- BGP UPDATE ----------------|  (routing updates)
  |<-- BGP UPDATE ----------------|
  |<-- BGP KEEPALIVE -------------|
  |--- BGP KEEPALIVE ------------>|
  |                               |
  |--- BGP NOTIFICATION --------->|  (disconnect)
  |<-- TCP FIN -------------------|
```

### LLM Integration

The client uses a state machine to prevent concurrent LLM calls:

- **Idle**: No LLM processing, ready to handle events
- **Processing**: LLM is analyzing an event
- **Accumulating**: LLM is busy, queuing events

Events trigger LLM calls:

- `bgp_connected` - Peer session established (after OPEN handshake)
- `bgp_update_received` - Route announcement/withdrawal received
- `bgp_notification_received` - Error notification received

The LLM can respond with actions:

- `send_keepalive` - Send KEEPALIVE message
- `send_notification` - Send NOTIFICATION and close
- `disconnect` - Gracefully close connection
- `wait_for_more` - Wait for more messages

### Connection State Management

Per-client state tracked:

```rust
struct ClientData {
    state: ConnectionState,        // Idle/Processing/Accumulating
    bgp_state: BgpState,            // Connect/OpenSent/OpenConfirm/Established
    queued_data: Vec<u8>,           // Queued events during Processing
    memory: String,                 // LLM conversation memory
    peer_as: Option<u32>,           // Peer AS number
    peer_router_id: Option<String>, // Peer router ID
    hold_time: u16,                 // Negotiated hold time
}
```

### Read Loop

The read loop handles incoming BGP messages:

1. Read BGP header (19 bytes): marker (16) + length (2) + type (1)
2. Validate marker (all 0xFF)
3. Parse message type (OPEN, KEEPALIVE, UPDATE, NOTIFICATION)
4. Read message body (length - 19 bytes)
5. Handle message based on type
6. Call LLM with appropriate event
7. Execute LLM-generated actions

### Logging

Dual logging strategy:

- **Tracing macros**: `info!()`, `debug!()`, `trace!()`, `error!()` → `netget.log`
- **Status channel**: `status_tx.send()` → TUI display

Log levels:

- `ERROR`: Protocol violations, connection errors
- `INFO`: Session establishment, state transitions
- `DEBUG`: KEEPALIVE messages, state changes
- `TRACE`: Raw message types, byte counts

## BGP Message Format

All BGP messages use a common header:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                         Marker (16 bytes)                     +
|                          (all ones)                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          Length               |      Type     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### OPEN Message

Sent immediately after TCP connection:

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Version (4) | My AS (16-bit) | Hold Time  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          BGP Identifier (Router ID)       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Opt Param Len (0) |
+-+-+-+-+-+-+-+-+-+-+
```

### KEEPALIVE Message

Just the header, no body (19 bytes total).

### UPDATE Message

Contains withdrawn routes, path attributes, and NLRI (Network Layer Reachability Information). The client receives these
but does not parse them fully - it passes the hex-encoded data to the LLM for analysis.

### NOTIFICATION Message

Contains error code and subcode:

```
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Error Code | Error Subcode  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          Data (variable)    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

## Startup Parameters

The client accepts these startup parameters:

- `local_as` (integer, optional): Local AS number (default: 65000, private ASN)
- `router_id` (string, optional): BGP router ID in IPv4 format (default: "192.168.1.100")
- `hold_time` (integer, optional): BGP hold time in seconds (default: 180)

Example:

```json
{
  "local_as": 65001,
  "router_id": "192.168.1.50",
  "hold_time": 120
}
```

## Use Cases

This BGP client is designed for:

1. **Route Monitoring**: Query peer for advertised routes
2. **AS Path Analysis**: Analyze routing paths through UPDATE messages
3. **BGP Debugging**: Test BGP peer behavior
4. **Route Learning**: Passive learning of routing information
5. **Network Reconnaissance**: Discover network topology via BGP

**NOT designed for:**

- Active route announcement (use BGP server for that)
- Full routing table (RIB) maintenance
- Route propagation to other peers
- Production BGP peering

## Limitations

1. **No RIB**: Client does not maintain a Routing Information Base
2. **No Route Filtering**: Accepts all UPDATE messages from peer
3. **No BGP Extensions**: No support for capabilities negotiation, route refresh, etc.
4. **Simplified FSM**: Only 4 states (full BGP FSM has 6)
5. **16-bit AS Numbers**: AS numbers truncated to 16-bit (no 32-bit ASN support)
6. **No Authentication**: No MD5 or TCP-AO authentication support
7. **Query Mode Only**: Passive monitoring, not active routing

## Security Considerations

- **Privileged Port**: BGP uses port 179, may require elevated privileges to bind (client typically connects, not binds)
- **AS Number Spoofing**: Client can use fake AS number for monitoring (use private ASNs 64512-65534)
- **Route Injection**: This client does not announce routes, only receives them
- **Denial of Service**: No protection against malicious UPDATE flooding

## Error Handling

BGP errors are handled via NOTIFICATION messages:

- **Protocol Violations**: Invalid marker, bad message length, unsupported version
- **Connection Errors**: Unexpected EOF, read/write failures
- **FSM Errors**: Messages received in wrong state

All errors result in connection termination and client status update to `Error(message)`.

## Performance

- **Memory**: Minimal, no route storage
- **CPU**: Low, simple message parsing
- **Network**: Depends on peer's UPDATE frequency
- **LLM Calls**: 1 call per event (OPEN, UPDATE, NOTIFICATION)

## Testing

See `tests/client/bgp/CLAUDE.md` for testing strategy.

## Future Enhancements

Potential improvements (not currently implemented):

1. **Route Parsing**: Parse UPDATE messages into prefix/AS path/attributes
2. **BGP Capabilities**: Support capabilities negotiation
3. **32-bit ASN**: Support 4-byte AS numbers
4. **Route Refresh**: Request route table from peer
5. **Graceful Restart**: Maintain routes during restart
6. **BGP Authentication**: MD5 or TCP-AO support
7. **Route Filtering**: Filter UPDATE messages by prefix/AS path

## References

- RFC 4271: A Border Gateway Protocol 4 (BGP-4)
- RFC 4760: Multiprotocol Extensions for BGP-4
- RFC 6793: BGP Support for Four-Octet Autonomous System (AS) Number Space
