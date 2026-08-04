# SNMP Protocol Implementation

## Overview

SNMP agent implementing RFC 1157 (SNMPv1) and RFC 3416 (SNMPv2c) using the rasn-snmp library. Provides network
management functionality where the LLM controls OID responses (system info, interface stats, custom MIBs). Uses UDP
transport for request/response protocol.

**Status**: Beta (Network Management Protocol)
**RFC**: RFC 1157 (SNMPv1), RFC 3416 (SNMPv2c), RFC 3584 (SNMPv2 Coexistence)
**Port**: 161 (agent), 162 (trap receiver)
**Privilege**: declares `PrivilegeRequirement::PrivilegedPort(161)`; any port above 1023 needs none.
**Test coverage**: `tests/server/snmp/test.rs` (the file is `test.rs`, not `e2e_test.rs`), driving the
agent with the `snmp` crate's client.

## Library Choices

### Core SNMP Implementation

- **rasn-snmp v0.18** - Pure Rust SNMP protocol implementation
    - BER (Basic Encoding Rules) encoding/decoding
    - SNMPv1 and SNMPv2c message parsing
    - PDU types: GetRequest, GetNextRequest, GetBulkRequest, SetRequest, Response, Trap
    - Variable binding support (OID + value pairs)

- **rasn v0.18** - ASN.1 encoding library
    - BER encoder/decoder (used by rasn-snmp)
    - OID type representation
    - Integer, OctetString, and other ASN.1 types

- **Manual BER encoding** - Custom BER encoder for responses
    - Needed because rasn-snmp doesn't provide easy response building
    - Encodes: OID, Integer, OctetString, Counter, Gauge, TimeTicks
    - Length encoding goes through one `encode_length()` helper covering the short form and the
      0x81/0x82/0x83 long forms. This matters: the earlier per-site encoder only handled lengths
      below 256, so a sysDescr longer than 255 bytes — or simply a response with several variable
      bindings — emitted a truncated length byte and the client rejected the packet
    - Constructs complete SNMP response messages

**Rationale**: rasn-snmp is the most mature pure-Rust SNMP library with async support. However, it focuses on parsing (
client-side), not response building (server-side). We use it for parsing requests and then manually encode responses
using BER. This gives us full control over response format and allows the LLM to return simple JSON which we convert to
proper SNMP packets.

## Architecture Decisions

### 1. UDP Request/Response Model

SNMP uses UDP (stateless):

