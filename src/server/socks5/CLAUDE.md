# SOCKS5 Proxy Protocol Implementation

## Overview

SOCKS5 proxy server implementing RFC 1928 (SOCKS Protocol Version 5) with LLM-controlled connection filtering and optional Man-in-the-Middle (MITM) traffic inspection. Supports IPv4, IPv6, and domain name resolution with flexible authentication.

**Compliance**: RFC 1928 (SOCKS5), RFC 1929 (Username/Password Authentication)

## Library Choices

**Manual Implementation** - Complete SOCKS5 protocol implemented from scratch
- **Why**: Maximum flexibility for LLM control over authentication and connection decisions
- No Rust SOCKS5 server libraries provide the granular action-based control needed
- Allows custom filtering, MITM inspection, and policy enforcement

**Protocol Parsing**:
- Manual binary protocol parsing for SOCKS5 handshake, auth, and CONNECT requests
- Efficient byte-level operations with tokio AsyncReadExt/AsyncWriteExt
- Clear separation of handshake phases for debugging

## Architecture Decisions

### Four-Phase Connection Lifecycle

**Phase 1: Handshake (Authentication Method Negotiation)**
```
Client → Server: [VER=5, NMETHODS, METHODS...]
Server → Client: [VER=5, METHOD]
```
- Client proposes authentication methods (0x00=no auth, 0x02=username/password)
- Server selects method based on `Socks5FilterConfig.auth_methods`
- LLM not consulted (fast path)

**Phase 2: Authentication (if required)**
```
Client → Server: [VER=1, ULEN, USERNAME, PLEN, PASSWORD]
Server → Client: [VER=1, STATUS]
```
- If username/password selected, client sends credentials
- **LLM consulted** via `SOCKS5_AUTH_REQUEST_EVENT` to validate credentials
- LLM returns success/failure → Server sends 0x00 (success) or 0x01 (failure)

**Phase 3: CONNECT Request**
```
Client → Server: [VER=5, CMD=1, RSV=0, ATYP, DST.ADDR, DST.PORT]
Server → Client: [VER=5, REP, RSV=0, ATYP, BND.ADDR, BND.PORT]
```
- Client requests connection to target (IPv4, IPv6, or domain)
- **LLM consulted** via `SOCKS5_CONNECT_REQUEST_EVENT` (if filter matches)
- LLM decides: allow/deny + optional MITM flag
- Server sends success (0x00) or failure (0x02=connection not allowed)

**Phase 4: Data Relay**
- **Pass-through mode**: Direct bidirectional copy between client ↔ target (tokio::io::copy_bidirectional)
- **MITM mode**: Inspect each data chunk via LLM (`SOCKS5_DATA_TO_TARGET_EVENT`, `SOCKS5_DATA_FROM_TARGET_EVENT`)

### Filter Configuration System

**`Socks5FilterConfig`** controls LLM involvement:

```rust
pub struct Socks5FilterConfig {
    auth_methods: Vec<u8>,              // [0x00, 0x02] = support both no-auth and user/pass
    filter_mode: FilterMode,            // AllowAll, DenyAll, AskLlm, Selective
    target_host_patterns: Vec<String>,  // Regex patterns for selective filtering
    target_port_ranges: Vec<(u16, u16)>, // Port ranges for selective filtering
    default_action: String,             // "allow" or "deny" when not matching patterns
    mitm_by_default: bool,              // Enable MITM for allowed connections
}
```

**FilterMode Behavior**:
- `AllowAll`: All connections allowed without LLM consultation (fast proxy mode)
- `DenyAll`: All connections denied (honeypot/monitoring mode)
- `AskLlm`: Every connection consults LLM (maximum control)
- `Selective`: LLM consulted only when target matches `target_host_patterns` or `target_port_ranges`

This prevents LLM overhead for non-interesting traffic while maintaining control over sensitive destinations.

### Target Address Types

**`TargetAddr` enum** supports all SOCKS5 address types:

```rust
pub enum TargetAddr {
    Ipv4(Ipv4Addr, u16),      // ATYP=0x01: Direct IPv4 address
    Ipv6(Ipv6Addr, u16),      // ATYP=0x04: Direct IPv6 address
    Domain(String, u16),      // ATYP=0x03: Domain name (proxy resolves DNS)
}
```

