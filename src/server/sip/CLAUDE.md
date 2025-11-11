# SIP Protocol Implementation

## Overview

SIP (Session Initiation Protocol) server implementing RFC 3261 for VoIP signaling. Handles user registration, call
setup/termination, and capability negotiation for voice/video sessions.

**Compliance**: RFC 3261 (SIP - Session Initiation Protocol)

**Protocol Purpose**: SIP is a text-based signaling protocol for creating, modifying, and terminating multimedia
sessions. It's the foundation for VoIP telephony, video conferencing, and unified communications.

## Library Choices

**Manual Implementation** - Complete SIP protocol parsing implemented from scratch

- **Why**: Available Rust SIP libraries (rsipstack, rsip) are either too complex for LLM integration or parser-only
- SIP is a text-based HTTP-like protocol, straightforward to parse manually
- Manual implementation provides full control over response generation for LLM

**Alternative Considered - rsipstack v0.2.52**:

- Full RFC 3261 compliant SIP stack with transaction/dialog management
- **Pros**: Handles protocol complexity (state machines, retransmissions, digest auth)
- **Cons**: Complex async architecture, requires deeper integration for LLM control points
- **Decision**: Start with manual implementation for simplicity, can migrate to rsipstack later for production features

**Text Protocol Handling**:

- Line-based parsing (similar to HTTP)
- Headers: `Name: Value\r\n` format
- Body: Optional SDP (Session Description Protocol) after blank line
- UTF-8 text encoding

## Architecture Decisions

### UDP-Based Protocol (with TCP option)

**Primary Transport: UDP on port 5060**:

- Connectionless: Each SIP request is independent transaction
- Stateless server possible: No mandatory session tracking
- Fast response: Single request-response round trip

**TCP Support** (future):

- Port 5060 (same as UDP)
- For large messages (>MTU) or reliable delivery
- Persistent connections for multiple transactions

**TLS Support** (future):

- Port 5061 (SIPS - SIP Secure)
- For encrypted signaling in production

**Connection Tracking**:

- Each SIP dialog (INVITE→ACK→BYE) creates a "connection" in NetGet UI
- Dialog ID = Call-ID + From tag + To tag
- Tracks state: Idle, Early (ringing), Confirmed (active), Terminated

### Message Format

**SIP Message Structure** (RFC 3261 Section 7):

```
Request Line:   METHOD sip:user@domain SIP/2.0
Headers:        Header-Name: Header-Value
                ...
Blank Line:
Body:           <SDP or other content>
```

**Example REGISTER Request**:

```
REGISTER sip:example.com SIP/2.0
Via: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK776asdhds
From: <sip:alice@example.com>;tag=1928301774
To: <sip:alice@example.com>
Call-ID: a84b4c76e66710@pc33.example.com
CSeq: 1 REGISTER
Contact: <sip:alice@192.0.2.1>
Expires: 7200
Content-Length: 0
```

**Example 200 OK Response**:

```
SIP/2.0 200 OK
Via: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK776asdhds
From: <sip:alice@example.com>;tag=1928301774
To: <sip:alice@example.com>;tag=a6c85cf
Call-ID: a84b4c76e66710@pc33.example.com
CSeq: 1 REGISTER
Contact: <sip:alice@192.0.2.1>
Expires: 3600
Content-Length: 0
```

### Core SIP Methods

**REGISTER**:

- Client registers current location (IP:port) with SIP server
- Server stores binding in location database
- Expires header controls registration lifetime (seconds)

**INVITE**:

- Initiates a session (phone call, video conference)
- Includes SDP body describing media capabilities (codecs, IP addresses)
- Response 200 OK also includes SDP (answer to offer)

**ACK**:

- Acknowledges final response to INVITE
- Completes 3-way handshake (INVITE → 200 OK → ACK)
- No SIP response to ACK (end of transaction)

**BYE**:

