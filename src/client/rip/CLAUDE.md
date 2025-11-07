# RIP Client Implementation

## Overview

RIP (Routing Information Protocol) client implementation for querying routing tables from RIP routers. Supports both RIPv1 and RIPv2.

## Protocol Details

- **Transport**: UDP port 520
- **Versions**: RIPv1 and RIPv2
- **Use Case**: Query routing tables, analyze routes, debug RIP networks
- **Authentication**: RIPv2 supports authentication (not yet implemented)

## Library Choices

**UDP Socket**: `tokio::net::UdpSocket`
- Standard async UDP socket
- No external dependencies
- Direct packet construction/parsing

**Why no external RIP library?**
- RIP is a simple protocol (4-byte header + 20-byte route entries)
- No mature Rust RIP client libraries
- Custom implementation allows full LLM control

## Architecture

### Packet Structure

**RIP Message** (4 + N*20 bytes):
```
0                   1                   2                   3
0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Command (1)  |  Version (1)  |      Must be zero (2)         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         Address Family (2)    |      Route Tag (2)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       IP Address (4)                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Subnet Mask (4)                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Next Hop (4)                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Metric (4)                             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
... (up to 25 route entries per packet)
```

**Command Types**:
- `1` = Request (query routing table)
- `2` = Response (routing table data)

**Version**:
- `1` = RIPv1 (no subnet masks, classful routing)
- `2` = RIPv2 (CIDR support, authentication)

**Metric**:
- Hop count (1-15)
- `16` = Infinity (unreachable)

### Connection Model

```
┌──────────────┐
│ RIP Client   │
│              │
│ UDP Socket   │
│   (port X)   │
└──────┬───────┘
       │
       │ UDP packets
       │ (port 520)
       ▼
┌──────────────┐
│ RIP Router   │
│  (router)    │
│              │
└──────────────┘
```

**Flow**:
1. Client binds to any available port (not 520, which requires root)
2. Client sends RIP Request to router:520
3. Router sends RIP Response with routing table
4. Client parses routes and calls LLM
5. LLM can analyze routes, send more requests, or disconnect

### State Machine

**Connection States**:
- **Idle**: No LLM call in progress, ready to process responses
- **Processing**: LLM call active, queue incoming responses
- **Accumulating**: Continue queuing responses until LLM completes

**State Transitions**:
```
Idle ─(response received)─> Processing
Processing ─(more responses)─> Accumulating
Accumulating ─(more responses)─> Accumulating
Processing/Accumulating ─(LLM complete)─> Idle
```

This prevents concurrent LLM calls for the same client.

## LLM Integration

### Events

**rip_connected**
- Triggered when client binds UDP socket
- LLM decides to send initial request or wait

**rip_response_received**
- Triggered when routing table response arrives
- LLM analyzes routes, decides next action

### Actions

**Async Actions** (user-triggered):
- `send_rip_request(version)` - Query routing table (RIPv1 or RIPv2)
- `disconnect()` - Close client

**Sync Actions** (in response to events):
- `send_rip_request(version)` - Send follow-up query
- `wait_for_more()` - Queue responses, wait for more data

### Example LLM Flow

```
1. User: "Query RIP router at 192.168.1.1"
   └─> open_client("rip", "192.168.1.1:520", "Query routing table")

2. Event: rip_connected
   └─> LLM Action: send_rip_request(version=2)

3. Client sends RIP Request (entire routing table)
   └─> Router responds with N routes

4. Event: rip_response_received
   - routes: [
       { ip: "10.0.0.0", mask: "255.255.255.0", next_hop: "192.168.1.254", metric: 2 },
       { ip: "172.16.0.0", mask: "255.255.0.0", next_hop: "192.168.1.253", metric: 5 },
       ...
     ]
   └─> LLM analyzes routes, sends status update

5. LLM Action: disconnect()
   └─> Client closes
```

## RIPv1 vs RIPv2

### RIPv1
- **Classful routing**: No subnet masks (inferred from IP class)
- **No authentication**: Open protocol
- **Broadcast**: 255.255.255.255
- **Fields**: Only `ip_address` and `metric` used

### RIPv2
- **CIDR support**: Subnet masks included
- **Authentication**: MD5 or simple password (not yet implemented)
- **Multicast**: 224.0.0.9
- **Additional fields**: `route_tag`, `subnet_mask`, `next_hop`

## Dual Logging

All logs use tracing macros AND send to TUI:
```rust
info!("RIP client {} received response", client_id);
let _ = status_tx.send(format!("[CLIENT] RIP response: {} routes", route_count));
```

**Log Levels**:
- `ERROR`: Failed to parse RIP message, socket errors
- `WARN`: (none currently)
- `INFO`: Connection lifecycle, request/response summary
- `DEBUG`: Request/response details
- `TRACE`: Raw packet data, state transitions

## Limitations

### Current Limitations

1. **No Authentication**: RIPv2 authentication (MD5, simple password) not implemented
2. **No Sending**: Client only sends requests, cannot announce routes
3. **No Triggered Updates**: Cannot request specific routes (only entire table)
4. **Requires Port 520 Access**: Target router must listen on port 520
5. **UDP Only**: No TCP support (RIP is UDP-only anyway)

### Security Considerations

- **No Validation**: Client trusts all received routes
- **Routing Attacks**: Malicious router could send fake routes
- **Metric Manipulation**: Router could lie about metrics
- **Use Case**: Testing/monitoring only, NOT for production routing

### Future Enhancements

1. **RIPv2 Authentication**: MD5 and simple password support
2. **Specific Route Queries**: Request individual routes (non-standard)
3. **Route Filtering**: Filter responses by prefix/metric
4. **Split Horizon**: Detect split horizon violations
5. **Poison Reverse**: Detect poison reverse issues
6. **Triggered Updates**: Support RIP triggered updates

## Testing Strategy

See `tests/client/rip/CLAUDE.md` for E2E testing approach.

**Mock Router**: Simple UDP server responding with fake routing table
**Real Router**: Test against Quagga/FRRouting RIP daemon (if available)

## References

- **RFC 1058**: RIPv1 specification
- **RFC 2453**: RIPv2 specification
- **RFC 4822**: RIPv2 Cryptographic Authentication

## Example Prompts

1. "Query RIP router at 192.168.1.1 for routing table"
2. "Connect to RIP at 10.0.0.1:520 using RIPv2 and analyze routes with metric < 5"
3. "Query RIP router and show all routes to 172.16.0.0/12 networks"
