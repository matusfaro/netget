# ICMP Client Implementation

## Overview

The ICMP (Internet Control Message Protocol) client implementation provides network-layer diagnostic capabilities, primarily for ping and traceroute functionality. The client sends ICMP Echo Requests and processes replies with full LLM control.

## Library Choices

### Raw Socket Implementation
- **socket2** (v0.5) - Raw IP socket creation
  - Required for ICMP protocol access (IP protocol 1)
  - Requires `CAP_NET_RAW` capability or root access
  - Non-blocking I/O for async integration

### Packet Handling
- **pnet_packet** (v0.34) - ICMP packet construction and parsing
  - Echo Request/Reply packet types
  - Destination Unreachable parsing
  - Time Exceeded parsing (for traceroute)
  - Automatic checksum calculation

### Async Runtime
- **tokio** - Async runtime integration
  - Async receive loop
  - LLM integration
  - State machine management

## Architecture

### Protocol Pattern
Follows the pattern of **UDP client** and **TCP client**:
1. Raw ICMP socket creation
2. Send Echo Requests based on LLM actions
3. Receive loop processing replies
4. State machine prevents concurrent LLM calls
5. RTT (Round-Trip Time) measurement

### Connection Model
**Connectionless** - Like UDP:
- No persistent connection
- Each request/reply pair is independent
- Pending request tracking for RTT calculation
- Timeout handling for lost packets

### Socket Configuration
```rust
Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
```
- Receives ICMP replies destined for this host
- Requires elevated privileges (`CAP_NET_RAW` or root)
- Non-blocking mode with async polling

### Packet Flow
1. **LLM Decision**: LLM returns `send_echo_request` action
2. **Build**: Construct ICMP Echo Request with IP header
3. **Send**: Send via raw socket to destination
4. **Track**: Store pending request with timestamp
5. **Receive**: Async loop receives ICMP replies
6. **Match**: Match reply to pending request (identifier + sequence)
7. **Calculate RTT**: Measure time delta
8. **LLM Event**: Call LLM with echo_reply event
9. **Execute**: LLM decides next action (send more, disconnect, etc.)

## LLM Integration

### Events

#### ICMP Connected
```json
{
  "event_type": "icmp_connected",
  "local_addr": "0.0.0.0:0",
  "target_ip": "8.8.8.8"
}
```

#### ICMP Echo Reply (Ping Response)
```json
{
  "event_type": "icmp_echo_reply",
  "source_ip": "8.8.8.8",
  "identifier": 1234,
  "sequence": 1,
  "rtt_ms": 15,
  "ttl": 56,
  "payload_hex": "48656c6c6f"
}
```

#### ICMP Timeout
```json
{
  "event_type": "icmp_timeout",
  "destination_ip": "192.168.1.100",
  "identifier": 1234,
  "sequence": 1
}
```

#### ICMP Destination Unreachable
```json
{
  "event_type": "icmp_destination_unreachable",
  "source_ip": "192.168.1.1",
  "code": 1  // 0=net, 1=host, 2=protocol, 3=port
}
```

#### ICMP Time Exceeded (Traceroute)
```json
{
  "event_type": "icmp_time_exceeded",
  "source_ip": "10.0.0.1",  // Hop address
  "code": 0  // 0=TTL exceeded, 1=fragment reassembly
}
```

### Actions

#### Send Echo Request (Ping)
```json
{
  "type": "send_echo_request",
  "destination_ip": "8.8.8.8",
  "identifier": 1234,
  "sequence": 1,
  "payload_hex": "48656c6c6f",
  "ttl": 64
}
```

#### Send Timestamp Request
```json
{
  "type": "send_timestamp_request",
  "destination_ip": "192.168.1.1",
  "identifier": 5678,
  "sequence": 1
}
```

#### Wait for More Responses
```json
{
  "type": "wait_for_more"
}
```

#### Disconnect
```json
{
  "type": "disconnect"
}
```

## Use Cases

### Basic Ping
```
"Ping 8.8.8.8 five times and report average latency"
```

LLM logic:
1. Send 5 echo requests (seq 1-5)
2. Receive replies, calculate RTT for each
3. Average the RTT values
4. Report results
5. Disconnect

### Traceroute
```
"Perform traceroute to example.com"
```