- Terminates active session
- Either party can send BYE
- Response is 200 OK

**OPTIONS**:

- Queries server capabilities
- Response includes Allow header with supported methods
- Used for presence checking ("is this user available?")

**CANCEL**:

- Cancels pending INVITE (before final response)
- Useful for "call ringing, user hung up" scenario

### Request Processing Flow

1. **Receive UDP packet** → Parse SIP message (text)
2. **Extract method** → Determine request type (REGISTER, INVITE, etc.)
3. **Parse headers** → Call-ID, From, To, Via, CSeq, Contact
4. **Parse body** (if present) → SDP for INVITE
5. **Create event** → Method-specific event (e.g., `SIP_REGISTER_EVENT`)
6. **LLM consultation** → Generate response (status code, headers, body)
7. **Build response** → Copy headers from request, add server headers
8. **Send UDP response** → To client's IP:port

### SDP (Session Description Protocol)

**Purpose**: Describes multimedia session parameters (codecs, IP addresses, ports)

**Example SDP in INVITE**:

```
v=0
o=alice 2890844526 2890844526 IN IP4 192.0.2.1
s=Call
c=IN IP4 192.0.2.1
t=0 0
m=audio 49170 RTP/AVP 0
a=rtpmap:0 PCMU/8000
```

**Fields**:

- `v=0`: SDP version
- `o=`: Originator (username, session ID, version, network type, IP)
- `s=`: Session name
- `c=`: Connection data (network type, address type, IP)
- `t=`: Timing (start and stop times, 0=unbounded)
- `m=`: Media description (type, port, protocol, format)
- `a=`: Attributes (codec mappings, etc.)

**LLM Generation**: For honeypot/testing, LLM generates simplified SDP with plausible values. No actual RTP media
streams created.

## LLM Integration

### Action-Based Response Generation

**LLM generates SIP responses** via actions for each method:

**REGISTER Example**:

```json
{
  "actions": [
    {
      "type": "sip_register",
      "status_code": 200,
      "reason_phrase": "OK",
      "expires": 3600
    }
  ]
}
```

**INVITE Example (Accept)**:

```json
{
  "actions": [
    {
      "type": "sip_invite",
      "status_code": 200,
      "reason_phrase": "OK",
      "sdp": "v=0\no=- 0 0 IN IP4 127.0.0.1\ns=NetGet Call\nc=IN IP4 127.0.0.1\nt=0 0\nm=audio 8000 RTP/AVP 0\na=rtpmap:0 PCMU/8000\n"
    }
  ]
}
```

**INVITE Example (Reject)**:

```json
{
  "actions": [
    {
      "type": "sip_invite",
      "status_code": 486,
      "reason_phrase": "Busy Here"
    }
  ]
}
```

**Action Parameters**:

- `status_code`: SIP response code (200=OK, 403=Forbidden, 486=Busy, etc.)
- `reason_phrase`: Optional (defaults based on status code)
- `expires`: For REGISTER responses (seconds)
- `sdp`: For successful INVITE responses
- `allow_methods`: For OPTIONS responses (array of method names)

### Event Types

**`SIP_REGISTER_EVENT`**:

- Triggered: Client sends REGISTER request
- Context:
    - `call_id`: Unique transaction identifier
    - `from`: Caller identity (SIP URI)
    - `to`: Callee identity (SIP URI)
    - `contact`: Client's current location (SIP URI with IP:port)
    - `expires`: Requested registration lifetime (seconds)
- LLM decides: Accept (200), Reject (403), Require Auth (401)

**`SIP_INVITE_EVENT`**:

- Triggered: Client initiates session with INVITE
- Context:
    - `call_id`: Dialog identifier
    - `from`: Caller
    - `to`: Callee
    - `sdp`: Session description (media offer)
- LLM decides: Accept (200 + SDP answer), Reject (486 Busy, 603 Decline), Redirect (302)

