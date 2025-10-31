# IPSec/IKEv2 Honeypot Implementation

## Overview

IPSec/IKEv2 **honeypot** that detects and logs IKE handshake attempts. This is **NOT a full VPN server** - it does not establish actual tunnels or perform IKE negotiation.

**Status**: Honeypot-only (reconnaissance detection)
**Protocol Spec**: [RFC 7296 (IKEv2)](https://datatracker.ietf.org/doc/html/rfc7296)
**Ports**: UDP 500 (IKE), UDP 4500 (NAT-T)

## Library Choices

### ipsec-parser (Parse-Only)

**What it is**:
- Parsing library for IKE packets
- Can extract headers, SPIs, exchange types, payloads
- **Cannot build IKE responses or establish SAs**

**Why parse-only**:
- IPSec/IKEv2 requires deep OS integration (Linux XFRM, Windows IPsec stack)
- No mature Rust library provides server functionality
- Reference implementations (strongSwan, libreswan) are hundreds of thousands of lines of C

**Why we don't use it**:
- Manual parsing is simple enough for honeypot needs
- IKE header is fixed 28 bytes with well-defined fields
- Avoids dependency for minimal benefit

### Manual IKE Header Parsing

Honeypot manually parses 28-byte IKE header:
```rust
let initiator_spi = u64::from_be_bytes([packet[0..8]]);
let responder_spi = u64::from_be_bytes([packet[8..16]]);
let version = packet[17];  // 0x20 for IKEv2, 0x10 for IKEv1
let exchange_type = packet[18];
let message_id = u32::from_be_bytes([packet[20..24]]);
```

### Comprehensive Research Completed

See `/Users/matus/dev/netget/IPSEC_RESEARCH.md` for detailed analysis of why full IPSec implementation is infeasible.

**Explored options**:
1. Pure Rust (swanny) - Experimental, very early stage, missing critical features
2. Parser-only (ipsec-parser) - Cannot build servers, parsing only
3. strongSwan daemon + VICI interface - Violates architecture, requires root + XFRM
4. Custom implementation from scratch - 12-36 months minimum, massive complexity

**Key findings**:
- **swanny**: ~7,000 LOC, 80% coverage, but missing SA rekeying, fragmentation, cert auth
- **strongSwan**: ~100,000+ LOC, requires external binary + root + XFRM netlink
- **XFRM Kernel**: Undocumented, complex netlink protocol with hash tables and red-black trees

**Conclusion**: All options non-viable. Honeypot-only is the correct design choice.

## Architecture Decisions

### UDP-Only Honeypot

IPSec uses UDP for IKE negotiation:
- Port 500 for IKE (standard)
- Port 4500 for NAT-T (NAT traversal)

Honeypot binds to specified port (typically 500):
```rust
let socket = UdpSocket::bind(bind_addr).await?;
```

### IKE Version Detection

Distinguishes IKEv1 and IKEv2 by version byte:
```rust
let (ike_version, exchange_name) = if version == 0x20 {
    // IKEv2
    match exchange_type {
        34 => ("IKEv2", "IKE_SA_INIT"),
        35 => ("IKEv2", "IKE_AUTH"),
        36 => ("IKEv2", "CREATE_CHILD_SA"),
        37 => ("IKEv2", "INFORMATIONAL"),
        _ => ("IKEv2", "Unknown"),
    }
} else {
    // IKEv1
    match exchange_type {
        2 => ("IKEv1", "Identity Protection"),
        4 => ("IKEv1", "Aggressive Mode"),
        _ => ("IKEv1", "Unknown"),
    }
};
```

### Handshake Detection Only

Honeypot focuses on detecting handshake initiation:
- **IKEv2**: `IKE_SA_INIT` (34) and `IKE_AUTH` (35)
- **IKEv1**: Identity Protection (2) and Aggressive Mode (4)

Other messages (CREATE_CHILD_SA, INFORMATIONAL) logged but not analyzed.

### No Response Packets

Honeypot **does not respond** to IKE messages. This prevents:
- Accidental SA establishment (impossible anyway)
- Revealing honeypot nature
- Complex crypto and payload generation

## LLM Integration

### Async Actions (User-triggered)

1. **list_connections**: View detected reconnaissance attempts
2. **close_connection**: Close honeypot (no-op, UDP is connectionless)

### Sync Actions (Network event triggered)

1. **accept_connection**: Log as accepted reconnaissance (no actual tunnel)
2. **reject_connection**: Log as rejected reconnaissance
3. **log_handshake**: Record IKE handshake details
4. **send_notify**: Placeholder for IKE NOTIFY message (not implemented)
5. **inspect_traffic**: Analyze IKE packet patterns

### Event Types

- `ipsec_handshake`: IKE handshake initiation detected
- `ipsec_data`: ESP encrypted data packet (future)

**Event data example**:
```json
{
  "peer_addr": "203.0.113.45:500",
  "packet_size": 256,
  "ike_version": "IKEv2",
  "exchange_type": "IKE_SA_INIT",
  "initiator_spi": "0123456789abcdef",
  "responder_spi": "0000000000000000",
  "honeypot_mode": true
}
```

## Connection Management

### No Connection State

UDP is connectionless, so "connections" are ephemeral:
- Each packet logged independently
- No SA (Security Association) tracking
- No persistent peer state

### SPI Extraction

IKE packets include SPIs (Security Parameter Indexes):
```rust
let initiator_spi = u64::from_be_bytes([packet[0], packet[1], ..., packet[7]]);
let responder_spi = u64::from_be_bytes([packet[8], packet[9], ..., packet[15]]);
```

- **Initiator SPI**: Non-zero, uniquely identifies initiator's SA
- **Responder SPI**: Zero in initial request, assigned by responder

## State Management

### Server State

```rust
pub struct IpsecServer;  // No state needed
```

Honeypot is stateless - just logs packets as they arrive.

### Protocol Connection Info

Not applicable - honeypot doesn't track SAs.

## Limitations

### No Actual VPN Tunnels

- **No IKE negotiation**: Cannot complete SA establishment
- **No ESP encryption**: Cannot encrypt/decrypt IPSec data packets
- **No XFRM policy**: No kernel SAD/SPD configuration
- **No tunnel interface**: No TUN device creation
- **No routing**: No IP forwarding or VPN subnet

### Detection Only

Honeypot can:
- ✅ Detect IKE handshake attempts (IKEv1 and IKEv2)
- ✅ Extract SPIs, exchange types, version
- ✅ Distinguish IKE_SA_INIT, IKE_AUTH, etc.
- ✅ Log reconnaissance attempts
- ✅ Provide data to LLM for analysis

Honeypot cannot:
- ❌ Complete IKE negotiation
- ❌ Establish SAs (Security Associations)
- ❌ Decrypt ESP traffic
- ❌ Authenticate clients
- ❌ Create VPN tunnels

### Why Full Implementation is Infeasible

From VPN_IMPLEMENTATION_STATUS.md:

> **Why No Full Implementation**:
> - IPSec/IKEv2 protocol is extremely complex (IKE negotiation + ESP encryption + XFRM policy)
> - Requires deep OS integration (Linux XFRM, Windows IPsec stack, etc.)
> - No mature Rust library provides server functionality
> - Reference implementations (strongSwan, libreswan) are hundreds of thousands of lines of C

**Recommendation**: Use WireGuard for production VPN needs. IPSec honeypot is sufficient for security research and IKE protocol analysis.

## Examples

### Honeypot Startup

```
netget> Start an IPSec/IKEv2 honeypot on port 500
```

Server output:
```
[INFO] Starting IPSec/IKEv2 honeypot on 0.0.0.0:500 (reconnaissance detection only)
[INFO] IPSec/IKEv2 honeypot listening on 0.0.0.0:500
→ IPSec/IKEv2 honeypot ready
```

### IKEv2 Handshake Detection

When client sends IKE_SA_INIT:
```
[TRACE] IPSec: IKEv2 IKE_SA_INIT from 203.0.113.45:500 (256 bytes)
[INFO] IPSec: IKEv2 handshake reconnaissance from 203.0.113.45:500
```

LLM receives event:
```json
{
  "event": "ipsec_handshake",
  "data": {
    "peer_addr": "203.0.113.45:500",
    "ike_version": "IKEv2",
    "exchange_type": "IKE_SA_INIT",
    "initiator_spi": "0123456789abcdef",
    "responder_spi": "0000000000000000",
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
      "details": "IKEv2 SA_INIT detected from 203.0.113.45 - likely reconnaissance or misconfigured VPN client"
    },
    {
      "type": "show_message",
      "message": "IKEv2 handshake logged for security analysis"
    }
  ]
}
```

### IKEv1 Detection

IKEv1 Identity Protection mode:
```
[TRACE] IPSec: IKEv1 Identity Protection from 198.51.100.10:500 (128 bytes)
[INFO] IPSec: IKEv1 handshake reconnaissance from 198.51.100.10:500
```

### Multiple Exchange Types

After SA_INIT, client might send IKE_AUTH:
```
[TRACE] IPSec: IKEv2 IKE_AUTH from 203.0.113.45:500 (512 bytes)
[DEBUG] IPSec: IKEv2 IKE_AUTH from 203.0.113.45:500 (logged)
```

## Use Cases

### Security Research

- Detect IPSec scanning/reconnaissance
- Identify misconfigured VPN clients
- Log attack patterns for analysis
- Fingerprint IKE client implementations

### Network Monitoring

- Identify unauthorized IPSec attempts
- Track VPN traffic patterns
- Monitor for IPSec-based attacks
- Detect IKEv1 vs IKEv2 usage

### Protocol Analysis

- Study IKE exchange sequences
- Analyze cipher suite proposals
- Identify common misconfigurations

### NOT for Production VPN

IPSec honeypot should **not** be used when actual VPN tunnels are needed. Use WireGuard instead:
```
netget> Start a WireGuard VPN server on port 51820
```

## References

- [RFC 7296 - IKEv2](https://datatracker.ietf.org/doc/html/rfc7296)
- [RFC 4301 - IPSec Architecture](https://datatracker.ietf.org/doc/html/rfc4301)
- [RFC 4303 - ESP](https://datatracker.ietf.org/doc/html/rfc4303)
- [ipsec-parser Documentation](https://docs.rs/ipsec-parser/)
- [strongSwan Documentation](https://docs.strongswan.org/)
- [IPSEC_RESEARCH.md](../../IPSEC_RESEARCH.md) - Detailed analysis of implementation options
- [VPN_IMPLEMENTATION_STATUS.md](../../VPN_IMPLEMENTATION_STATUS.md) - Why IPSec is honeypot-only
