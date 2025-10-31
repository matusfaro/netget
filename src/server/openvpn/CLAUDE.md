# OpenVPN Honeypot Implementation

## Overview

OpenVPN **honeypot** that detects and logs OpenVPN connection attempts. This is **NOT a full VPN server** - it does not establish actual tunnels or perform TLS handshakes.

**Status**: Honeypot-only (reconnaissance detection)
**Protocol Spec**: [OpenVPN Protocol](https://openvpn.net/community-resources/reference-manual-for-openvpn-2-4/)
**Port**: UDP 1194 (default)

## Library Choices

### No Library - Manual Packet Parsing

**Why no library**:
- **No viable Rust OpenVPN server library exists**
- Available crates are client-only, plugin interfaces, or management APIs
- Reference implementation (OpenVPN 2.x/3.x) is 500,000+ lines of C++
- Protocol is extremely complex: TLS + custom framing + compression + many cipher modes

**What we implement manually**:
- Opcode extraction from packet header (upper 5 bits)
- Key ID extraction (lower 3 bits)
- Session ID parsing (V2 packets only)
- Packet type detection (Hard Reset, Control, ACK, Data)

**Why not FFI to libopenvpn3**:
- Client-only library, not suitable for server
- Complex C++ FFI with fragile ABI
- Would violate NetGet architecture (external dependencies)

### Comprehensive Research Completed

See `/Users/matus/dev/netget/OPENVPN_RESEARCH.md` for detailed analysis of why full OpenVPN implementation is infeasible.

**Explored options**:
1. Pure Rust implementation - Does not exist, would take 6-12+ months
2. FFI to libopenvpn3 - Client-only, not suitable
3. Spawn openvpn daemon + management interface - Violates architecture
4. Custom implementation from scratch - 12-24 months minimum

**Conclusion**: All options non-viable. Honeypot-only is the correct design choice.

## Architecture Decisions

### UDP-Only Honeypot

OpenVPN can run on TCP or UDP. Honeypot listens on UDP only (more common for VPN):
```rust
let socket = UdpSocket::bind(bind_addr).await?;
```

### Packet Opcode Detection

OpenVPN opcodes stored in upper 5 bits of first byte:
```rust
let opcode = (packet[0] >> 3) & 0x1F;
let key_id = packet[0] & 0x07;
```

**Supported opcodes**:
- `P_CONTROL_HARD_RESET_CLIENT_V1` (1) - V1 handshake initiation
- `P_CONTROL_HARD_RESET_CLIENT_V2` (7) - V2 handshake initiation
- `P_CONTROL_HARD_RESET_SERVER_V1` (2) - Server response
- `P_CONTROL_SOFT_RESET_V1` (3) - Renegotiation
- `P_CONTROL_V1` (4) - Control message
- `P_ACK_V1` (5) - Acknowledgment
- `P_DATA_V1` (6) - Encrypted data (V1)
- `P_DATA_V2` (9) - Encrypted data (V2)

### Handshake Detection Only

Honeypot focuses on detecting handshake initiation packets (`HARD_RESET_CLIENT`):
```rust
let is_handshake = matches!(opcode,
    P_CONTROL_HARD_RESET_CLIENT_V1 | P_CONTROL_HARD_RESET_CLIENT_V2
);

if is_handshake {
    // Log reconnaissance attempt
    // Notify LLM for analysis
}
```

### No Response Packets

Honeypot **does not respond** to handshakes. This prevents:
- Accidental VPN tunnel establishment (impossible anyway)
- Revealing honeypot nature to sophisticated attackers
- Complex TLS state machine implementation

## LLM Integration

### Async Actions (User-triggered)

1. **list_connections**: View detected reconnaissance attempts
2. **close_connection**: Close honeypot (no-op, UDP is connectionless)

### Sync Actions (Network event triggered)

1. **accept_connection**: Log as accepted reconnaissance (no actual tunnel)
2. **reject_connection**: Log as rejected reconnaissance
3. **log_handshake**: Record handshake details to memory/logs
4. **send_reset**: Placeholder (no reset for UDP)
5. **inspect_traffic**: Analyze packet patterns

### Event Types

- `openvpn_handshake`: Client initiated handshake (V1 or V2)
- `openvpn_data`: Data packet received (encrypted, cannot decrypt)

**Event data example**:
```json
{
  "peer_addr": "203.0.113.45:1194",
  "packet_size": 128,
  "packet_type": "ControlHardResetClientV2",
  "opcode": 7,
  "key_id": 0,
  "session_id": "0123456789abcdef",
  "honeypot_mode": true
}
```

## Connection Management

### No Connection State

UDP is connectionless, so "connections" are ephemeral:
- Each packet logged independently
- No connection tracking in server state
- No persistent peer information

### Session ID Extraction

V2 packets include 8-byte session ID after opcode:
```rust
let session_id = if opcode == P_CONTROL_HARD_RESET_CLIENT_V2 && packet.len() >= 9 {
    Some(format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        packet[1], packet[2], packet[3], packet[4],
        packet[5], packet[6], packet[7], packet[8]
    ))
} else {
    None
};
```

## State Management

### Server State

```rust
pub struct OpenvpnServer;  // No state needed
```

Honeypot is stateless - just logs packets as they arrive.

### Protocol Connection Info

Not applicable - honeypot doesn't track connections.

## Limitations

### No Actual VPN Tunnels

- **No TLS handshake**: Cannot complete OpenVPN authentication
- **No encryption**: Cannot decrypt data packets
- **No tunnel interface**: No TUN/TAP device creation
- **No routing**: No IP forwarding or VPN subnet

### Detection Only

Honeypot can:
- ✅ Detect handshake attempts (V1 and V2)
- ✅ Extract session IDs and opcodes
- ✅ Log reconnaissance attempts
- ✅ Provide data to LLM for analysis

Honeypot cannot:
- ❌ Complete handshake
- ❌ Establish tunnels
- ❌ Decrypt traffic
- ❌ Authenticate clients

### Why Full Implementation is Infeasible

From VPN_IMPLEMENTATION_STATUS.md:

> **Why No Full Implementation**:
> - OpenVPN protocol is extremely complex (TLS + custom framing + compression + many modes)
> - Reference implementation is 500K+ lines of C++
> - Rust ecosystem has focused on modern protocols (WireGuard)
> - FFI to libopenvpn3 would be fragile and hard to maintain

**Recommendation**: Use WireGuard for production VPN needs. OpenVPN honeypot is sufficient for security research and reconnaissance detection.

## Examples

### Honeypot Startup

```
netget> Start an OpenVPN honeypot on port 1194
```

Server output:
```
[INFO] Starting OpenVPN honeypot on 0.0.0.0:1194 (reconnaissance detection only)
[INFO] OpenVPN honeypot listening on 0.0.0.0:1194
→ OpenVPN honeypot ready
```

### Handshake Detection (V2)

When client sends handshake:
```
[TRACE] OpenVPN: ControlHardResetClientV2 packet from 203.0.113.45:1194 (128 bytes)
[INFO] OpenVPN: Handshake reconnaissance from 203.0.113.45:1194
```

LLM receives event:
```json
{
  "event": "openvpn_handshake",
  "data": {
    "peer_addr": "203.0.113.45:1194",
    "packet_type": "ControlHardResetClientV2",
    "opcode": 7,
    "key_id": 0,
    "session_id": "0123456789abcdef",
    "honeypot_mode": true
  }
}
```

LLM can respond:
```json
{
  "actions": [
    {
      "type": "log_handshake",
      "details": "OpenVPN V2 handshake detected from 203.0.113.45 - likely reconnaissance or misconfigured client"
    },
    {
      "type": "show_message",
      "message": "OpenVPN handshake logged for security analysis"
    }
  ]
}
```

### Other Packet Types

Control, ACK, and Data packets also logged:
```
[DEBUG] OpenVPN ControlV1 from 203.0.113.45:1194 (logged)
[DEBUG] OpenVPN AckV1 from 203.0.113.45:1194 (logged)
[DEBUG] OpenVPN DataV2 from 203.0.113.45:1194 (logged)
```

## Use Cases

### Security Research

- Detect OpenVPN scanning/reconnaissance
- Identify misconfigured clients attempting connections
- Log attack patterns for analysis
- Fingerprint OpenVPN client versions

### Network Monitoring

- Identify unauthorized VPN attempts
- Track VPN traffic patterns
- Monitor for VPN-based attacks

### NOT for Production VPN

OpenVPN honeypot should **not** be used when actual VPN tunnels are needed. Use WireGuard instead:
```
netget> Start a WireGuard VPN server on port 51820
```

## References

- [OpenVPN Protocol Documentation](https://openvpn.net/community-resources/openvpn-protocol/)
- [OpenVPN Source Code](https://github.com/OpenVPN/openvpn)
- [OPENVPN_RESEARCH.md](../../OPENVPN_RESEARCH.md) - Detailed analysis of implementation options
- [VPN_IMPLEMENTATION_STATUS.md](../../VPN_IMPLEMENTATION_STATUS.md) - Why OpenVPN is honeypot-only
