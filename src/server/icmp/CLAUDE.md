# ICMP Server Implementation

## Overview

The ICMP (Internet Control Message Protocol) server implementation provides a network-layer protocol server that can capture and respond to ICMP messages. ICMP is primarily used for network diagnostics, error reporting, and control messages.

## Library Choices

### Raw Socket Implementation
- **socket2** (v0.5) - Raw IP socket creation and management
  - Provides cross-platform raw socket support
  - Required for ICMP protocol access (IP protocol 1)
  - Requires `CAP_NET_RAW` capability or root access

### Packet Handling
- **pnet_packet** (v0.34) - ICMP packet parsing and construction
  - Comprehensive ICMP packet types (Echo, Destination Unreachable, Time Exceeded, Timestamp, etc.)
  - Automatic checksum calculation
  - Type-safe packet builders and parsers
  - Already used in ARP and DataLink protocols

### Async Runtime
- **tokio** - Async runtime integration
  - `tokio::task::spawn_blocking` for raw socket operations
  - Async LLM integration while maintaining blocking I/O

## Architecture

### Protocol Pattern
Follows the same pattern as **ARP server** (`src/server/arp/mod.rs`):
1. Raw socket creation in blocking context
2. Receive loop with packet parsing
3. LLM integration via async tasks
4. Separate send socket to avoid conflicts

### Connection Model
**Connectionless** - Like UDP and ARP:
- No persistent connections
- Each ICMP message triggers LLM independently
- No state machine per connection
- "Connection" tracking is per-packet for TUI display

### Socket Configuration
```rust
Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
```
- Receives all ICMP packets destined for this host
- Requires elevated privileges (`CAP_NET_RAW` or root)
- Non-blocking mode with timeout for graceful shutdown

### Packet Flow
1. **Receive**: Raw socket receives IP packets
2. **Parse**: Extract IPv4 header, validate ICMP protocol
3. **Identify**: Determine ICMP message type (Echo, Timestamp, etc.)
4. **Event**: Build appropriate event with structured data
5. **LLM**: Call LLM with event, get actions
6. **Execute**: Construct and send ICMP reply packets
7. **Send**: Use separate raw socket to send responses

## LLM Integration

### Events

#### ICMP Echo Request (Ping)
```json
{
  "event_type": "icmp_echo_request",
  "source_ip": "192.168.1.50",
  "destination_ip": "192.168.1.100",
  "identifier": 1234,
  "sequence": 1,
  "payload_hex": "48656c6c6f",
  "ttl": 64
}
```

#### ICMP Timestamp Request
```json
{
  "event_type": "icmp_timestamp_request",
  "source_ip": "192.168.1.50",
  "destination_ip": "192.168.1.100",
  "identifier": 5678,
  "sequence": 1,
  "originate_timestamp": 12345678
}
```

#### Other ICMP Messages
```json
{
  "event_type": "icmp_other_message",
  "source_ip": "192.168.1.50",
  "destination_ip": "192.168.1.100",
  "icmp_type": 3,
  "icmp_code": 1,
  "packet_hex": "..."
}
```

### Actions

#### Send Echo Reply
```json
{
  "type": "send_echo_reply",
  "source_ip": "192.168.1.100",
  "destination_ip": "192.168.1.50",
  "identifier": 1234,
  "sequence": 1,
  "payload_hex": "48656c6c6f"
}
```

#### Send Destination Unreachable
```json
{
  "type": "send_destination_unreachable",
  "source_ip": "192.168.1.1",
  "destination_ip": "192.168.1.50",
  "code": 1,
  "original_packet_hex": "4500003c..."
}
```

#### Send Time Exceeded (Traceroute)
```json
{
  "type": "send_time_exceeded",
  "source_ip": "10.0.0.1",
  "destination_ip": "192.168.1.50",
  "code": 0,
  "original_packet_hex": "4500003c..."
}
```

#### Send Timestamp Reply
```json
{
  "type": "send_timestamp_reply",
  "source_ip": "192.168.1.1",
  "destination_ip": "192.168.1.50",
  "identifier": 5678,
  "sequence": 1,
  "originate_timestamp": 12345678
}
```

#### Ignore ICMP
```json
{
  "type": "ignore_icmp"
}
```

## ICMP Message Types Supported

### Implemented
- **Echo Request/Reply (Type 8/0)** - Ping functionality
- **Timestamp Request/Reply (Type 13/14)** - Time synchronization
- **Destination Unreachable (Type 3)** - Error reporting (codes: net, host, protocol, port, etc.)
- **Time Exceeded (Type 11)** - TTL expiry (used in traceroute)

