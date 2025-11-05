# IS-IS Server Implementation

## Overview

IS-IS (Intermediate System to Intermediate System) routing protocol server implementing ISO/IEC 10589 and RFC 1195 (IS-IS for IP). The LLM controls neighbor adjacencies, Hello PDU responses, and Link State PDU generation.

**Status**: Experimental
**Protocol Spec**: [ISO/IEC 10589](https://www.iso.org/standard/30932.html), [RFC 1195 (IS-IS for IP)](https://datatracker.ietf.org/doc/html/rfc1195)
**Port**: UDP 3784 (encapsulated IS-IS)

## Library Choices

### No Library - Manual Protocol Implementation

**Why no library**:
- Holo exists but is a full production routing daemon, not a library
- No lightweight IS-IS parsing library available
- IS-IS packet structure is well-documented (ISO/IEC 10589, RFC 1195)
- Manual implementation provides full LLM control over routing behavior
- Honeypot/learning use case doesn't require full routing functionality

**What we implement manually**:
- IS-IS PDU parsing (Hello, LSP, CSNP, PSNP)
- TLV (Type-Length-Value) encoding/decoding
- Hello PDU construction with area addresses
- Simplified LSP construction
- Adjacency state tracking

**Why not alternatives**:
- `holo` - Production routing suite, too heavyweight for NetGet
- No other Rust IS-IS libraries exist
- External daemon (FRR, BIRD) - Violates NetGet architecture

## Architecture Decisions

### UDP Encapsulation

IS-IS traditionally runs directly over Layer 2 (Data Link), but this implementation uses UDP encapsulation:
- **Port**: 3784 (standard for IS-IS over UDP)
- **Rationale**: Simpler than raw sockets, no elevated privileges required
- **Limitation**: Cannot participate in real IS-IS networks (which use Layer 2)
- **Use case**: Honeypot, testing, learning, simulation

### IS-IS PDU Types

IS-IS uses 4 types of PDUs:

1. **Hello (IIH)** - Neighbor discovery and adjacency maintenance
   - LAN Hello Level 1 (type 15)
   - LAN Hello Level 2 (type 16)
   - Point-to-Point Hello (type 17)

2. **LSP (Link State PDU)** - Topology information distribution
   - Level 1 LSP (type 18)
   - Level 2 LSP (type 20)

3. **CSNP (Complete Sequence Number PDU)** - Database synchronization
   - Level 1 CSNP (type 24)
   - Level 2 CSNP (type 25)

4. **PSNP (Partial Sequence Number PDU)** - LSP acknowledgment/request
   - Level 1 PSNP (type 26)
   - Level 2 PSNP (type 27)

**Current implementation**: Hello and LSP parsing/construction. CSNP/PSNP logged but not handled.

### TLV (Type-Length-Value) Encoding

IS-IS uses TLV encoding for extensibility. Each TLV has:
- Type (1 byte) - identifies the TLV
- Length (1 byte) - length of value field
- Value (variable) - data

**Common TLVs implemented**:
- Area Addresses (type 1) - IS-IS area IDs
- Protocols Supported (type 129) - IPv4/IPv6 support
- IP Interface Addresses (type 132) - IPv4 addresses
- Hostname (type 137) - Router hostname

### IS-IS Addressing

**System ID**: 6-byte identifier (e.g., `0000.0000.0001`)
- Uniquely identifies an IS-IS router
- Format: 3 dotted groups of 4 hex digits

**Area ID**: Variable-length (e.g., `49.0001`)
- Identifies the IS-IS area
- Private areas start with 49 (similar to RFC 1918)

**Level**: IS-IS routers operate at one or both levels
- Level 1: Intra-area routing
- Level 2: Inter-area routing (backbone)
- Level 1+2: Both levels

### Adjacency State Machine

Simplified IS-IS adjacency states:
1. **Init**: Received Hello, neighbor detected
2. **Up**: Adjacency established, can exchange routing info
3. **Down**: Adjacency lost (holding time expired)

Full IS-IS has more complex state machine, but this is sufficient for honeypot/testing.

## LLM Integration

### Startup Parameters

Server configured with:
```json
{
  "system_id": "0000.0000.0001",
  "area_id": "49.0001",
  "level": "level-2"
}
```

Extracted from LLM-generated startup prompt.

### Sync Actions (Network event triggered)

1. **send_isis_hello**: Send Hello PDU for neighbor discovery
   - `pdu_type`: "lan_hello_l1", "lan_hello_l2", or "p2p_hello"
   - `system_id`: Local system ID (e.g., "0000.0000.0001")
   - `area_id`: Area ID (e.g., "49.0001")
   - `holding_time`: Holding time in seconds (default: 30)

2. **send_isis_lsp**: Send Link State PDU
   - `level`: "level-1" or "level-2"
   - `system_id`: Local system ID

3. **send_isis_pdu**: Send raw IS-IS PDU from hex
   - `data`: Hex-encoded PDU

4. **ignore_pdu**: No response to received PDU

### Event Types

**`isis_hello`** - IS-IS Hello PDU received

Event data:
```json
{
  "pdu_type": "LAN Hello L2",
  "pdu_type_code": 16,
  "peer_addr": "192.168.1.100:3784",
  "packet_hex": "831b01...",
  "area_addresses": ["49.0001"],
  "protocols_supported": ["0xCC"],
  "ip_addresses": ["192.168.1.100"],
  "hostname": "router1"
}
```

## Connection Management

### Per-PDU "Connection"

IS-IS uses UDP, so each PDU creates a new "connection" entry:
- Connection ID: Unique per PDU
- Tracks adjacency state, neighbor system ID, level
- Updated on each Hello received

### State Tracking

```rust
ProtocolConnectionInfo::Isis {
    adjacency_state: String,      // "init", "up", "down"
    neighbor_system_id: Option<String>, // e.g., "0000.0000.0002"
    level: String,                 // "level-1", "level-2", "level-1+2"
}
```

## Logging

### Dual Logging Strategy

**DEBUG**: PDU summaries
- "IS-IS received LAN Hello L2 from 192.168.1.100, 128 bytes"
- "IS-IS sent 96 bytes to 192.168.1.100"

**TRACE**: Full packet dumps
- "IS-IS PDU (hex): 831b01..."
- "IS-IS sent (hex): 831b01..."

**INFO**: Adjacency events
- "IS-IS LAN Hello L2 from 192.168.1.100"
- "IS-IS LSP received (forwarding to LLM)"

**WARN**: Invalid packets
- "IS-IS invalid protocol discriminator: 0x82"
- "IS-IS unsupported version: 2"

All logs go to both `netget.log` (via tracing macros) and TUI (via `status_tx`).

## Limitations

### Partial Implementation

**Implemented**:
- ✅ Hello PDU parsing (all 3 types)
- ✅ Hello PDU construction
- ✅ TLV parsing (Area, Protocols, IP Addresses, Hostname)
- ✅ Basic LSP construction
- ✅ Adjacency state tracking

**Not Implemented**:
- ❌ CSNP/PSNP handling (logged but ignored)
- ❌ LSP database (no topology storage)
- ❌ SPF (Shortest Path First) calculation
- ❌ Routing table integration
- ❌ Designated IS election (for LAN segments)
- ❌ Authentication (MD5, HMAC-SHA)
- ❌ IPv6 support (only IPv4 TLVs)
- ❌ Flooding logic
- ❌ Holding time enforcement

### No Routing Functionality

Server doesn't perform routing:
- LSPs parsed but not stored in database
- No SPF calculation or route computation
- Cannot participate in real IS-IS networks
- For honeypot, testing, and learning only

### UDP Encapsulation Only

Real IS-IS uses Layer 2 (Ethernet, PPP), this uses UDP:
- Cannot interoperate with real IS-IS routers
- No multicast support (IS-IS uses 01-80-C2-00-00-14/15)
- Simplified for honeypot/testing scenarios

### No Multi-Level Support

Each server instance operates at a single level (L1 or L2):
- No Level 1+2 router support
- No inter-level route leaking
- Simplified for testing

## Examples

### Server Startup

```
netget> Start an IS-IS router on port 3784 with system-id 0000.0000.0001 in area 49.0001
```

Server output:
```
[INFO] IS-IS server listening on 0.0.0.0:3784
[INFO] IS-IS configured: system_id=0000.0000.0001, area=49.0001, level=level-2
```

### Hello PDU Received

Client sends Hello:
```
[DEBUG] IS-IS received LAN Hello L2 from 192.168.1.100, 128 bytes
[TRACE] IS-IS PDU (hex): 831b0100100106000...
→ IS-IS LAN Hello L2 from 192.168.1.100
```

LLM receives event:
```json
{
  "pdu_type": "LAN Hello L2",
  "pdu_type_code": 16,
  "peer_addr": "192.168.1.100:3784",
  "area_addresses": ["49.0001"],
  "protocols_supported": ["0xCC"],
  "hostname": "neighbor-router"
}
```

LLM responds:
```json
{
  "actions": [
    {
      "type": "send_isis_hello",
      "pdu_type": "lan_hello_l2",
      "system_id": "0000.0000.0001",
      "area_id": "49.0001",
      "holding_time": 30
    }
  ]
}
```

Server sends Hello:
```
[DEBUG] IS-IS sent 96 bytes to 192.168.1.100
[TRACE] IS-IS sent (hex): 831b0100100106000...
→ IS-IS response to 192.168.1.100 (96 bytes)
```

### LSP Received

```
[INFO] IS-IS LSP received (forwarding to LLM)
```

Currently logged but not fully processed (no LSP database).

## Use Cases

### Learning IS-IS Protocol

- Understand IS-IS packet structure
- Experiment with Hello PDUs and adjacencies
- Analyze TLV encoding
- Study routing protocol behavior

### Honeypot/Monitoring

- Detect unauthorized IS-IS routers on network
- Log IS-IS reconnaissance attempts
- Monitor for routing protocol attacks
- Simulate IS-IS router for testing

### Testing

- Test IS-IS client implementations
- Validate IS-IS packet parsing
- Simulate network topologies
- Debug IS-IS issues

### NOT for Production Routing

IS-IS server should **not** be used for production routing:
- No SPF calculation or routing table
- No LSP database or flooding
- UDP encapsulation not compatible with real IS-IS
- No authentication or security features

For production routing, use established implementations (FRR, BIRD, Holo).

## References

- [ISO/IEC 10589 - IS-IS Routing Protocol](https://www.iso.org/standard/30932.html)
- [RFC 1195 - Use of OSI IS-IS for Routing in TCP/IP and Dual Environments](https://datatracker.ietf.org/doc/html/rfc1195)
- [RFC 5120 - M-ISIS: Multi Topology Routing in IS-IS](https://datatracker.ietf.org/doc/html/rfc5120)
- [RFC 5303 - Three-Way Handshake for IS-IS Point-to-Point Adjacencies](https://datatracker.ietf.org/doc/html/rfc5303)
- [RFC 5305 - IS-IS Extensions for Traffic Engineering](https://datatracker.ietf.org/doc/html/rfc5305)
- [Holo Routing Suite](https://github.com/holo-routing/holo) - Production IS-IS implementation in Rust
