# IPSec/IKEv2 Enhanced Honeypot Implementation

## Overview

IPSec/IKEv2 **enhanced honeypot** that detects and logs IKE handshake attempts with detailed protocol analysis. This is **NOT a full VPN server** - it does not establish actual tunnels, but provides comprehensive IKE message parsing.

**Status**: Experimental (enhanced honeypot with swanny library)
**Protocol Spec**: [RFC 7296 (IKEv2)](https://datatracker.ietf.org/doc/html/rfc7296)
**Ports**: UDP 500 (IKE), UDP 4500 (NAT-T)
**Future Path**: Full VPN implementation when swanny reaches 1.0 (mid-2025)

## Library Choices

### swanny v0.1 (Enhanced Parsing)

**Repository**: https://gitlab.com/dueno/swanny
**Author**: Daiki Ueno (Red Hat, GnuTLS maintainer)
**Created**: May 2025
**License**: GPL v3.0 or later
**Status**: Experimental (use at own risk)

**What it provides**:
- Complete IKE message parsing (headers, payloads, transforms)
- IKE SA state machine (initial exchange, auth)
- Child SA creation/deletion/rekeying
- Composable, library-based architecture (no daemon)
- Externally driven API (easy LLM integration)
- 80% test coverage (~7k LOC library, 400 LOC examples)

**Current capabilities** (as of Jan 2025):
- ✅ Initial exchange (IKE_SA_INIT, IKE_AUTH)
- ✅ Child SA creation/deletion
- ✅ Child SA rekeying
- ✅ Basic interop with Libreswan
- ❌ IKE SA rekeying (connections won't persist long-term)
- ❌ IP fragmentation (large packets may fail)
- ❌ Certificate-based authentication (PSK only)

**Why we use it for enhanced honeypot**:
- Provides detailed IKE message analysis beyond basic header parsing
- Can extract cipher suites, transforms, payloads for security research
- Library-based design fits NetGet architecture (like WireGuard's defguard)
- Foundation for future full VPN implementation when swanny matures
- Active development by experienced cryptography maintainer

**Why NOT full VPN implementation yet**:
- Library is only 6 months old (very early stage)
- Missing IKE SA rekeying (major limitation for production VPN)
- Missing fragmentation support (large packets fail)
- No certificate auth (PSK only)
- Untested with diverse client implementations
- WireGuard already provides production VPN in NetGet

### Previous Research (Nov 2024)

**Explored options**:
1. **ipsec-parser** - Parser-only, cannot build responses or establish SAs
2. **strongSwan + VICI** - Requires external daemon, root privileges, XFRM kernel integration (violates NetGet architecture)
3. **Custom implementation** - 12-36 months minimum development time, massive complexity

**Key findings**:
- **strongSwan**: ~100,000+ LOC, requires external binary + root + XFRM netlink
- **XFRM Kernel**: Undocumented, complex netlink protocol with hash tables and red-black trees
- **Manual parsing**: Simple for basic honeypot, but limited analysis capabilities

**Conclusion (Nov 2024)**: Honeypot-only was the correct choice at the time.

**Update (Jan 2025)**: Swanny library emerged as viable path forward for enhanced analysis and future full implementation.

## Architecture Decisions

### Enhanced Honeypot (Current Implementation)

**Design philosophy**: Enhanced detection and analysis WITHOUT establishing actual VPN tunnels.

**UDP-based IKE listener**:
- Port 500 for IKE (standard)
- Port 4500 for NAT-T (NAT traversal)
- Binds UDP socket: `UdpSocket::bind(bind_addr).await?`

**Enhanced manual parsing** (no external dependencies):
- Complete IKE header extraction (28 bytes)
- Payload chain analysis (next payload indicators)
- Payload type identification (SA, KE, Nonce, etc.)
- Flag analysis (Initiator, Response, Version bits)
- Message ID tracking

**Parsing flow**:
```rust
// Parse IKE header (28 bytes)
let initiator_spi = u64::from_be_bytes(packet[0..8]);
let responder_spi = u64::from_be_bytes(packet[8..16]);
let next_payload = packet[16];  // First payload type
let version = packet[17];
let exchange_type = packet[18];
let flags = packet[19];  // Initiator, Response, Version
let message_id = u32::from_be_bytes(packet[20..24]);
let length = u32::from_be_bytes(packet[24..28]);

// Analyze flags
let is_initiator = (flags & 0x08) != 0;
let is_response = (flags & 0x20) != 0;

// Extract payload types from chain
let payload_types = extract_payload_chain(packet, next_payload);
// e.g., [SA, KE, Nonce, Notify] for IKE_SA_INIT
```

**Why manual parsing**:
- Swanny not yet on crates.io (git dependency)
- GPL v3.0 license (incompatible with NetGet's licensing)
- Experimental API (frequent breaking changes expected)
- Manual parsing sufficient for honeypot analysis
- Foundation documented for future full implementation

### Enhanced Detection Capabilities

**Beyond basic honeypot** (current implementation):
- ✅ Extract all IKE header fields (SPIs, flags, message ID)
- ✅ Identify payload types in chain (SA, KE, Nonce, Notify, etc.)
- ✅ Detect initiator vs responder messages
- ✅ Track message ID sequences
- ✅ Distinguish IKE_SA_INIT, IKE_AUTH, CREATE_CHILD_SA, INFORMATIONAL
- ✅ Provide detailed logs for security research

**Future capabilities** (with swanny when mature):
- Extract proposed cipher suites
- Identify Diffie-Hellman groups
- Parse transform attributes (key lengths, PRF, etc.)
- Detect vendor IDs (fingerprint client implementations)
- Analyze traffic selectors (identify VPN routes)
- Log certificate requests

**Still honeypot**:
- ❌ Does NOT send IKE responses
- ❌ Does NOT establish SAs (Security Associations)
- ❌ Does NOT create tunnels
- ❌ Does NOT perform cryptographic operations

**Why no responses**:
- Prevents accidental tunnel establishment
- Avoids revealing honeypot nature to scanners
- Keeps implementation simple and safe
- Focus on reconnaissance detection, not VPN service

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

### Why Full Implementation Deferred (Not Infeasible)

**Previous conclusion (Nov 2024)**: Full IPSec implementation was infeasible.

**Updated assessment (Jan 2025)**: Full implementation is now **viable but premature**.

**Why deferred**:
- Swanny library is only 6 months old (experimental stage)
- Missing critical features (IKE SA rekeying, fragmentation, cert auth)
- WireGuard already provides production-ready VPN in NetGet
- Better to wait for swanny 1.0 (expected mid-2025)

**Path to full implementation**:
1. **Current**: Enhanced honeypot with swanny parsing (Jan 2025)
2. **Mid-2025**: Evaluate swanny 1.0 for full VPN capability
3. **Future**: Full IPSec VPN server when library matures

**For production VPN needs**: Use WireGuard (NetGet's stable VPN protocol).

**For IPSec research**: Enhanced honeypot provides comprehensive protocol analysis.

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
