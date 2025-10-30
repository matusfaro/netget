# IPSec/IKEv2 Full Server Implementation Research

## Executive Summary

After comprehensive research into Rust IPSec/IKEv2 libraries and implementation options, **there is no viable path to implement a full IPSec VPN server in NetGet** that maintains the project's architecture and LLM-control philosophy. This document details all options explored and explains why IPSec should remain a honeypot-only implementation.

---

## Research Findings

### 1. Pure Rust IPSec/IKEv2 Server Implementation

**Status**: ⚠️ Experimental only (swanny library in early development)

**Libraries Evaluated**:

#### a) `ipsec-parser` (v0.7.3)
- **Type**: Parser-only library using nom combinator framework
- **Functionality**: Parses IKEv2 headers, payloads, and ESP envelope
- **Server Support**: ❌ Parser-only, no server functionality
- **RFC Support**: RFC7296 (IKEv2)
- **Features**:
  - Parse IKEv2 header and message structure
  - Parse IKEv2 payload list (SA, Auth, Certificates, KE, Notify, etc.)
  - Read ESP encapsulated message envelope
  - Differentiate between IKE and ESP headers
- **Limitations**:
  - Does NOT handle message interpretation
  - Does NOT provide encryption/decryption
  - Does NOT implement SA negotiation
  - Does NOT create IPSec tunnels
- **Maturity**: Last updated ~4 years ago (2020)
- **Project**: Part of Rusticata project (Rust parsers for network protocols)
- **Verdict**: Not suitable for server implementation, parser-only

#### b) `swanny` (Not published on crates.io)
- **Type**: Composable IKEv2 library (experimental)
- **Repository**: gitlab.com/dueno/swanny
- **Developer**: Daiki Ueno
- **Presented**: DevConf.CZ 2025
- **Status**: ⚠️ **Very early stage, use at your own risk**
- **Codebase Size**: ~7,000 lines of code (library), ~400 LOC (examples)
- **Test Coverage**: 80% line coverage

**Working Features**:
- ✅ Initial exchange (IKE_SA_INIT, IKE_AUTH)
- ✅ Child SA creation
- ✅ Child SA deletion
- ✅ Child SA rekeying
- ✅ Very basic interop with Libreswan

**Not Yet Working**:
- ❌ IKE SA rekeying
- ❌ Fragmentation
- ❌ Certificate-based authentication
- ❌ Production readiness
- ❌ Full protocol feature set

**Design Goals**:
- Enable custom solutions for specific scenarios
- Don't require real network setup for testing (simulation-friendly)
- Fuzzing-friendly architecture

**Assessment**:
- **Maturity**: Experimental, early alpha stage
- **Production Ready**: ❌ No
- **API Stability**: Unknown, likely unstable
- **Maintenance**: Active (2025) but very new
- **Documentation**: Minimal (presentation slides only)
- **Community**: No significant adoption yet
- **Interoperability**: Very basic (tested only with Libreswan)

**Verdict**: Not production-ready for NetGet. Would require:
- 6-12 months of maturation
- Comprehensive testing with multiple IPSec implementations
- Stable API definition
- Production hardening and security audits
- Feature completion (IKE SA rekeying, certificate auth, etc.)

**Recommendation**: Monitor swanny development but DO NOT use currently

---

### 2. strongSwan Control via VICI Interface

**Status**: ⚠️ Technically possible but architecturally problematic

**Approach**:
1. Spawn `strongswan` daemon process
2. Control via VICI (Versatile IKE Control Interface) protocol
3. Use `rustici` crate to communicate over Unix domain socket

**Libraries Available**:

#### `rustici` (v0.1.9)
- **Type**: Pure Rust client for strongSwan VICI protocol
- **Functionality**:
  - Encode/decode VICI messages (sections, lists, key/values)
  - Encode/decode VICI packets with transport framing
  - Blocking client over UnixStream for request/response
  - Event registration support
