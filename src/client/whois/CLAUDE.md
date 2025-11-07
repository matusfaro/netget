# WHOIS Client Implementation

## Overview

The WHOIS client provides LLM-controlled domain and IP address lookups using the WHOIS protocol (RFC 3912). WHOIS is a simple TCP-based text protocol that runs on port 43.

## Protocol Details

**Port:** 43 (standard WHOIS)
**Transport:** TCP
**Format:** Plain text
**Pattern:** Single request-response (connection closes after response)

## Library Choices

**Network:** `tokio::net::TcpStream`
- Standard async TCP for connecting to WHOIS servers
- No external WHOIS libraries needed - protocol is very simple

**Protocol Implementation:** Direct TCP with manual text formatting
- Send: `<query>\r\n`
- Receive: Read until EOF (server closes connection)
- Response is plain text (no structured format)

## Architecture

### Connection Model

WHOIS is a one-shot protocol:
1. Client connects to server (port 43)
2. Client sends query (domain or IP) + "\r\n"
3. Server sends full response
4. Server closes connection

### LLM Integration Flow

```
User → open_client(whois, server:43, "Query example.com")
  ↓
1. TCP connect to server:43
  ↓
2. Call LLM with whois_connected event
  ↓
3. LLM returns query_whois(query="example.com")
  ↓
4. Send "example.com\r\n" to server
  ↓
5. Read full response until EOF
  ↓
6. Call LLM with whois_response_received event
  ↓
7. LLM parses response (registrar, dates, nameservers, etc.)
  ↓
8. Connection closes (status: Disconnected)
```

### State Machine

Unlike TCP/HTTP clients, WHOIS has no ongoing state:
- **Connected** → Send query → **Reading response** → **Disconnected**
- No idle/processing/accumulating states (single request only)

## LLM Control Points

### Actions

**Async Actions (user-triggered):**
- `query_whois` - Send WHOIS query for domain or IP
- `disconnect` - Close connection (usually not needed, auto-closes)

**Sync Actions (response to events):**
- None (WHOIS is one-shot, no response to responses)

### Events

1. **whois_connected** - Triggers after connection established
   - LLM responds with `query_whois` action
   - Parameters: `remote_addr`

2. **whois_response_received** - Triggers when full response received
   - LLM parses text response
   - Parameters: `response` (text), `query` (original query)

### Custom Action Results

- `ClientActionResult::Custom { name: "whois_query", data: { query: String } }`
  - Sent from `query_whois` action
  - Triggers query transmission

## Response Parsing

WHOIS responses are **unstructured text**. Format varies by registry:

**Domain WHOIS (example.com):**
```
Domain Name: EXAMPLE.COM
Registrar: Example Registrar, Inc.
Creation Date: 1995-08-14T04:00:00Z
Expiration Date: 2025-08-13T04:00:00Z
Name Server: ns1.example.com
Name Server: ns2.example.com
Status: clientTransferProhibited
```

**IP WHOIS (8.8.8.8):**
```
NetRange: 8.0.0.0 - 8.255.255.255
CIDR: 8.0.0.0/8
NetName: LVLT-ORG-8-8
NetHandle: NET-8-0-0-0-1
Organization: Level 3 Parent, LLC (LPL-141)
```

**LLM Parsing:** The LLM receives raw text and extracts relevant fields using pattern matching.

## Referrals

Some WHOIS servers redirect queries to authoritative servers:

**Example (querying whois.iana.org for example.com):**
```
refer: whois.verisign-grs.com
```

**Current Implementation:** No automatic referral following
**Future:** LLM could detect "refer:" and trigger new query to referred server

## WHOIS Servers

**Default Servers:**
- **IANA:** whois.iana.org (root registry, provides referrals)
- **Domain TLDs:**
  - .com/.net: whois.verisign-grs.com
  - .org: whois.pir.org
  - .io: whois.nic.io
- **IP Registries:**
  - ARIN (Americas): whois.arin.net
  - RIPE (Europe): whois.ripe.net
  - APNIC (Asia-Pacific): whois.apnic.net

**Server Selection:** User specifies server in `remote_addr` parameter

## Logging Strategy

**Dual Logging (tracing + status_tx):**

- **INFO:** Connection events (connected, disconnected)
  - `"WHOIS client 1 connected to whois.iana.org:43"`

- **DEBUG:** Query/response events
  - `"WHOIS client 1 querying: example.com"`
  - `"WHOIS client 1 received 1234 bytes"`

- **TRACE:** Full response text
  - `"WHOIS response:\n<full text>"`

- **ERROR:** Connection/read failures
  - `"WHOIS client 1 failed to send query: <error>"`

## Limitations

1. **No Structured Parsing:** Response is plain text, LLM must parse manually
   - Different registries use different formats
   - No standard schema (unlike DNS)

2. **No Referral Following:** If server returns "refer: <other-server>", manual re-query needed
   - Could be enhanced: LLM detects referral and opens new client

3. **Rate Limiting:** Many WHOIS servers rate-limit queries
   - Excessive queries may result in temporary IP bans
   - Best practice: Cache results, avoid repeated queries

4. **Server Availability:** Some WHOIS servers are unreliable
   - Timeouts, connection refused, incomplete responses
   - LLM should handle gracefully

5. **Single Query Per Connection:** WHOIS closes after one response
   - Cannot reuse connection for multiple queries
   - Need new client for each query

## Security Considerations

**Privacy:** WHOIS exposes domain registration information
- Registrant names, emails, addresses (unless redacted)
- Use responsibly, respect privacy

**Abuse:** WHOIS can be used for reconnaissance
- Domain enumeration, registrant tracking
- Consider ethical implications

## Example Prompts

**Basic Domain Query:**
```
Connect to WHOIS at whois.verisign-grs.com:43 and query "example.com"
```

**IP Lookup:**
```
Connect to WHOIS at whois.arin.net:43 and find information about 8.8.8.8
```

**Referral Following (manual):**
```
Query whois.iana.org for "example.com", then follow the referral to query the authoritative server
```

## Testing Notes

**E2E Testing:**
- Uses public WHOIS servers (whois.iana.org)
- Simple domain queries (example.com)
- No mock servers needed (real protocol)

**Rate Limit Concerns:**
- Keep test queries minimal (< 5 per test run)
- Use well-known domains (example.com, example.org)
- Avoid automated repeated testing

## Future Enhancements

1. **Automatic Referral Following:** Detect "refer:" in response, open new client
2. **Response Parsing Library:** Structured WHOIS response parser
3. **Multi-Query Support:** Connection pooling for multiple queries
4. **RDAP Support:** Modern alternative to WHOIS (RFC 7480, JSON-based)