LLM logic:
1. Send echo request with TTL=1
2. Receive Time Exceeded from first hop
3. Send echo request with TTL=2
4. Receive Time Exceeded from second hop
5. Continue until Echo Reply (destination reached)
6. Report path

### Latency Monitoring
```
"Ping 1.1.1.1 every second and alert if latency > 100ms"
```

LLM logic:
1. Send echo request
2. Receive reply, check RTT
3. If RTT > 100ms, alert user
4. Wait 1 second
5. Repeat

## Pending Request Tracking

### Transaction ID
ICMP uses `(identifier, sequence)` tuple to match requests to replies:
- **Identifier**: Usually process ID or random value
- **Sequence**: Incrementing counter per ping session

### RTT Calculation
```rust
struct PendingRequest {
    sent_at: Instant,
    identifier: u16,
    sequence: u16,
    destination_ip: Ipv4Addr,
}

// When reply arrives:
let rtt_ms = req.sent_at.elapsed().as_millis();
```

### Timeout Handling
- Pending requests tracked in HashMap
- TODO: Implement timeout timer (e.g., 5 seconds)
- If no reply after timeout, fire `icmp_timeout` event
- LLM decides whether to retry or give up

## Limitations

### Privilege Requirements
**CRITICAL**: Requires root or `CAP_NET_RAW` capability
- Cannot run in unprivileged environments
- Not available in Claude Code for Web (sandboxed)
- Testing requires elevated permissions

### Platform Considerations
- **Linux**: Full support with raw sockets
- **macOS**: Full support (requires sudo)
- **Windows**: Limited support (may require additional configuration)

### Kernel Interaction
- Kernel may intercept ICMP Echo Replies before userspace
- Some ICMP types filtered by firewall
- May need to configure system to allow raw ICMP

### IPv6 Support
Not yet implemented:
- Current implementation only supports ICMPv4
- ICMPv6 uses different socket (Protocol::ICMPV6)
- ICMPv6 message types differ

### Timestamp Requests
Not fully implemented:
- Action defined but not executed
- Would require timestamp packet construction
- Future enhancement

## State Machine

### Client Connection States
1. **Idle** - No LLM processing
2. **Processing** - LLM call in progress
3. **Accumulating** - Data queued during processing

Pattern prevents concurrent LLM calls on same client.

## Performance

### Latency Breakdown
- Packet construction: < 1ms
- Raw socket send: < 1ms
- Network RTT: 1-500ms (network dependent)
- Packet parsing: < 0.1ms
- LLM call: 100-500ms (dominant factor for next action)

### Throughput
- Can send multiple pings in quick succession
- Limited by LLM response time for adaptive logic
- Scripting mode could bypass LLM for predictable patterns

## Security Considerations

### Legitimate Uses
- Network diagnostics (ping, traceroute)
- Latency monitoring
- Reachability testing
- Network path discovery

### Potential Misuse
- ⚠️ ICMP flood attacks (DoS)
- ⚠️ Network scanning without authorization
- ⚠️ Covert channels

### Safeguards
- No default flooding behavior
- LLM controls send rate
- Requires explicit root access
- Documentation emphasizes authorized use only

## Example Prompts

### Simple Ping
```
"Ping 8.8.8.8 three times"
```

### Latency Test
```
"Ping cloudflare DNS (1.1.1.1) and Google DNS (8.8.8.8), compare latency"
```

### Traceroute
```
"Trace route to github.com by incrementing TTL"
```

### Conditional Logic
```
"Ping 192.168.1.1 until it responds, then alert me"
```

## Testing Notes

See `tests/client/icmp/CLAUDE.md` for test strategy and E2E test details.

## Future Enhancements

1. **Timeout Timer**: Automatic timeout for pending requests
2. **IPv6 Support**: ICMPv6 with Neighbor Discovery
3. **Timestamp Requests**: Full implementation
4. **Bulk Ping**: Send multiple requests concurrently
5. **Statistics**: Min/max/avg/stddev RTT calculation
6. **Packet Loss**: Track sent vs received ratio

## References

- RFC 792 - Internet Control Message Protocol (ICMP)
- RFC 1122 - Requirements for Internet Hosts
- pnet documentation: https://docs.rs/pnet/latest/pnet/
- socket2 documentation: https://docs.rs/socket2/latest/socket2/
