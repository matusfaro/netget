# WHOIS Protocol Implementation

## Overview

WHOIS (RFC 3912) server for domain lookup queries. The LLM responds with domain registration information using
structured actions.

**Status**: Beta
**RFC**: RFC 3912
**Port**: 43 (TCP)
**Stack**: ETH>IP>TCP>WHOIS

## Protocol Description

WHOIS is a simple TCP-based query/response protocol used to query databases that store information about registered
internet resources, particularly domain names and IP addresses.

**Protocol Flow**:

1. Client connects to server on TCP port 43
2. Client sends query (domain name or IP address) terminated by CRLF
3. Server responds with registration information in text format
4. Server may close connection or wait for additional queries
5. Client closes connection when done

**Query Format**: `<domain>\r\n`
**Response Format**: Free-form text with field:value pairs

## Library Choices

### Server Implementation

- **Manual TCP connection handling** - No external library
    - WHOIS is simple enough for direct implementation
    - Line-based text protocol (queries end with CRLF)
    - Single response per query
    - No binary parsing needed

**Rationale**: Using manual TCP handling gives us complete control over the protocol without adding dependencies. The
protocol is straightforward enough that a library provides minimal value.

## Architecture Decisions

### 1. Action-Based LLM Control

The LLM responds to queries with these actions:

- **`send_whois_response`** - Send custom response text
    - Full control over response format
    - For non-standard responses

- **`send_whois_record`** - Send formatted domain record
    - Structured parameters: domain, registrar, registrant, admin_contact, name_servers
    - Automatically formats standard WHOIS output

- **`send_error`** - Send error message
    - For domain not found or invalid queries

- **`close_connection`** - Close the connection
    - Allows LLM to terminate connection after response

### 2. TCP Connection Management

**Per-connection Handling**:

- Each client connection runs in its own task
- Connection object tracked in `ServerInstance`
- Stats tracked: bytes sent/received, packets sent/received, last activity
- Recent queries stored in `ProtocolConnectionInfo::Whois`

**Connection Lifecycle**:

1. Accept connection → Create `ConnectionId` → Add to `ServerInstance`
2. Loop: Read query → Call LLM → Send response
3. Close → Update status to `ConnectionStatus::Closed`

### 3. Dual Logging

Every operation logs to **both** tracing macros and `status_tx`:

```rust
// DEBUG: Query summary
debug!("WHOIS received {} bytes from {}", n, peer_addr);
let _ = status_tx.send(format!("[DEBUG] WHOIS received {} bytes from {}", n, peer_addr));

// TRACE: Full payload text
trace!("WHOIS query data: {}", query_str);
let _ = status_tx.send(format!("[TRACE] WHOIS query data: {}", query_str));

// INFO: LLM messages
info!("{}", message);
let _ = status_tx.send(format!("[INFO] {}", message));

// ERROR: Failures
error!("WHOIS read error: {}", e);
let _ = status_tx.send(format!("[ERROR] WHOIS read error: {}", e));
```

**Logging Levels**:

- **ERROR**: Critical failures (accept error, read/write errors)
- **WARN**: Non-fatal issues (unused currently)
- **INFO**: User-facing messages, LLM responses
- **DEBUG**: Operation summaries (bytes sent/received, LLM calls)
- **TRACE**: Full payloads (queries, responses)

### 4. Event System

**Sync Events** (network-triggered):

- `whois_query` - Triggered when client sends query
    - Parameters: `query` (string)
    - Available actions: all WHOIS actions

**No Async Events**: WHOIS has no user-triggered actions.

### 5. Connection Tracking

**ProtocolConnectionInfo::Whois**:

```rust
Whois {
    recent_queries: Vec<(String, Instant)>, // (domain, timestamp)
}
```

Stores last 10 queries per connection for UI display and debugging.

## LLM Integration

### Prompt Structure

The LLM receives:

1. Server instruction (from user)
2. Event type and parameters
3. Available actions
4. Connection context (recent queries)

### Example Interaction

**User Instruction**:

```
WHOIS server on port 43 - respond with fake registrar info for any domain
```

**Client Query**: `example.com\r\n`

**LLM Response**:

```json
{
  "type": "send_whois_record",
  "domain": "example.com",
  "registrar": "Example Registrar Inc.",
  "registrant": "Example Organization",
  "admin_contact": "admin@example.com",
  "name_servers": ["ns1.example.com", "ns2.example.com"]
}
```

**Server Output**:

```
Domain Name: example.com
Registrar: Example Registrar Inc.
Registrant Name: Example Organization
Admin Name: admin@example.com
Name Server: ns1.example.com
Name Server: ns2.example.com

```

## Known Limitations

### 1. No WHOIS+ (RFC 1835)

- WHOIS++ (RFC 1835) not supported
- No advanced query syntax
- No templates or structured queries

### 2. No Referral Handling

- No WHOIS server redirection
- No referral URLs
- Single-server responses only

### 3. No Authentication

- No access control lists
- All queries answered for all domains
- No rate limiting built into protocol layer

### 4. No Internationalization

- ASCII-only queries and responses
- No IDN (Internationalized Domain Names) support
- No multi-language responses

### 5. Simplified Protocol

- No thick/thin WHOIS distinction
- No privacy/GDPR redaction logic
- Fake data only (not a real WHOIS database)

## Security Considerations

1. **Public Port 43** - Standard WHOIS port, requires root/admin privileges
2. **No Rate Limiting** - Protocol layer doesn't enforce query limits (can be added via LLM instruction)
3. **Information Disclosure** - LLM controls what data is revealed
4. **Resource Exhaustion** - Each connection consumes resources until closed

## Example Prompts

### Basic Domain Server

```
WHOIS server on port 43
Respond with fake registration info for any .com domain
Include registrar "Example Registrar" and nameservers ns1/ns2.example.com
```

### Error-Only Server

```
listen on whois port 43
Return "Domain not found" error for all queries
```

### Selective Response

```
WHOIS server port 43
For example.com: show full registration details
For other domains: return error message
```

## Testing

See `tests/server/whois/CLAUDE.md` for E2E testing strategy.

## References

- [RFC 3912: WHOIS Protocol Specification](https://datatracker.ietf.org/doc/html/rfc3912)
- [RFC 1835: Architecture of the WHOIS++ service](https://datatracker.ietf.org/doc/html/rfc1835) (not implemented)
- [IANA WHOIS Service](https://www.iana.org/whois)