- Each request is independent
- No connection tracking (unlike TCP)
- Response must match request ID and community string
- Client may retry if no response (agent doesn't handle retries)

**Connection Tracking**:

- "Connection" = recent peer address that sent a request
- Recorded with `ProtocolConnectionInfo::empty()` — no SNMP-specific payload is attached
- Used for UI display only (not a protocol requirement), and never reaped

### 2. LLM Control Point

Single integration point:

**SNMP Requests**:

- LLM receives `snmp_request` event with request type (GetRequest, GetNextRequest, etc.) and requested OIDs
- Returns JSON with variables array: `[{oid, type, value}, ...]`
- Server encodes JSON to BER format and sends to client

**No Traps (Yet)**:

- Async action `send_trap` defined but not fully implemented
- Would require LLM-initiated UDP send to trap receiver
- Future enhancement for proactive notifications

### 3. JSON to BER Translation

LLM returns simple JSON, server converts to SNMP BER:

**LLM JSON Format**:

```json
{
  "variables": [
    {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "NetGet SNMP Agent"},
    {"oid": "1.3.6.1.2.1.1.3.0", "type": "timeticks", "value": 12345}
  ]
}
```

**BER Encoding**:

1. Parse OID string to numeric array
2. Encode value based on type (string → OctetString, integer → Integer, etc.)
3. Wrap in SEQUENCE for variable binding
4. Build complete GetResponse PDU with request ID and community
5. Wrap in outer SEQUENCE for SNMP message

**Supported Value Types**:

- `string` - OCTET STRING (tag 0x04)
- `integer` - INTEGER (tag 0x02)
- `counter` - Counter32 (application tag 0x41)
- `gauge` - Gauge32 (application tag 0x42)
- `timeticks` - TimeTicks (application tag 0x43)
- `null` - NULL (tag 0x05)

### 4. Protocol Version Support

Supports both SNMPv1 and SNMPv2c:

**Parsing**:

- Try v2c first (ber::decode::<v2c::Message>)
- Fall back to v1 if v2c parse fails
- Extract request type, OIDs, and community string

**Response**:

- Use same version as request
- Match request ID and community string
- Encode response with appropriate PDU type (GetResponse for v1, Response for v2c)

**Version Differences**:

- v1: GetRequest, GetNextRequest, SetRequest, GetResponse, Trap
- v2c: Adds GetBulkRequest, InformRequest, Report
- Community string same for both (simple authentication)

### 5. OID Parsing and Formatting

OID handling is critical for SNMP:

**OID Format**: Dotted decimal notation (e.g., "1.3.6.1.2.1.1.1.0")

**Common OIDs**:

- `1.3.6.1.2.1.1.1.0` - sysDescr (system description)
- `1.3.6.1.2.1.1.3.0` - sysUpTime (system uptime in timeticks)
- `1.3.6.1.2.1.1.5.0` - sysName (system name)
- `1.3.6.1.2.1.2.2.1.x.N` - ifTable (interface stats for interface N)

**OID Encoding**:

1. First two components encoded specially: `first * 40 + second`
2. Remaining components encoded as base-128 (7 bits per byte, MSB=1 for continuation)
3. Example: "1.3.6" → `[0x2B, 0x06]` (43, 6)

**Validation**: `encode_oid` rejects an OID whose first component is above 2, whose second component
is 40 or more (when the first is 0 or 1), or that contains a non-numeric component. A non-numeric
component used to be dropped silently, which turned a typo into a different, valid-looking OID.

### 6. Error Handling

SNMP has built-in error responses:

**Error Status Codes**:

- 0: noError
- 1: tooBig (response too large)
- 2: noSuchName (OID not found)
- 3: badValue (invalid value in SetRequest)
- 4: readOnly (tried to set read-only OID)
- 5: genErr (generic error)

**Error Responses**:

- `send_snmp_error` takes `error_message` (logs only — SNMP has no field for text), an optional
  `error_status` given as a name (`noSuchName`, `badValue`, `readOnly`, `tooBig`, `genErr`) or as
  the matching code 0-5, and an optional 1-based `error_index`
- The reply carries that status with empty variable bindings
- Omitting `error_status` yields genErr (5), the previous fixed behaviour

### 7. Dual Logging

All operations use **dual logging**:

- **DEBUG**: Request summary (request type, OIDs, peer address)
- **TRACE**: Full packet hex dump (both request and response)
- **INFO**: LLM messages and high-level events
- **ERROR**: Parse failures, encoding errors, LLM errors
- All logs go to both `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model

The LLM responds to SNMP events with actions:

**Events**:

- `snmp_request` - SNMP request received
    - Parameters: `request_type` (`GetRequest` / `GetNextRequest` / `GetBulkRequest` /
      `SetRequest` — note the exact spelling, it is dhcproto-style CamelCase, not `GET`), `oids`,
      `community`, `request_id`, `version` (`v1` / `v2c`), `client_ip`

**Request/response correlation**: the request-id, community string and version are captured
per-request in the socket task and passed into `build_snmp_response` as plain locals — they are
never read from shared state, so overlapping requests cannot cross-contaminate. The LLM does not
need to (and cannot) set them.

**Available Actions**:

- `send_snmp_response` - Send SNMP response with variable bindings
- `send_snmp_error` - Send error response with a chosen error-status
- `ignore_request` - Don't respond (client will timeout and retry)
- `send_trap` - **not implemented**: the executor validates its arguments and returns JSON that the
  server drops. No datagram is sent. Its description says so
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Responses

**Simple GET Response**:

```json
{
  "actions": [
    {
      "type": "send_snmp_response",
      "variables": [
        {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "NetGet SNMP Agent v1.0"}
      ]
    }
  ]
}
```

**Multiple OIDs (GetBulkRequest)**:

```json
{
  "actions": [
    {
      "type": "send_snmp_response",
      "variables": [
        {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "System Description"},
        {"oid": "1.3.6.1.2.1.1.3.0", "type": "timeticks", "value": 12345},
        {"oid": "1.3.6.1.2.1.1.5.0", "type": "string", "value": "netget.local"}
      ]
    }
  ]
}
```

**Error Response**:

```json
{
  "actions": [
    {
      "type": "send_snmp_error",
      "error_message": "OID not found"
    }
  ]
}
```

**Trap (Future)**:

```json
{
  "actions": [
    {
      "type": "send_trap",
      "target": "192.168.1.100:162",
      "variables": [
        {"oid": "1.3.6.1.4.1.99999.1.0", "type": "string", "value": "Alert: Service down"}
      ]
    }
  ]
}
```

## Connection Management

### Stateless Protocol

SNMP has no connection concept:

- Each UDP packet is independent request
- No handshake, no session, no connection state
- Community string is only "authentication" (plaintext)

### Request Processing Flow

1. Receive UDP packet on port 161
2. Parse as SNMPv2c or v1 (try v2c first)
3. Extract request type, OIDs, community string, request ID
4. Create `snmp_request` event with request data
5. Call LLM with event
6. LLM returns JSON with variables or error
7. Build BER-encoded SNMP response
8. Send UDP packet to peer address (same socket, different destination)

### Concurrent Requests

- Each request spawned in separate tokio task
- No queueing (unlike TCP protocols)
- Requests from different clients processed in parallel
- Ollama lock serializes LLM calls but not UDP I/O

## Known Limitations

### 1. No SNMPv3 Support

- Only SNMPv1 and SNMPv2c supported
- No encryption (all data plaintext)
- No authentication beyond community string
- No user-based security

**Rationale**: v1/v2c cover 90% of SNMP deployments. v3 adds significant complexity (encryption, user management, key
exchange).

### 2. No SET Request Implementation

- SetRequest parsed but not fully implemented
- LLM receives event but can't modify persistent state
- Would require state management (in-memory or persistent store)

**Workaround**: LLM can acknowledge SET but won't persist changes. Useful for honeypot scenarios (log SET attempts).

### 3. No Trap Sending

- `send_trap` is advertised as an async action, parses `target` and `variables`, and then returns
  JSON that nobody sends anywhere. Nothing reaches the network
- Implementing it means encoding a Trap/Trap-v2 PDU (a different PDU shape from GetResponse) and
  opening an outbound socket, plus a decision about where traps may be sent
- Until then the action's own description says NOT IMPLEMENTED, so the LLM is not misled

### 4. No MIB Loading

- No MIB file parsing
- LLM must know OIDs (can be provided in prompt)
- No OID name resolution (e.g., "sysDescr" → "1.3.6.1.2.1.1.1.0")

**Workaround**: Include common OIDs in prompt or rely on LLM's training data.

### 5. No SNMP Table Walking

- GetNextRequest supported but LLM must track "next" OID
- No automatic table iteration
- LLM must understand SNMP table structure

**Workaround**: Provide clear instructions about table structure in prompt.

### 6. No Timeout/Retry Logic

- Agent doesn't track if client retries request
- Each retry treated as new independent request
- Could result in duplicate LLM calls for same OID

**Rationale**: SNMP client handles retries, agent just responds to each request.

## Example Prompts

### Basic SNMP Agent

```
listen on port 161 via snmp
Respond to SNMP GET requests:
- 1.3.6.1.2.1.1.1.0 (sysDescr): "NetGet SNMP Agent v1.0 running on Linux"
- 1.3.6.1.2.1.1.3.0 (sysUpTime): 123456 timeticks
- 1.3.6.1.2.1.1.5.0 (sysName): "netget-server.local"
```

### Network Interface Agent

```
listen on port 161 via snmp
Simulate network interface eth0:
- 1.3.6.1.2.1.2.2.1.1.1 (ifIndex): 1
- 1.3.6.1.2.1.2.2.1.2.1 (ifDescr): "eth0"
- 1.3.6.1.2.1.2.2.1.3.1 (ifType): 6 (ethernetCsmacd)
- 1.3.6.1.2.1.2.2.1.5.1 (ifSpeed): 1000000000 (1 Gbps)
- 1.3.6.1.2.1.2.2.1.8.1 (ifOperStatus): 1 (up)
```

### Custom Enterprise MIB

```
listen on port 161 via snmp
Support custom enterprise OID tree 1.3.6.1.4.1.12345:
- 1.3.6.1.4.1.12345.1.1.0: "MyApp Server v2.3"
- 1.3.6.1.4.1.12345.1.2.0: 42 (active connections counter)
- 1.3.6.1.4.1.12345.1.3.0: "running" (server status)
For unknown OIDs, return error "noSuchName"
```

### SNMP Honeypot

```
listen on port 161 via snmp
Log all SNMP requests (community strings, OIDs queried)
Respond with realistic but fake data:
- System: "Cisco IOS Router 12.4"
- Interfaces: eth0 (1 Gbps), eth1 (1 Gbps)
- Uptime: incrementing value (starts at 1000000 timeticks)
Track which OIDs are queried most frequently
```

## Performance Characteristics

### Latency

- Parse request: <1ms (BER decoding)
- LLM processing: 2-5s (typical)
- Encode response: <1ms (BER encoding)
- Total: ~2-5s per request (LLM dominates)

### Throughput

- Limited by LLM response time
- Concurrent requests handled in parallel (each in own tokio task)
- UDP has no connection overhead (faster than TCP)

### Concurrency

- Unlimited concurrent requests (bounded by system resources)
- Each request processed independently
- Ollama lock serializes LLM calls but not UDP I/O or BER encoding

### Memory

- One 64KB receive buffer per server (reused across requests), plus a copy of each datagram
- BER encoding allocates small vectors (<1KB typical)
- No persistent state per client (stateless protocol)

## Security Considerations

### Plaintext Protocol

- SNMPv1/v2c has no encryption
- Community string sent in cleartext (like password)
- Anyone on network can sniff packets and see data

### Community String

- Acts as shared password
- Typically "public" for read-only, "private" for read-write
- No per-user authentication
- Easily brute-forced or sniffed

### Honeypot Usage

SNMP commonly targeted by attackers:

- Port 161 scanned frequently
- Default community strings ("public", "private") tried
- OIDs queried to fingerprint device type
- LLM can log attempts and adapt responses

## References

- [RFC 1157: SNMP (SNMPv1)](https://datatracker.ietf.org/doc/html/rfc1157)
- [RFC 3416: SNMPv2 Protocol Operations](https://datatracker.ietf.org/doc/html/rfc3416)
- [RFC 3584: SNMPv2 to SNMPv1 Coexistence](https://datatracker.ietf.org/doc/html/rfc3584)
- [rasn-snmp Documentation](https://docs.rs/rasn-snmp/latest/rasn_snmp/)
- [SNMP OID Reference](http://www.oid-info.com/)
- [Net-SNMP Tools](http://www.net-snmp.org/) - Command-line SNMP client for testing