**`SIP_BYE_EVENT`**:

- Triggered: Client/server terminates session
- Context:
    - `call_id`: Dialog ID
    - `from`, `to`: Parties
- LLM decides: Always 200 OK (acknowledge termination)

**`SIP_ACK_EVENT`**:

- Triggered: Client acknowledges INVITE response
- Context: `call_id`, `from`, `to`
- LLM action: Update dialog state (passive observation, no response)

**`SIP_OPTIONS_EVENT`**:

- Triggered: Client queries capabilities
- Context: `call_id`, `from`, `to`
- LLM decides: Return supported methods (200 OK + Allow header)

**`SIP_CANCEL_EVENT`**:

- Triggered: Client cancels pending INVITE
- Context: `call_id`, `from`, `to`
- LLM decides: Acknowledge cancellation (200 OK)

## Connection and State Management

### Dialog Tracking

**Per-Dialog State** (`ProtocolConnectionInfo::Sip`):

```rust
Sip {
    dialog_id: Option<String>,    // Call-ID + From tag + To tag
    from: Option<String>,          // Caller SIP URI
    to: Option<String>,            // Callee SIP URI
    state: String,                 // idle/early/confirmed/terminated
    call_id: Option<String>,       // Call-ID header value
}
```

**Dialog States** (RFC 3261 Section 12):

- **Idle**: No dialog exists
- **Early**: INVITE sent, waiting for final response (180 Ringing)
- **Confirmed**: Session active (200 OK + ACK exchanged)
- **Terminated**: BYE sent/received, dialog closed

**Dialog Creation**: INVITE + 200 OK + ACK establishes confirmed dialog

### Registration Database

**In-Memory Storage**: HashMap<String, ContactBinding>

- Key: SIP URI (e.g., "sip:alice@example.com")
- Value: Contact URI (e.g., "sip:alice@192.0.2.1:5060"), Expires timestamp

**Registration Flow**:

1. Client sends REGISTER with Contact header
2. LLM decides to accept/reject
3. If accepted, store binding in database
4. Return 200 OK with Expires header
5. After expiration, remove binding

**Call Routing** (future):

- Lookup callee in registration database
- Forward INVITE to registered Contact URI
- Proxy mode: relay SIP messages between parties

## Protocol Validation

### Request Validation Checks

1. **Method Line**: Must be `METHOD sip:uri SIP/2.0`
2. **Required Headers**: Via, From, To, Call-ID, CSeq
3. **Via Header**: Must include branch parameter (starts with `z9hG4bK` for RFC 3261)
4. **CSeq Header**: Format is `number METHOD` (e.g., "1 REGISTER")
5. **Content-Length**: Must match body length if body present

**Invalid Requests**: Return 400 Bad Request (unlike STUN, SIP requires error responses)

### Response Generation

**Minimal Valid Response**:

```
SIP/2.0 200 OK
Via: <copied from request>
From: <copied from request>
To: <copied from request>;tag=<generated>
Call-ID: <copied from request>
CSeq: <copied from request>
Content-Length: 0
```

**Critical Headers to Copy**:

- **Via**: All Via headers in reverse order (response routing)
- **From**: Caller identity (unchanged)
- **To**: Callee identity (add tag if not present in request)
- **Call-ID**: Transaction/dialog identifier (unchanged)
- **CSeq**: Command sequence (unchanged)

**Server-Generated Headers**:

- **To tag**: Random hex string (identifies dialog)
- **Expires**: Registration lifetime (REGISTER responses)
- **Allow**: Supported methods (OPTIONS responses)
- **Contact**: Server's URI (REGISTER responses)

## Limitations

### Current Implementation

1. **UDP Only**
    - TCP/TLS transports not implemented
    - Max UDP packet size: 65535 bytes (sufficient for most SIP)

2. **Manual SIP Parsing**
    - Not using rsipstack or rsip crates
    - May have edge cases in header parsing
    - No automatic retransmission handling

