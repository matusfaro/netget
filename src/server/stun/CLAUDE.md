# STUN Protocol Implementation

## Overview

STUN (Session Traversal Utilities for NAT) server implementing RFC 8489 (STUN - Session Traversal Utilities for NAT).
Provides NAT traversal assistance by informing clients of their public IP address and port as seen from the internet.

**Compliance**: RFC 8489 (STUN), RFC 5389 (obsolete, but widely deployed)

**Protocol Purpose**: STUN allows clients behind NAT to discover their external IP address and port mapping, essential
for WebRTC, VoIP, and peer-to-peer applications.

## Library Choices

**Manual Implementation** - Complete STUN protocol parsing implemented from scratch

- **Why**: STUN is a simple binary protocol (20-byte header + attributes)
- No complex Rust STUN server libraries available that integrate with LLM control
- Manual implementation provides full control over response generation

**Binary Protocol Handling**:

- Manual parsing of STUN message headers (20 bytes fixed)
- Attribute parsing for extensibility
- Network byte order (big-endian) for all multi-byte fields

## Architecture Decisions

### UDP-Based Protocol

**STUN uses UDP exclusively** (no TCP variant in RFC 8489):

- Connectionless: Each binding request is independent
- Stateless server: No session tracking required
- Fast response: Single request-response round trip

**Connection Tracking**:

- Each STUN request creates a "connection" in NetGet UI
- Connection ID represents a single transaction
- Closes immediately after response sent

### Message Format

**STUN Message Structure** (RFC 8489 Section 6):

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|0 0|     STUN Message Type     |         Message Length        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Magic Cookie (0x2112A442)             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Transaction ID (96 bits)                  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Attributes (variable)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

**Message Type Encoding**:

- Method: Binding (0x0001)
- Class: Request (0x00), Success Response (0x01), Error Response (0x10)
- Encoding: 0bMMMMMMMMMMCCCCMM (M=method bits, C=class bits)

**Example**:

- Binding Request: 0x0001
- Binding Success Response: 0x0101
- Binding Error Response: 0x0111

### Request Processing Flow

1. **Receive UDP packet** → Parse STUN header
2. **Validate magic cookie** (0x2112A442) → Reject if invalid
3. **Extract transaction ID** (12 bytes) → Must echo in response
4. **Parse message type** → Determine method and class
5. **Create event** → `STUN_BINDING_REQUEST_EVENT` with peer_addr, transaction_id
6. **LLM consultation** → Generate STUN Binding Success Response
7. **Build response** → Copy transaction ID, add XOR-MAPPED-ADDRESS attribute
8. **Send UDP response** → To client's IP:port

### Attribute Handling

**Common STUN Attributes**:

- **XOR-MAPPED-ADDRESS** (0x0020): Client's public IP:port (XOR-encoded with magic cookie)
- **MAPPED-ADDRESS** (0x0001): Client's public IP:port (plain, legacy)
- **SOFTWARE** (0x8022): Server software identification (optional)
- **FINGERPRINT** (0x8028): CRC32 checksum (optional)

**XOR Encoding** (RFC 8489 Section 14.1):

- Port: XOR with upper 16 bits of magic cookie (0x2112)
- IPv4: XOR each byte with magic cookie (0x2112A442)
- IPv6: XOR with magic cookie + transaction ID

**Why XOR**: Prevents NATs from rewriting addresses embedded in STUN payloads.

## LLM Integration

### Action-Based Response Generation

**LLM generates complete STUN response** via action:

```json
{
  "actions": [
    {
      "type": "send_stun_binding_response",
      "transaction_id": "0102030405060708090a0b0c",
      "client_address": "203.0.113.5",
      "client_port": 54321,
      "xor_mapped": true,
      "software": "NetGet STUN/1.0"
    }
  ]
}
```

**Action Parameters**:

- `transaction_id`: Hex string (must match request)
- `client_address`: Public IP to return (usually peer_addr from UDP packet)
- `client_port`: Public port to return
- `xor_mapped`: true = XOR-MAPPED-ADDRESS, false = MAPPED-ADDRESS
- `software`: Optional SOFTWARE attribute value

**Action Execution**:

1. Parse action parameters
2. Decode transaction ID from hex
3. Build STUN Binding Success Response (0x0101)
4. Add XOR-MAPPED-ADDRESS attribute with XOR-encoded IP:port
5. Optionally add SOFTWARE attribute
6. Send as UDP datagram to client

### Event Type

**`STUN_BINDING_REQUEST_EVENT`**:

- Triggered: Valid STUN Binding Request received
- Context:
    - `peer_addr`: Client's IP:port as seen by server (their public address)
    - `local_addr`: Server's listening IP:port
    - `transaction_id`: Hex-encoded transaction ID (for response matching)
    - `message_type`: "BindingRequest"
    - `bytes_received`: Request size
- LLM decides: Generate Binding Success Response with XOR-MAPPED-ADDRESS

**No filtering logic**: All valid STUN requests receive responses (STUN is inherently public).

## Connection and State Management

**Per-Request State** (`ProtocolConnectionInfo::Stun`):

```rust
Stun {
    transaction_id: Option<String>, // Hex-encoded transaction ID
}
```

