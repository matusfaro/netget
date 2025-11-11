# HTTP/2 E2E Testing

## Overview

End-to-end tests for the HTTP/2 protocol implementation using real HTTP/2 clients (reqwest with HTTP/2 prior knowledge).

## Test Strategy

Black-box testing approach where:

1. NetGet binary is spawned with HTTP/2 prompts
2. Real HTTP/2 client (reqwest) connects and sends requests
3. Responses are validated for correctness (status, headers, body)
4. HTTP/2-specific features like multiplexing are tested

## Client Library

- **reqwest** - HTTP client with HTTP/2 support
- Uses `http2_prior_knowledge()` for cleartext HTTP/2 (h2c)
- No TLS/ALPN negotiation required for testing
- Validates HTTP/2 version in responses

## Test Suite

### Test 1: Basic GET Requests (`test_http2_basic_get_requests`)

**Purpose**: Verify basic HTTP/2 request-response cycle with multiple routes

**Scenario**:

- Start HTTP/2 server with 4 routes (/, /api/users, /api/status, /nonexistent)
- Send GET requests to each route
- Validate status codes, HTTP/2 version, and response bodies
- Verify 404 handling for unknown routes

**LLM Calls**: 1 (server startup)

**Runtime**: ~5-7 seconds

- Server startup: 2-3s
- LLM processing: 2s
- Requests: 4 × ~0.5s = 2s

**Key Assertions**:

- All responses use HTTP/2 (not HTTP/1.1)
- Status codes match expected (200, 404)
- JSON responses parse correctly
- Content-Type headers set appropriately

### Test 2: POST with Body (`test_http2_post_with_body`)

**Purpose**: Verify HTTP/2 POST requests with request bodies

**Scenario**:

- Start HTTP/2 server with POST endpoints (/echo, /api/users)
- Send POST with text body to /echo
- Send POST with JSON body to /api/users
- Validate response includes request data

**LLM Calls**: 1 (server startup)

**Runtime**: ~5-6 seconds

- Server startup: 2-3s
- LLM processing: 2s
- POST requests: 2 × ~0.5s = 1s

**Key Assertions**:

- POST bodies received and processed by LLM
- 201 Created status for user creation
- Response contains data from request (echo or user name)
- HTTP/2 version confirmed

### Test 3: Multiplexing (`test_http2_multiplexing`)

**Purpose**: Verify HTTP/2 multiplexing (concurrent requests over single TCP connection)

**Scenario**:

- Start HTTP/2 server with simple JSON endpoint
- Send 3 concurrent GET requests using same client
- Validate all requests succeed simultaneously

**LLM Calls**: 1 (server startup) + 3 (concurrent requests, but over same connection)

**Runtime**: ~7-8 seconds

- Server startup: 2-3s
- 3 concurrent LLM calls: ~4-5s (processed in parallel by different LLM calls)

**Key Assertions**:

- All 3 requests return 200 OK
- All use HTTP/2 protocol
- Requests processed concurrently (HTTP/2 multiplexing benefit)

## Total LLM Call Budget

**Total LLM Calls**: 6

- Test 1: 1 (startup) + 4 (requests) = 5 calls
- Test 2: 1 (startup) + 2 (requests) = 3 calls
- Test 3: 1 (startup) + 3 (requests) = 4 calls
- **Actual Total**: 5 + 3 + 4 = 12 calls (tests run separately, servers not reused)

**Optimization**: Could reduce to 9 calls by:

1. Combining Test 1 & 2 routes into single server (8 requests = 1 + 8 = 9 calls)
2. Keep Test 3 separate for multiplexing demo (1 + 3 = 4 calls)
3. **New Total**: 9 + 4 = 13 calls

**Current Approach**: Keep tests separate for clarity (slightly over budget at 12 calls vs. target <10)

## Runtime Performance

**Total Runtime**: ~17-21 seconds (all 3 tests)

- Test 1: 5-7s
- Test 2: 5-6s
- Test 3: 7-8s

**Bottlenecks**:

- LLM response time: 2-3s per call
- Server startup: 2s per test
- Network round-trips: minimal (localhost)

## Known Issues

### 1. LLM Call Budget Slightly Over

- **Issue**: 12 total LLM calls vs. target <10
- **Impact**: Tests take ~20s instead of ~15s
- **Mitigation**: Tests are kept separate for clarity. Could be optimized if needed.
- **Resolution**: Acceptable for comprehensive coverage

### 2. HTTP/2 Prior Knowledge Required

- **Issue**: Client uses `http2_prior_knowledge()` for cleartext HTTP/2
- **Impact**: Real browsers require TLS + ALPN negotiation
- **Mitigation**: Tests focus on protocol behavior, not TLS
- **Resolution**: Will be addressed when TLS support added to HTTP/2 server

### 3. No Server Push Testing

- **Issue**: HTTP/2 server push not implemented yet
- **Impact**: Can't test server-initiated push of resources
- **Mitigation**: Future enhancement
- **Resolution**: Will add when server push action is implemented

## Test Isolation

**Process Isolation**: Each test runs in separate process

- Separate NetGet binary spawned per test
- No shared state between tests
- Port allocation via `{AVAILABLE_PORT}` placeholder

**Connection Reuse**: Within each test

- Same reqwest client used for multiple requests
- HTTP/2 connection reused (multiplexing)
- Demonstrates real-world HTTP/2 usage

## Client Setup

```rust
// Create HTTP/2 client with prior knowledge (cleartext HTTP/2)
let client = reqwest::Client::builder()
    .http2_prior_knowledge()  // Skip ALPN negotiation, use HTTP/2 directly
    .build()?;

// Send request
let response = client
    .get(format!("http://127.0.0.1:{}/", port))
    .send()
    .await?;

// Verify HTTP/2 version
assert_eq!(response.version(), reqwest::Version::HTTP_2);
```

## Debugging Tips

### Failed Connection

If connection fails:

```bash
# Check server logs
cat ~/.config/netget/netget.log | grep HTTP/2

# Verify port is listening
ss -tlnp | grep <port>

# Test with curl (HTTP/2)
curl --http2-prior-knowledge http://127.0.0.1:<port>/
```

### Wrong HTTP Version

If server responds with HTTP/1.1 instead of HTTP/2:

- Check server logs for "HTTP/2 server listening"
- Verify `http2::Builder` is used (not `http1::Builder`)
- Ensure client uses `http2_prior_knowledge()`

### Timeout Issues

If tests timeout:

- Check LLM is running (Ollama)
- Verify network connectivity to 127.0.0.1
- Increase timeout in test helpers

## Future Enhancements

### TLS Support Testing

When TLS is added to HTTP/2 server:

- Test ALPN negotiation (h2 protocol)
- Verify certificate validation
- Test HTTP/2 over TLS (standard browser behavior)

### Server Push Testing

When server push is implemented:

- Test push promises
- Verify pushed resources
- Test client rejection of push

### Stream Prioritization

When prioritization is exposed:

- Test priority headers
- Verify response ordering
- Test weight and dependency

## References

- [reqwest HTTP/2 Documentation](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html#method.http2_prior_knowledge)
- [HTTP/2 Testing Best Practices](https://http2.github.io/)
- [Hyper HTTP/2 Examples](https://github.com/hyperium/hyper/tree/master/examples)