3. **No Authentication**
    - RFC 2617 digest authentication not implemented
    - No 401 Unauthorized challenges
    - Accept/reject based on LLM decision only

4. **Simplified SDP**
    - No actual RTP media streams
    - LLM generates plausible SDP strings
    - No codec negotiation or media relay

5. **Stateless Server**
    - No persistent registration database (in-memory only)
    - No transaction retransmission tracking
    - No dialog state persistence across restarts

6. **Limited Method Support**
    - Core methods: REGISTER, INVITE, ACK, BYE, OPTIONS, CANCEL
    - No SUBSCRIBE/NOTIFY (presence)
    - No REFER (call transfer)
    - No UPDATE (session modification)

### Protocol Compliance Gaps

**RFC 3261 Features Not Implemented**:

- Transaction layer (retransmissions, timeouts)
- Digest authentication (MESSAGE-INTEGRITY, nonce, realm)
- Proxy/redirect server modes (forwarding, forking)
- TCP/TLS transports
- SIP over WebSocket (WebRTC)
- Presence (SUBSCRIBE/NOTIFY)
- Call transfer (REFER method)

**Impact**: Suitable for honeypot, testing, and basic call signaling. Not suitable for production VoIP service.

## Use Cases

### VoIP Honeypot

**Detect SIP Scanners**:

```
listen on port 5060 via sip

Log all REGISTER attempts with username, password, source IP.
Accept all REGISTER requests with 200 OK (fake successful registration).
For INVITE requests, accept and provide fake SDP.
Track attempted usernames, caller IDs, and target URIs.
```

**Typical Attacks**:

- SIP registration scanning (brute force username/password)
- Toll fraud (unauthorized calls through server)
- INVITE floods (DoS attacks)
- SIP message injection

### Basic Registrar Server

**User Registration**:

```
listen on port 5060 via sip

Accept REGISTER from:
- alice@localhost with any password → 200 OK, expires 3600
- bob@localhost with any password → 200 OK, expires 1800
Reject all other users → 403 Forbidden

For OPTIONS requests, return Allow: INVITE, ACK, BYE, REGISTER, OPTIONS
```

### Call Routing Server

**Simple Call Logic**:

```
listen on port 5060 via sip

Route incoming INVITE requests:
- From alice to bob → 200 OK with SDP (accept call)
- From bob to alice → 486 Busy Here (reject)
- From bob to charlie → 302 Moved Temporarily, Contact: sip:charlie@other-server.com
- All other calls → 404 Not Found
```

### Testing SIP Clients

**Comprehensive Test Server**:

```
listen on port 5060 via sip

REGISTER: Accept alice, bob, charlie with 200 OK
INVITE:
  - alice → bob: 180 Ringing, then 200 OK with SDP
  - bob → alice: 486 Busy Here
  - unknown → anyone: 403 Forbidden
OPTIONS: Return Allow: INVITE, ACK, BYE, REGISTER, OPTIONS, CANCEL
BYE: Always 200 OK
CANCEL: 200 OK (cancels pending INVITE)

Use scripting mode for deterministic responses (0 LLM calls per request).
```

## Performance Considerations

**Stateless Operation**: Each request O(1) processing (no database lookups in basic mode).

**Text Parsing Overhead**: Higher than binary protocols (STUN), but still <1ms per message.

**LLM Latency**: 500ms-5s per request. Acceptable for SIP signaling (not time-critical like RTP).

**Concurrent Requests**: Handled in parallel via tokio. Hundreds of requests/second possible.

**Memory Usage**: ~1KB per active dialog (in-memory state).

## Scripting Mode - Perfect Fit

**Why SIP is Ideal for Scripting**:
✅ Deterministic request-response protocol
✅ Limited method set (6 core methods)
✅ Well-defined status codes (RFC 3261 Section 21)
✅ Stateless server possible (no complex session management)
✅ Text-based (easy to generate from scripts)