- **Features**:
  - Pure std, blocking I/O
  - UNIX-only (requires Unix domain sockets)
  - No external dependencies (doesn't depend on libstrongswan)
- **Last Updated**: 21 days ago (actively maintained)
- **Limitations**:
  - **Client-only**: Cannot spawn strongSwan daemon
  - **Unix-only**: Requires Unix domain socket (Linux, macOS, BSD)
  - **Requires running daemon**: strongSwan must already be running

#### `serde_vici` (v0.1.0)
- **Type**: Serde serialization for VICI protocol format
- **Functionality**: Serialize/deserialize Rust structs to VICI format
- **Use Case**: Simplifies building VICI messages

**Architecture Example**:
```rust
// Hypothetical implementation
pub async fn spawn_ipsec_server(port: u16) -> Result<()> {
    // 1. Generate configuration
    let config = generate_strongswan_config(port)?;
    write_config_file(&config)?;

    // 2. Spawn strongSwan daemon
    let mut child = Command::new("strongswan")
        .arg("start")
        .spawn()?;

    // 3. Connect to VICI interface
    let client = rustici::Client::connect("/var/run/charon.vici")?;

    // 4. Load connection configuration
    let conn_msg = build_connection_config()?;
    client.request("load-conn", conn_msg)?;

    // 5. Monitor via VICI events
    client.register_event("ike-updown")?;
    // ...
}
```

**VICI Capabilities**:
- Load/unload connection configurations
- Initiate/terminate connections
- Query status and statistics
- Rekey SAs
- List certificates
- Monitor IKE_SA and CHILD_SA events
- Install policies and routes

---

### Problems with strongSwan Daemon Approach

#### External Dependency
- ❌ **Requires strongSwan binary installed** on host system
- Breaks NetGet's single-binary philosophy
- Installation burden on users:
  - Linux: `apt install strongswan` or `yum install strongswan`
  - macOS: `brew install strongswan`
  - FreeBSD: `pkg install strongswan`
  - Windows: Not officially supported
- Version compatibility issues across platforms
- Different packages on different distros (strongswan, strongswan-charon, etc.)

#### Privilege Requirements
- ❌ **Requires root/administrator privileges** to run strongSwan
- strongSwan needs elevated permissions for:
  - XFRM kernel subsystem access
  - Netlink socket creation
  - Route table manipulation
  - IPSec policy installation
- Significantly increases security risk profile
- User experience friction (sudo prompts, privilege escalation)
- May trigger security warnings on modern systems

#### OS-Level Integration Complexity
- ⚠️ **Deep kernel integration required**
- **Linux**: XFRM subsystem
  - Complex netlink protocol (NETLINK_XFRM)
  - Undocumented and complex API
  - Multiple hash tables and red-black trees for SA/SP storage
  - Communication between userspace daemon and kernel modules
- **BSD**: Different IPSec stack (PF_KEY interface)
- **macOS**: Limited IPSec support, different APIs
- **Windows**: Completely different architecture (WFP - Windows Filtering Platform)

#### Limited LLM Control
- ⚠️ **VICI provides administrative control, not protocol-level control**
- LLM would control:
  - Connection loading/unloading
  - SA initiation/termination
  - Status queries
  - Configuration management
- LLM would NOT control:
  - IKE negotiation details
  - Per-packet SA selection
  - Encryption algorithm selection during handshake
  - Low-level protocol decisions
- **Contrast with WireGuard**: defguard_wireguard_rs provides protocol-level peer authorization, NetGet's WireGuard LLM controls peer acceptance at handshake time

#### Configuration Complexity
- ⚠️ **strongSwan configuration is extremely complex**
- Must generate:
  - `strongswan.conf` - Daemon configuration
  - `swanctl.conf` or `ipsec.conf` - Connection definitions
  - Certificate infrastructure (if using certificate auth)
  - Pre-shared keys (if using PSK auth)
  - Routing and policy configuration
- Configuration options:
  - IKE version (IKEv1/IKEv2)
  - Authentication method (PSK, RSA, EAP, X.509)
  - Encryption algorithms (AES-128/256, 3DES, ChaCha20)
  - Integrity algorithms (SHA1, SHA256, SHA384, SHA512)
  - DH groups (modp1024, modp2048, modp3072, curve25519, etc.)
  - IPsec mode (tunnel vs transport)
  - Perfect Forward Secrecy settings
  - NAT traversal options
  - Dead Peer Detection
  - Traffic selectors
- **High risk of misconfigurations** causing security vulnerabilities

#### Process Management
- ⚠️ **Daemon lifecycle management adds complexity**
- Must handle:
  - Process spawning and monitoring
  - Graceful shutdown
  - Crash recovery and restart
  - Log file rotation
  - PID file management
  - Socket file cleanup (/var/run/charon.vici)
  - Zombie process prevention
  - Signal handling (HUP, USR1, TERM)

#### Platform Portability
- ⚠️ **Different binary paths, configs, and architectures across platforms**

| Platform | Daemon | Config Path | Socket Path | Package |
|----------|--------|-------------|-------------|---------|
| Debian/Ubuntu | `/usr/sbin/strongswan` | `/etc/strongswan.d/` | `/var/run/charon.vici` | `strongswan` |
| RHEL/Fedora | `/usr/sbin/strongswan` | `/etc/strongswan/` | `/var/run/charon.vici` | `strongswan` |
| Arch Linux | `/usr/bin/strongswan` | `/etc/strongswan.d/` | `/run/charon.vici` | `strongswan` |
| FreeBSD | `/usr/local/sbin/strongswan` | `/usr/local/etc/strongswan.d/` | `/var/run/charon.vici` | `security/strongswan` |
| macOS | `/usr/local/sbin/strongswan` | `/usr/local/etc/strongswan.d/` | `/usr/local/var/run/charon.vici` | `strongswan` (Homebrew) |
| Windows | Not officially supported | N/A | N/A | N/A |

**Verdict**: Technically feasible but architecturally incompatible with NetGet's design principles

---

### 3. Alternative: Libreswan

**Status**: ⚠️ Similar problems to strongSwan

**Overview**:
- Fork of OpenSwan (successor to FreeS/WAN)
- ~100,000+ lines of code (OpenSwan had >8MB of code)
- Closer to original FreeS/WAN architecture than strongSwan
- Primarily used on RHEL/Fedora distributions

**Control Mechanism**:
- Uses older `ipsec` command interface (not VICI)
- No modern programmatic control API like VICI
- Configuration via `ipsec.conf` and `ipsec.secrets`

**Why Not Use Libreswan**:
- ❌ No Rust client library (no equivalent to rustici)
- ❌ Older architecture (less modular than strongSwan)
- ❌ Same external dependency problems
- ❌ Same privilege requirements
- ❌ Same configuration complexity
- ❌ Less flexible control interface

**Verdict**: Worse option than strongSwan for NetGet integration

---

## Comparison: WireGuard vs IPSec Implementation Approaches

| Aspect | WireGuard (Current) | IPSec (strongSwan via VICI) |
|--------|---------------------|------------------------------|
| **Library** | defguard_wireguard_rs | rustici + strongswan binary |
| **Integration** | Native Rust library | External process spawn |
| **Dependencies** | Pure Rust crate | System-installed strongSwan |
| **Privileges** | Required for TUN | Required for XFRM/kernel |
| **Single Binary** | ✅ Yes | ❌ No (requires strongswan) |
| **LLM Control** | Deep (protocol-level peer authorization) | Shallow (administrative commands) |
| **Configuration** | Programmatic API (Key, Peer, IpAddrMask) | Config file generation (swanctl.conf) |
| **Process Management** | N/A (library) | Complex (daemon lifecycle, signals) |
| **Lines of Code** | WireGuard: ~4,000 | strongSwan: ~100,000+ (est.) |
| **Protocol Complexity** | Simple, modern | Complex, legacy |
| **OS Integration** | TUN interface | XFRM (Linux), PF_KEY (BSD), varies |
| **Platform Support** | Linux, macOS, Windows, FreeBSD | Linux, BSD (macOS limited, no Windows) |
| **Cipher Negotiation** | Fixed (ChaCha20Poly1305, AES-GCM) | Complex (20+ algorithms to negotiate) |
| **Auth Methods** | Public key only | PSK, RSA, X.509, EAP, etc. |
| **NetGet Philosophy** | ✅ Aligned | ❌ Violates principles |

---

## Alternative Considered: Custom IPSec Protocol Implementation

**Proposal**: Implement minimal IPSec/IKEv2 subset from scratch in Rust

**Requirements**:

### IKEv2 Protocol Implementation
- IKE_SA_INIT exchange (4 messages)
  - HDR, SAi1, KEi, Ni → HDR, SAr1, KEr, Nr
- IKE_AUTH exchange (2 messages)
  - HDR, SK{IDi, AUTH, SAi2, TSi, TSr} → HDR, SK{IDr, AUTH, SAr2, TSi, TSr}
- CREATE_CHILD_SA exchange (rekeying)
- INFORMATIONAL exchange
- Payload parsing (14+ payload types):
  - Security Association (SA)
  - Key Exchange (KE)
  - Identification (IDi, IDr)
  - Certificate (CERT)
  - Certificate Request (CERTREQ)
  - Authentication (AUTH)
  - Nonce (Ni, Nr)
  - Notify (N)
  - Delete (D)
  - Vendor ID (V)
  - Traffic Selector (TSi, TSr)
  - Encrypted (SK)
  - Configuration (CP)
  - Extensible Authentication Protocol (EAP)

### Cryptographic Operations
- Diffie-Hellman key exchange (multiple groups: modp1024-8192, curve25519, curve448)
- PRF (Pseudorandom Function): HMAC-SHA1/256/384/512
- Integrity algorithms: HMAC-SHA1/256/384/512, AES-XCBC-96
- Encryption: AES-CBC (128/192/256), AES-GCM (128/256), ChaCha20Poly1305
- Authentication: RSA, ECDSA, Pre-Shared Key, EAP
- Certificate handling: X.509 parsing, validation, CRL checking

### ESP Implementation
- Encapsulating Security Payload (RFC4303)
- SPI (Security Parameter Index) management
- Sequence number tracking
- Anti-replay protection
- Packet encryption/decryption
- HMAC calculation and verification

### SA (Security Association) Management
- IKE_SA state machine
- CHILD_SA state machine
- SA database (SAD)
- Security Policy Database (SPD)
- Rekeying logic (soft/hard lifetimes)
- Delete/rekey collision handling

### OS Integration
- XFRM netlink interface (Linux)
- PF_KEY interface (BSD)
- Routing table manipulation
- Policy installation
- TUN/TAP interface creation
- NAT traversal (UDP encapsulation)

**Estimated Effort**:
- **Minimal viable implementation**: 12-18 months full-time development
- **Production-ready**: 24-36 months with security audits
- **Full feature parity**: 3-5 years

**Problems**:
- ❌ **Massive time investment** for marginal benefit
- ❌ **Security risk**: Cryptographic protocol implementation requires expert review
- ❌ **Maintenance burden**: Protocol evolves, requires ongoing updates
- ❌ **Testing complexity**: Interoperability with strongSwan, Libreswan, Cisco, etc.
- ❌ **Duplication of effort**: Would reinvent existing battle-tested implementations
- ❌ **Formal verification**: IPSec has not been formally verified due to complexity

**Comparison**:
- OpenVPN: 70,000 lines of code (still too complex to justify)
- strongSwan: ~100,000+ lines of code
- IPSec custom implementation: Estimated 50,000-80,000 lines minimum
- WireGuard: 4,000 lines (already implemented in NetGet)

**Verdict**: Not justified given WireGuard already provides full VPN functionality

---

## Protocol Complexity Analysis

### IPSec/IKEv2 Protocol Layers

```
┌─────────────────────────────────────────┐
│        IKEv2 (UDP port 500/4500)        │ ← Key exchange, auth, SA negotiation
├─────────────────────────────────────────┤
│              ESP/AH                      │ ← Packet encryption/integrity
├─────────────────────────────────────────┤
│             IP Layer                     │ ← Routing, policies
├─────────────────────────────────────────┤
│        XFRM (Linux Kernel)               │ ← Policy enforcement, SA management
└─────────────────────────────────────────┘
```

### IKEv2 Message Exchange Complexity

**Best Case**: 4 packets (IKE_SA_INIT + IKE_AUTH)
```
Initiator                    Responder
    │                             │
    │── IKE_SA_INIT (HDR, SA, KE, Ni) ──→│
    │←─ IKE_SA_INIT (HDR, SA, KE, Nr) ───│
    │                             │
    │── IKE_AUTH (HDR, SK{...}) ──→│
    │←─ IKE_AUTH (HDR, SK{...}) ───│
```

**Worst Case**: 30+ packets
- Multiple IKE_SA_INIT retries
- EAP authentication (multi-round)
- Multiple CHILD_SA creation
- Certificate chain exchanges
- Configuration payloads
- Vendor-specific extensions

### Configuration Complexity

**WireGuard Configuration**:
```rust
// 6 lines of code
let config = InterfaceConfiguration {
    name: "wg0".into(),
    prvkey: private_key.to_string(),
    addresses: vec!["10.0.0.1".parse()?],
    port: 51820,
    peers: vec![],
    mtu: Some(1420),
};
```

**strongSwan Configuration** (swanctl.conf):
```
# 40+ lines for basic tunnel
connections {
    my-vpn {
        version = 2
        proposals = aes256-sha256-modp2048,aes256-sha1-modp2048
        dpd_delay = 30s
        rekey_time = 4h
        local_addrs = 192.168.1.1
        remote_addrs = 192.168.2.1
        local {
            auth = psk
            id = vpn.example.com
        }
        remote {
            auth = psk
            id = client.example.com
        }
        children {
            my-tunnel {
                local_ts = 10.0.1.0/24
                remote_ts = 10.0.2.0/24
                esp_proposals = aes256-sha256-modp2048
                rekey_time = 1h
                dpd_action = restart
            }
        }
    }
}

secrets {
    ike-1 {
        id = vpn.example.com
        secret = "pre-shared-key-here"
    }
}
```

---

## Honeypot Implementation Status

**Current Implementation** (`src/server/ipsec/mod.rs`):
- ✅ UDP packet listener on port 500 (IKE) and 4500 (NAT-T)
- ✅ IKE header parsing (28 bytes):
  - Initiator SPI (8 bytes)
  - Responder SPI (8 bytes)
  - Next Payload (1 byte)
  - Version (1 byte)
  - Exchange Type (1 byte)
  - Flags (1 byte)
  - Message ID (4 bytes)
  - Length (4 bytes)
- ✅ Version detection (IKEv1 vs IKEv2)
- ✅ Exchange type recognition:
  - **IKEv2**: IKE_SA_INIT (34), IKE_AUTH (35), CREATE_CHILD_SA (36), INFORMATIONAL (37)
  - **IKEv1**: Identity Protection (2), Aggressive (4), Quick Mode (32), Informational (5)
- ✅ SPI extraction (Initiator and Responder)
- ✅ Handshake detection
- ✅ Logging and UI updates
- ✅ Connection tracking in NetGet state

**Honeypot Capabilities**:
- Detects IPSec reconnaissance attempts
- Logs IKE handshake initiation packets
- Records source IPs, SPIs, and exchange types
- Tracks repeated connection attempts
- Useful for security research and threat detection

**What It Cannot Do**:
- Complete IKE_SA_INIT exchange
- Perform Diffie-Hellman key exchange
- Decrypt encrypted payloads (SK)
- Establish CHILD_SAs
- Route ESP traffic
- Provide VPN connectivity

---

## Recommendations

### For NetGet Users

#### If You Need Full VPN Functionality:
✅ **Use WireGuard** - NetGet provides production-ready WireGuard VPN server with full LLM control

#### If You Need IPSec Specifically:
⚠️ **Use established solutions alongside NetGet**:
- **Linux**: strongSwan (`apt install strongswan-swanctl`)
- **RHEL/Fedora**: Libreswan (`yum install libreswan`)
- **BSD**: strongSwan via ports/packages
- **macOS**: strongSwan via Homebrew (limited support)
- **Windows**: Microsoft built-in IPSec (not strongSwan compatible)

NetGet's IPSec honeypot can still provide value for:
- Security research
- IKE reconnaissance detection
- Connection attempt logging
- Threat intelligence gathering

#### If You Need Enterprise VPN:
Consider modern alternatives:
- **WireGuard** (NetGet supported) - Modern, fast, secure
- **OpenVPN** - Mature, widely compatible
- **Tailscale** - WireGuard-based mesh VPN
- **ZeroTier** - Software-defined networking

### For NetGet Development

**Recommendation**: ✅ **Keep IPSec as honeypot-only implementation**

**Rationale**:
1. **WireGuard provides full VPN functionality** - No gap in NetGet's capabilities
2. **swanny too immature** - Experimental library, not production-ready
3. **External daemon approach violates architecture** - Breaks single-binary, pure-Rust philosophy
4. **Limited LLM control** - VICI interface doesn't enable protocol-level LLM decisions
5. **OS integration complexity** - XFRM, PF_KEY, platform-specific netlink interfaces
6. **Maintenance burden** - Config generation, process management, privilege handling
7. **User friction** - External dependencies, root requirements, complex configuration
8. **Development cost** - Massive effort (12-36 months) for marginal benefit over WireGuard

**Focus Instead On**:
- ✅ Enhancing WireGuard LLM features (traffic shaping, conditional routing, bandwidth limits)
- ✅ Improving honeypot detection capabilities across all protocols
- ✅ Implementing other high-value protocols (BGP completed, others pending)
- ✅ Adding WireGuard peer policy management (allowlists, denylists, time-based rules)

---

## Future Possibilities

### If IPSec Full Server Becomes Viable:

**Scenario 1**: swanny library matures
- **Condition**: Production-ready, API stable, full IKEv2 feature set
- **Timeline**: 12-24 months minimum
- **Action**: Re-evaluate for integration into NetGet
- **Likelihood**: Low-medium (depends on project momentum and funding)
- **Requirements**:
  - Full IKEv2 RFC7296 compliance
  - Certificate authentication support
  - IKE SA rekeying
  - Fragmentation support
  - Tested interoperability with strongSwan, Libreswan, Cisco
  - Security audit completed
  - Stable API with semantic versioning
  - Active community and maintenance commitment

**Scenario 2**: Alternative pure-Rust IPSec library emerges
- **Condition**: New library provides server functionality
- **Action**: Evaluate against swanny
- **Likelihood**: Very low (ecosystem favors WireGuard)

**Scenario 3**: User demand justifies external daemon approach
- **Condition**: Strong community need for IPSec specifically
- **Action**: Could implement as optional feature behind `ipsec-daemon` flag
- **Requirements**:
  - Clear documentation of external dependencies
  - Root privilege warnings
  - Platform-specific installation guides
  - Fallback if strongSwan binary not found
  - Config file generation utilities
  - Process monitoring and recovery
- **Likelihood**: Very low (WireGuard addresses VPN use cases)

---

## Technical Appendix

### IKEv2 Header Structure (28 bytes)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       IKE SA Initiator's SPI                  |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       IKE SA Responder's SPI                  |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Next Payload | MjVer | MnVer | Exchange Type |     Flags     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Message ID                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                            Length                             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### IKEv2 Exchange Types

| Value | Exchange Type | Description |
|-------|---------------|-------------|
| 34 | IKE_SA_INIT | Initial exchange, establishes IKE_SA |
| 35 | IKE_AUTH | Authentication, establishes first CHILD_SA |
| 36 | CREATE_CHILD_SA | Creates additional CHILD_SA or rekeys |
| 37 | INFORMATIONAL | Status, errors, deletes |

### IKEv1 Exchange Types (Legacy)

| Value | Exchange Type | Description |
|-------|---------------|-------------|
| 2 | Identity Protection | Main Mode |
| 4 | Aggressive | Aggressive Mode |
| 32 | Quick Mode | Establish CHILD_SA |
| 5 | Informational | Notifications, deletes |

### IKEv2 Payload Types

| Next Payload | Notation | Payload Name |
|--------------|----------|--------------|
| 33 | SA | Security Association |
| 34 | KE | Key Exchange |
| 35 | IDi | Identification - Initiator |
| 36 | IDr | Identification - Responder |
| 37 | CERT | Certificate |
| 38 | CERTREQ | Certificate Request |
| 39 | AUTH | Authentication |
| 40 | Ni, Nr | Nonce |
| 41 | N | Notify |
| 42 | D | Delete |
| 43 | V | Vendor ID |
| 44 | TSi | Traffic Selector - Initiator |
| 45 | TSr | Traffic Selector - Responder |
| 46 | SK | Encrypted and Authenticated |
| 47 | CP | Configuration |
| 48 | EAP | Extensible Authentication |

### ESP Header Structure

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|               Security Parameters Index (SPI)                 |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Sequence Number                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Payload Data (variable)                    |
~                                                               ~
|                                                               |
+               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|               |     Padding (0-255 bytes)                     |
+-+-+-+-+-+-+-+-+               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                               |  Pad Length   | Next Header   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         Integrity Check Value-ICV (variable)                  |
~                                                               ~
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Linux XFRM Netlink Interface

**Communication**:
- Netlink socket: `NETLINK_XFRM`
- User space → Kernel: `xfrm_netlink_rcv()`
- Message types: `XFRM_MSG_*` (50+ message types)

**Data Structures**:
- **SA Database (SAD)**: 3 hash tables per namespace
  - Indexed by: SPI, destination address, source address
  - Resizable hash tables
- **SP Database (SPD)**: Red-black trees + hash tables
  - Indexed by: namespace, IP family, direction, interface
  - Multiple trees per entry

**Complexity**:
- Undocumented API
- Complex policy matching
- Multi-module communication (kernel ↔ userspace)
- Network namespace isolation

### strongSwan VICI Commands

```
# Connection management
load-conn          Load connection configuration
unload-conn        Unload connection
list-conns         List all connections
get-conns          Get connection details

# SA management
initiate           Initiate SA
terminate          Terminate SA
rekey              Rekey SA
list-sas           List all SAs
list-policies      List installed policies

# Certificate management
load-cert          Load certificate
load-key           Load private key
list-certs         List certificates

# Monitoring
stats              Get IKE statistics
version            Get daemon version

# Events (registration)
ike-updown         IKE_SA up/down events
child-updown       CHILD_SA up/down events
log                Log message events
```

---

## Conclusion

After thorough evaluation of all available options, **NetGet should maintain IPSec/IKEv2 as a honeypot-only implementation**. The combination of:

- WireGuard providing full production VPN functionality in NetGet
- swanny library being experimental (very early stage, not production-ready)
- No mature pure-Rust IPSec server library available
- Architectural incompatibility of strongSwan daemon-based approach
- Massive complexity of XFRM/kernel integration
- Immense effort required for custom implementation (12-36 months)

...means that investing in full IPSec support would violate NetGet's design principles while providing minimal user value.

NetGet's IPSec honeypot remains valuable for:
- **Security research** - Detect IKE reconnaissance patterns
- **Threat detection** - Log connection attempts and SPIs
- **Threat intelligence** - Build attacker profiles
- **IKE protocol analysis** - Study handshake behavior

For production VPN needs, **WireGuard** in NetGet provides superior security, performance, and LLM integration compared to IPSec.

---

**Last Updated**: 2025-10-30
**Research Conducted By**: Claude (Sonnet 4.5)
**Crates Evaluated**: ipsec-parser, swanny (GitLab), rustici, serde_vici
**Daemons Evaluated**: strongSwan, Libreswan
**Decision**: Keep as honeypot only