Domain resolution performed by proxy, not client. This allows:
- DNS-based filtering (block connections to specific domains)
- DNS monitoring (log all domain lookups)
- DNS manipulation (redirect domains to honeypot servers)

## LLM Integration

### Action-Based Control

**Authentication Decision** (`SOCKS5_AUTH_REQUEST_EVENT`):
```json
{
  "actions": [
    {
      "type": "allow_socks5_auth",
      "username": "user123",
      "message": "Credentials valid"
    }
  ]
}
```
- LLM can validate credentials against any policy (database, LDAP, custom logic)
- ActionResult::NoAction = auth success
- ActionResult::CloseConnection = auth failure

**Connection Decision** (`SOCKS5_CONNECT_REQUEST_EVENT`):
```json
{
  "actions": [
    {
      "type": "allow_socks5_connect",
      "target": "example.com:443",
      "mitm": true,
      "message": "Allowing with inspection"
    },
    {
      "type": "deny_socks5_connect",
      "target": "malware-c2.com:8080",
      "reason": "Known malicious domain"
    }
  ]
}
```
- `mitm: true` → Enable MITM inspection for this connection
- `mitm: false` → Fast pass-through mode

**Data Inspection** (MITM mode only):

`SOCKS5_DATA_TO_TARGET_EVENT` (client → target):
```json
{
  "actions": [
    {
      "type": "forward_socks5_data",
      "data_base64": "...",
      "message": "Allowed"
    },
    {
      "type": "modify_socks5_data",
      "data_base64": "...",  // Modified data
      "message": "Redacted sensitive info"
    },
    {
      "type": "close_socks5_connection",
      "reason": "Blocked malicious payload"
    }
  ]
}
```

`SOCKS5_DATA_FROM_TARGET_EVENT` (target → client): Same structure

### Event Types

1. **`SOCKS5_AUTH_REQUEST_EVENT`**
   - Triggered: Username/password authentication phase
   - Context: username, password
   - LLM decides: Allow (NoAction) or Deny (CloseConnection)

2. **`SOCKS5_CONNECT_REQUEST_EVENT`**
   - Triggered: CONNECT request (if filter matches)
   - Context: target (host:port), username (if authenticated)
   - LLM decides: Allow (with optional MITM), Deny

3. **`SOCKS5_DATA_TO_TARGET_EVENT`** (MITM mode)
   - Triggered: Each data chunk from client to target
   - Context: data, target, username
   - LLM decides: Forward, Modify, Close

4. **`SOCKS5_DATA_FROM_TARGET_EVENT`** (MITM mode)
   - Triggered: Each data chunk from target to client
   - Context: data, target, username
   - LLM decides: Forward, Modify, Close

## Connection and State Management

**Per-Connection State** (`ProtocolConnectionInfo::Socks5`):
```rust
Socks5 {
    target_addr: Option<String>,       // e.g., "example.com:443"
    username: Option<String>,          // If authenticated
    mitm_enabled: bool,                // Whether MITM inspection active
    state: ProtocolState,              // Idle, Processing, Accumulating
    queued_data: Vec<Vec<u8>>,         // Data queued during LLM processing
}
```

**Connection Lifecycle**:
1. Accept TCP connection → Add to server connections
2. Phase 1: Handshake → Select auth method (fast)
3. Phase 2: Authentication (if needed) → LLM consultation
4. Phase 3: CONNECT request → LLM consultation (if filtered)
5. Connect to target server
6. Phase 4: Relay data (pass-through or MITM)
7. Connection closes → Mark as closed

**Concurrent Connections**: Each connection handled in separate tokio task. No limit enforced (production should add rate limiting).

## MITM Inspection Mode

### When to Use MITM

**Trade-off**: MITM adds significant latency (LLM call per data chunk) but provides complete visibility.

**Use Cases**:
- Malware analysis: Inspect all traffic to/from suspicious destinations
- Data exfiltration detection: Scan outbound data for sensitive patterns
- Protocol analysis: Decode and log application protocols (HTTP, custom protocols)
- Forensics: Capture full plaintext traffic for investigation

**Performance Impact**:
- Pass-through: ~1-2ms overhead per connection
- MITM: 500ms-5s per data chunk (2 LLM calls: to target, from target)

