# WHOIS E2E Testing

## Test Strategy

Use TCP socket client to send WHOIS queries and validate responses.

**Client**: Manual TCP socket (no external `whois` client needed)
**Approach**: Black-box testing with comprehensive prompts

## LLM Call Budget

**Target**: < 5 LLM calls total

1. **Server startup** (1 call) - Initial LLM call to set up server
2. **Basic query test** (1 call) - Query for example.com
3. **Error handling test** (1 call) - Query for non-existent domain
4. **Multiple queries** (1 call) - Test persistent connection
5. **Close connection test** (1 call) - Test LLM-initiated close

## Runtime

**Estimated**: ~30-45 seconds

- Server startup: ~5-10s
- Each test case: ~5-8s per LLM call
- Cleanup: minimal

## Test Plan

### Test 1: Basic Domain Query

**Setup**: Server with instruction to respond with fake data
**Action**: Send `example.com\r\n`
**Validation**:

- Response contains "Domain Name: example.com"
- Response contains registrar information
- Response contains nameservers

### Test 2: Error Response

**Setup**: Same server
**Action**: Send `nonexistent-domain-xyz123.com\r\n`
**Validation**:

- Response contains "Error" or "not found"
- Connection remains open or closes gracefully

### Test 3: Multiple Queries on Same Connection

**Setup**: Server that keeps connections open
**Action**: Send multiple queries sequentially
**Validation**:

- All queries receive responses
- Connection stays open between queries
- Stats track multiple packets

### Test 4: Connection Close

**Setup**: Server instructed to close after first response
**Action**: Send query
**Validation**:

- Receive response
- Connection closes (EOF)
- Connection status updated to Closed

## Implementation Notes

### Efficient Testing

Reuse server instance across test cases by using comprehensive initial prompt:

```
WHOIS server on port 43
For example.com: respond with registrar "Test Registrar", registrant "Test Org", nameservers ns1/ns2.example.com
For unknown domains: return "Domain not found" error
Keep connections open for multiple queries
```

This allows testing multiple scenarios with a single server startup.

### Privacy Requirements

- All tests use `127.0.0.1` (localhost only)
- No external network access
- No real WHOIS queries
- Works completely offline

### Ollama Lock

Tests run with `--ollama-lock` flag to serialize LLM access when running concurrent tests.

## Known Issues

None currently identified.

## Test Execution

```bash
# Build first
./cargo-isolated.sh build --release --no-default-features --features whois

# Run WHOIS E2E tests
./cargo-isolated.sh test --no-default-features --features whois --test whois_e2e_test
```

## Test Implementation Checklist

- [ ] Feature gate: `#[cfg(all(test, feature = "whois"))]`
- [ ] Use `AVAILABLE_PORT` placeholder for dynamic port allocation
- [ ] Test helper to start WHOIS server with instruction
- [ ] Test helper to send query and read response
- [ ] Assert on response content (domain info, errors)
- [ ] Assert on connection stats
- [ ] Clean shutdown between tests