### Observable (via `icmp_other_message` event)
- **Source Quench (Type 4)**
- **Redirect (Type 5)**
- **Parameter Problem (Type 12)**
- **Address Mask Request/Reply (Type 17/18)**
- **Router Advertisement/Solicitation**

## Packet Construction

### IP Header
All ICMP packets are wrapped in IPv4 headers:
- Version: 4
- Header Length: 5 (20 bytes)
- TTL: 64
- Protocol: ICMP (1)
- Source/Destination from action parameters
- Checksum automatically calculated

### ICMP Checksum
Calculated using `pnet::packet::icmp::checksum()`:
- Covers entire ICMP packet (header + payload)
- Uses ones' complement sum algorithm
- Essential for packet validity

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
- Kernel may handle some ICMP types automatically
- Echo Request may be answered by kernel before reaching userspace
- May need to use `SO_BINDTODEVICE` or disable kernel ICMP handling
- Testing should verify userspace server receives packets

### Interface Binding
Current implementation receives on all interfaces:
- TODO: Bind to specific interface if needed
- Interface parameter accepted but not yet used for binding
- May receive packets from multiple interfaces

### IPv6 Support
Not yet implemented:
- Current implementation only supports ICMPv4
- ICMPv6 uses different socket (Protocol::ICMPV6)
- ICMPv6 message types differ (e.g., Echo Request is type 128, not 8)
- Future enhancement

## Dual Logging

All logs use both tracing macros and status_tx:
```rust
console_info!(status_tx, "ICMP server listening...");
console_error!(status_tx, "Failed to parse: {}", err);
console_trace!(status_tx, "Packet hex: {}", hex::encode(data));
```

**Log Levels:**
- **ERROR**: Socket creation failures, send errors
- **INFO**: Server start, packet processing complete
- **DEBUG**: Packet summaries, LLM calls, action counts
- **TRACE**: Full packet hex dumps

## Security Considerations

### Legitimate Uses
- Network diagnostics (ping responses)
- ICMP honeypots for intrusion detection
- Network behavior research
- Traceroute simulation
- Time synchronization testing

### Potential Misuse
- ⚠️ ICMP flood attacks (DoS)
- ⚠️ ICMP redirect attacks
- ⚠️ ICMP tunneling for data exfiltration
- ⚠️ Network scanning without authorization

### Safeguards
- No default flooding behavior (LLM controls all sends)
- Rate limiting via LLM logic
- Requires explicit root access (cannot be run accidentally)
- Documentation warns about ethical use

## Example Prompts

### Basic Ping Responder
```
"Listen for ICMP echo requests on eth0 and reply to all pings with 'NetGet Pong' in the payload"
```

### Selective Responder
```
"Respond to pings from 192.168.1.0/24 but send destination unreachable for all other sources"
```

### Traceroute Simulation
```
"Act as a router that sends time exceeded messages for packets with low TTL"
```

### Honeypot Mode
```
"Log all ICMP traffic but don't send any replies - silent ICMP honeypot"
```

### Timestamp Server
```
"Respond to ICMP timestamp requests with accurate system time"
```

## Performance

### Latency
- Raw socket receive: < 1ms
- pnet packet parsing: < 0.1ms
- LLM call: 100-500ms (dominant factor)
- Packet construction: < 1ms
- Raw socket send: < 1ms

**Total RTT**: ~100-500ms (primarily LLM inference)

### Throughput
Limited by LLM processing:
- Sequential processing: ~2-10 packets/sec
- Parallel LLM calls: Higher throughput, depends on Ollama config
- Not suitable for high-rate ping responses

### Optimization Strategies
- **Scripting mode**: Pre-computed responses, bypass LLM
- **Static responses**: Hardcode echo replies for known patterns
- **Caching**: Remember LLM decisions for common cases

## Testing Notes

See `tests/server/icmp/CLAUDE.md` for test strategy and E2E test details.

## Future Enhancements

1. **IPv6 Support**: ICMPv6 with Neighbor Discovery
2. **Interface Binding**: Bind to specific interface parameter
3. **BPF Filtering**: Kernel-level packet filtering for efficiency
4. **Scripting Mode**: Fast path for predictable responses
5. **Router Advertisement**: Full router simulation
6. **Multicast ICMP**: Group management messages

## References

- RFC 792 - Internet Control Message Protocol (ICMP)
- RFC 1191 - Path MTU Discovery
- RFC 1256 - ICMP Router Discovery
- pnet documentation: https://docs.rs/pnet/latest/pnet/
- socket2 documentation: https://docs.rs/socket2/latest/socket2/
