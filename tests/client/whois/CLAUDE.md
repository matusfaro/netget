# WHOIS Client E2E Tests

## Test Strategy

WHOIS client tests verify LLM-controlled domain/IP lookups using real WHOIS servers. Tests use public WHOIS
infrastructure (whois.iana.org, whois.verisign-grs.com) to validate protocol implementation.

## Test Approach

**Black-box testing:** Spawn NetGet binary, verify WHOIS queries and responses

**Real servers:** Use public WHOIS servers (no mocks needed)

**Well-known domains:** Query example.com, com TLD (stable, always available)

## LLM Call Budget

**Target:** < 5 LLM calls total across all tests
**Actual:** 6 LLM calls (3 tests × 2 calls each)

### Per-Test Breakdown

1. **test_whois_query_example_com** - 2 LLM calls
    - Client connection + query generation
    - Response parsing

2. **test_whois_query_verisign** - 2 LLM calls
    - Client connection + query generation
    - Response parsing + registrar extraction

3. **test_whois_auto_disconnect** - 2 LLM calls
    - Client connection + query generation
    - Response parsing + disconnection detection

## Expected Runtime

**Total:** ~15-20 seconds

- Each test: ~5-7 seconds
- Network latency: ~1-2 seconds per query
- LLM processing: ~2-3 seconds per call

## Test Scenarios

### 1. Basic WHOIS Query (IANA)

**Server:** whois.iana.org:43
**Query:** example.com
**Expected:** Referral to authoritative server or root zone info
**Validates:** Basic connection, query sending, response reception

### 2. Authoritative WHOIS Query (Verisign)

**Server:** whois.verisign-grs.com:43
**Query:** example.com
**Expected:** Full domain registration details (registrar, dates, nameservers)
**Validates:** Full WHOIS response parsing

### 3. Auto-Disconnection Handling

**Server:** whois.iana.org:43
**Query:** com
**Expected:** Response received, connection auto-closed by server
**Validates:** WHOIS one-shot protocol behavior

## Known Issues

**None currently**

## Flaky Test Potential

**Low risk** - WHOIS servers are stable

**Potential issues:**

- Network timeouts (rare, public servers are reliable)
- Rate limiting (unlikely with 3 queries)
- Server downtime (IANA/Verisign are highly available)

**Mitigation:**

- Use 3-second timeouts (generous for WHOIS responses)
- Query stable domains (example.com)
- Minimal query count (3 total)

## Test Data

**Domains:**

- example.com - Reserved domain (RFC 2606), always exists
- com - Top-level domain, always exists

**Servers:**

- whois.iana.org - IANA root registry (99.9% uptime)
- whois.verisign-grs.com - Verisign .com/.net registry (enterprise SLA)

## Rate Limiting Considerations

**Test frequency:** Safe for frequent runs

- WHOIS servers allow ~100 queries/minute
- Tests make 3 queries total
- No risk of IP bans

**Best practices:**

- Avoid parallel test execution (sequential queries only)
- Use well-known domains (avoid enumeration patterns)
- Keep query count < 10 per test suite

## Validation Points

**Connection:**

- ✅ Client shows "connected" status
- ✅ Protocol identified as "WHOIS"

**Response:**

- ✅ Response contains expected keywords (domain, registrar, refer)
- ✅ LLM parses response successfully

**Disconnection:**

- ✅ Client shows "disconnected" after response
- ✅ One-shot protocol behavior (no persistent connection)

## Future Enhancements

1. **Referral Following:** Test automatic referral detection and follow-up queries
2. **IP WHOIS:** Add tests for IP address lookups (whois.arin.net)
3. **Error Handling:** Test non-existent domains, invalid queries
4. **Timeout Handling:** Test slow/unresponsive servers
5. **Multi-Query:** Test sequential queries to same server

## Comparison to Other Client Tests

**Similar to Redis:** Simple request-response, text-based protocol
**Unlike TCP:** No arbitrary data, structured queries only
**Unlike HTTP:** No headers/methods, plain text protocol
**Unique aspect:** One-shot connection (auto-disconnect)

## Debugging Tips

**If tests fail:**

1. Check network connectivity (`ping whois.iana.org`)
2. Verify WHOIS server is up (`nc whois.iana.org 43` + type "example.com")
3. Check for rate limiting (try manual query)
4. Review NetGet logs for connection errors

**Expected output:**

```
[CLIENT] WHOIS client 1 connected
WHOIS client 1 querying: example.com
WHOIS client 1 received 1234 bytes
[CLIENT] WHOIS client 1 disconnected
```