**Stateless Server**: No connection tracking beyond single request. Each request:

1. Creates connection entry in UI
2. Processes request
3. Sends response
4. Closes immediately

**No Session Management**: STUN has no concept of sessions or persistent connections.

## Protocol Validation

### Request Validation Checks

1. **Minimum Length**: At least 20 bytes (header only)
2. **Magic Cookie**: Bytes 4-7 must be 0x2112A442
3. **Message Type**: Must be Binding Request (class=0, method=1)
4. **Transaction ID**: Extract 12 bytes (bytes 8-19)

**Invalid Requests**: Silently ignored (no error response sent per RFC 8489).

### Response Generation

**Minimal Valid Response**:

```
[Message Type: 0x0101 (Success)]
[Message Length: 12 (XOR-MAPPED-ADDRESS attribute)]
[Magic Cookie: 0x2112A442]
[Transaction ID: <copy from request>]
[Attribute Type: 0x0020 (XOR-MAPPED-ADDRESS)]
[Attribute Length: 8 (family + port + IPv4)]
[Family: 0x01 (IPv4)]
[X-Port: port XOR 0x2112]
[X-Address: ip XOR 0x2112A442]
```

## Limitations

### Current Limitations

1. **IPv4 Only**
    - IPv6 XOR-MAPPED-ADDRESS not implemented
    - Could be added (XOR with magic cookie + transaction ID)

2. **No Authentication**
    - RFC 8489 defines MESSAGE-INTEGRITY and USERNAME attributes
    - Not implemented (STUN is typically used without auth in public servers)

3. **No TURN Support**
    - STUN only provides IP discovery
    - Does not relay traffic (use separate TURN server for relay)

4. **Minimal Attribute Support**
    - Only XOR-MAPPED-ADDRESS and optional SOFTWARE
    - No FINGERPRINT, REALM, NONCE, etc.

5. **No UDP Retransmission Handling**
    - STUN clients typically retry on timeout
    - Server doesn't track or deduplicate retries

### Protocol Compliance Gaps

**RFC 8489 Features Not Implemented**:

- Alternate Server (ALTERNATE-SERVER attribute)
- Error responses (400 Bad Request, 500 Server Error)
- Authentication (MESSAGE-INTEGRITY, USERNAME, REALM, NONCE)
- Fingerprint (FINGERPRINT attribute with CRC32)
- IPv6 support
- Backwards compatibility with RFC 3489/5389

**Impact**: Sufficient for basic WebRTC/STUN usage. Not suitable for enterprise deployments requiring authentication.

## Use Cases

### WebRTC NAT Traversal

**Typical Flow**:

1. WebRTC client sends STUN Binding Request to stun.example.com:3478
2. STUN server responds with client's public IP:port
3. Client includes this in ICE candidate exchange
4. Peer-to-peer connection established using discovered address

### VoIP Configuration

**SIP/SDP Integration**:

1. VoIP client behind NAT doesn't know public IP
2. Queries STUN server to discover public IP:port
3. Includes discovered address in SIP INVITE or SDP offer
4. Remote peer can reach client through NAT

### Network Diagnostics

**NAT Type Detection** (using STUN with additional queries):

- Full Cone NAT: Same mapping for all destinations
- Restricted Cone NAT: Port reuse, filtered by source IP
- Port Restricted Cone NAT: Port reuse, filtered by source IP:port
- Symmetric NAT: Different mapping per destination (STUN insufficient)

## Performance Considerations

**Stateless Operation**: Each request O(1) processing, no memory growth.

**UDP Overhead**: Minimal (20-byte header + ~12-byte attribute = 32 bytes response).

**LLM Latency**: 500ms-5s per request. Acceptable for STUN (not latency-sensitive like media).

**Concurrent Requests**: Handled in parallel via tokio. Thousands of requests/second possible (limited by LLM
throughput).

## Example Prompts

### Basic STUN Server

```
Start a STUN server on port 3478. For all binding requests, return the client's
public IP address and port.
```

### STUN with Custom Software Identifier

```
Start a STUN server on port 3478. Include SOFTWARE attribute "NetGet-STUN/1.0"
in all responses.
```

### Logging STUN Requests

```
Start a STUN server on port 3478. Log the source IP and port of every binding
request before responding.
```

### STUN with Response Delay (for testing)

```
Start a STUN server on port 3478. Wait 2 seconds before sending each response
(simulating high latency).
```

## Security Considerations

**Amplification Attack Potential**: STUN response (~32 bytes) is similar size to request (~20 bytes). Not suitable for
DDoS amplification.

**IP Spoofing**: UDP allows spoofed source IPs. Server should NOT trust client IP for authentication (none implemented
anyway).

**Rate Limiting**: Production deployments should rate-limit per source IP to prevent abuse.

## References

- RFC 8489: Session Traversal Utilities for NAT (STUN)
- RFC 5389: Session Traversal Utilities for NAT (obsolete, but widely deployed)
- RFC 5780: NAT Behavior Discovery Using STUN
- WebRTC STUN Usage: https://developer.mozilla.org/en-US/docs/Web/API/RTCIceServer
- STUN Message Structure: https://datatracker.ietf.org/doc/html/rfc8489#section-6