**Implementation**:
```rust
loop {
    tokio::select! {
        // Read from client
        result = client_stream.read(&mut buf) => {
            // Consult LLM, forward modified data to target
        }
        // Read from target
        result = target_stream.read(&mut buf) => {
            // Consult LLM, forward modified data to client
        }
    }
}
```

## Protocol Compliance

### Supported Features

- ✅ SOCKS5 handshake (RFC 1928)
- ✅ No authentication (0x00)
- ✅ Username/password authentication (0x02, RFC 1929)
- ✅ CONNECT command (0x01)
- ✅ IPv4 addresses (ATYP=0x01)
- ✅ IPv6 addresses (ATYP=0x04)
- ✅ Domain names (ATYP=0x03)

### Not Implemented

- ❌ BIND command (0x02) - Server listens for inbound connections
- ❌ UDP ASSOCIATE command (0x03) - UDP relay
- ❌ GSSAPI authentication (0x01)
- ❌ Other authentication methods

**Rationale**: CONNECT is 99% of real-world SOCKS5 usage. BIND/UDP ASSOCIATE rarely used, complex to implement correctly.

## Limitations

### Current Limitations

1. **No UDP Support**
   - Only TCP connections (CONNECT command)
   - UDP ASSOCIATE not implemented
   - Games, VoIP, and DNS-over-UDP won't work

2. **No BIND Support**
   - FTP active mode won't work
   - Some P2P protocols require BIND

3. **MITM Performance**
   - Each data chunk requires LLM consultation
   - Not suitable for high-throughput applications (video streaming, large downloads)
   - Consider pass-through mode for performance-sensitive traffic

4. **No Connection Pooling**
   - Each SOCKS5 connection creates new target connection
   - No HTTP/1.1 keep-alive equivalent
   - May exhaust ports under high load

### Security Considerations

**Authentication**: Username/password sent in plaintext over SOCKS5 connection (not TLS). Use SOCKS5 over SSH tunnel for encrypted credentials.

**MITM Mode**: Breaks end-to-end encryption if used with HTTPS/TLS (user sees cleartext). Use carefully and with user consent.

**DNS Leaks**: Proxy resolves domain names, preventing client DNS leaks. Good for privacy.

## Example Prompts

### Basic Proxy (No Auth, Allow All)
```
Listen on port 1080 using SOCKS5 stack with no authentication. Allow all connections.
```

### Allow All with Logging
```
Listen on port 1080 using SOCKS5 stack. Allow all connections but log the target
destination for each connection.
```

### Username/Password Authentication
```
Listen on port 1080 using SOCKS5 stack with username/password authentication.
Accept username "admin" with password "secret123". Deny all others.
```

### Selective Blocking
```
Listen on port 1080 using SOCKS5 stack. Block connections to *.facebook.com and
*.twitter.com. Allow all other connections.
```

### MITM Inspection for Specific Domain
```
Listen on port 1080 using SOCKS5 stack. For connections to api.suspicious.com,
enable MITM inspection and log all data. Use pass-through for all other connections.
```

### Port-Based Filtering
```
Listen on port 1080 using SOCKS5 stack. Allow connections to ports 80 and 443 only.
Block all other ports with reason "Only HTTP/HTTPS allowed".
```

### Honeypot Mode
```
Listen on port 1080 using SOCKS5 stack. Accept all connections but log full details
(source IP, username, target, all data). Enable MITM for all connections.
```

## Performance Considerations

**Pass-Through Mode**: Near-zero CPU overhead after connection establishment. Memory usage: 2x16KB buffers per connection.

**Selective Filtering**: Regex pattern matching adds ~10-50μs per connection.

**MITM Mode**: High latency (LLM calls). Not recommended for >10 concurrent connections or high-bandwidth usage.

**Concurrent Connections**: Tokio async allows thousands of connections. Practical limit: LLM throughput (1-10 queries/second).

## References

- RFC 1928: SOCKS Protocol Version 5
- RFC 1929: Username/Password Authentication for SOCKS V5
- SOCKS5 Protocol Specification: https://datatracker.ietf.org/doc/html/rfc1928
- Common SOCKS5 Issues: https://en.wikipedia.org/wiki/SOCKS#SOCKS5