**Example Scripting Logic** (Python):

```python
def handle_sip_request(event):
    method = event['method']
    from_uri = extract_user(event['from'])
    to_uri = extract_user(event['to'])

    if method == 'REGISTER':
        if from_uri in ['alice', 'bob']:
            return {'status_code': 200, 'expires': 3600}
        else:
            return {'status_code': 403}

    elif method == 'INVITE':
        if from_uri == 'alice' and to_uri == 'bob':
            return {
                'status_code': 200,
                'sdp': 'v=0\no=- 0 0 IN IP4 127.0.0.1\n...'
            }
        else:
            return {'status_code': 486}

    elif method == 'OPTIONS':
        return {
            'status_code': 200,
            'allow_methods': ['INVITE', 'ACK', 'BYE', 'REGISTER', 'OPTIONS']
        }

    elif method == 'BYE':
        return {'status_code': 200}

    # Fallback to LLM for unknown scenarios
    return {'fallback_to_llm': True}
```

**Expected Performance with Scripting**:

- Setup: 5-10s (LLM generates script)
- Per request: <1ms (script execution)
- **Total test time: ~10s for 10+ test cases**

## Example Prompts

### Basic SIP Registrar

```
Start a SIP server on port 5060. Accept REGISTER requests from any user.
Store registrations for 1 hour. Log all registration attempts.
```

### Call Routing with Auth

```
Start a SIP server on port 5060.

REGISTER:
  - alice@localhost → 200 OK
  - bob@localhost → 200 OK
  - Others → 403 Forbidden

INVITE:
  - alice → bob: Accept with SDP
  - bob → alice: 486 Busy Here
  - Others: 404 Not Found
```

### VoIP Honeypot

```
Start a SIP honeypot on port 5060.

Log all REGISTER attempts (username, password, source IP).
Accept all REGISTER with 200 OK (fake success).
For INVITE, accept with fake SDP.
Track attempted fraud (unusual destinations, high call rates).
```

## Security Considerations

**SIP Scanning**: Public SIP servers are heavily scanned for open relays. Rate limiting recommended.

**Authentication Bypass**: No digest auth means anyone can register. Fine for honeypot, not for production.

**Toll Fraud**: Attackers use open SIP servers to make expensive calls (international, premium numbers). Log all INVITE
destinations.

**SIP Message Injection**: Malformed SIP headers can crash parsers. Validate all headers.

**Amplification Attacks**: SIP responses ~same size as requests. Not suitable for DDoS amplification.

## References

- RFC 3261: SIP - Session Initiation Protocol (2002)
- RFC 3665: SIP Basic Call Flow Examples
- RFC 2617: HTTP Digest Authentication (used in SIP)
- RFC 4566: SDP - Session Description Protocol
- RFC 5389: STUN (complementary protocol for NAT traversal)
- SIP Security: https://datatracker.ietf.org/doc/html/rfc3261#section-26

## Future Enhancements

**Priority 1** (Promote to Beta):

- Digest authentication (401 challenges)
- TCP transport support
- Persistent registration database

**Priority 2** (Advanced Features):

- TLS (SIPS) on port 5061
- Proxy mode (forward INVITE to registered contacts)
- SUBSCRIBE/NOTIFY (presence)
- REFER (call transfer)

**Priority 3** (Production):

- Transaction retransmission handling
- Call state persistence
- WebSocket transport (WebRTC)
- Multi-party conferencing

## Migration to rsipstack

**When to Migrate**: If production features needed (auth, TCP, proxy mode)

**Migration Path**:

1. Replace manual parser with `rsip` crate
2. Use `rsipstack::Endpoint` for transaction management
3. Integrate LLM at transaction layer (decide response per transaction)
4. Keep action-based API unchanged (transparent to LLM)

**Estimated Effort**: 2-3 days for full rsipstack integration
