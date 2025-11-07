# HTTP/3 Client E2E Tests

## Overview

E2E tests for the HTTP/3 client implementation verify that the LLM-controlled HTTP/3 client can successfully make requests over QUIC transport to HTTP/3 servers.

## Test Strategy

### Approach: Black-Box Testing

Tests spawn actual NetGet processes as black boxes:
1. Start NetGet HTTP/3 server
2. Start NetGet HTTP/3 client with instruction
3. Verify client behavior via output
4. Verify server received request (via logs)

### Test Infrastructure

- **Server**: NetGet HTTP/3 server (built-in)
- **Client**: NetGet HTTP/3 client
- **Transport**: QUIC over UDP (localhost)
- **Verification**: Process output inspection

## Test Cases

### 1. `test_http3_client_get_request`

**Purpose**: Verify basic GET request over QUIC

**Flow**:
1. Start HTTP/3 server on available port
2. Start HTTP/3 client with GET instruction
3. Verify client output shows HTTP/3/QUIC connection
4. Clean up both processes

**LLM Calls**: 2 (server startup, client connection)

**Expected Runtime**: ~3-4 seconds
- Server startup: 1s
- Client connection + QUIC handshake: 2s
- Verification: <1s

**Assertions**:
- Client output contains "HTTP/3", "HTTP3", "QUIC", or "connected"

### 2. `test_http3_client_with_priority`

**Purpose**: Verify stream priority control

**Flow**:
1. Start HTTP/3 server configured to log stream priorities
2. Start client with high-priority request instruction
3. Verify client protocol is HTTP3
4. Clean up

**LLM Calls**: 2 (server startup, client connection)

**Expected Runtime**: ~3-4 seconds

**Assertions**:
- Client protocol is "HTTP3"

**Note**: Server-side priority verification not yet implemented (would require log parsing)

### 3. `test_http3_client_llm_controlled`

**Purpose**: Verify LLM can control request details (method, headers, body)

**Flow**:
1. Start HTTP/3 server that echoes POST bodies
2. Start client with POST + JSON body instruction
3. Verify HTTP/3/QUIC transport used
4. Clean up

**LLM Calls**: 2 (server startup, client connection)

**Expected Runtime**: ~3-4 seconds

**Assertions**:
- Client output shows HTTP3 or QUIC usage

## LLM Call Budget

**Total**: 6 LLM calls across 3 tests

| Test | Server Startup | Client Action | Total |
|------|----------------|---------------|-------|
| test_http3_client_get_request | 1 | 1 | 2 |
| test_http3_client_with_priority | 1 | 1 | 2 |
| test_http3_client_llm_controlled | 1 | 1 | 2 |

**Justification**: Minimal LLM usage while covering key scenarios:
- Basic GET (connectivity)
- Stream priorities (QUIC feature)
- LLM control (POST with body)

## Test Execution

### Running Tests

```bash
# All HTTP/3 client tests
./cargo-isolated.sh test --no-default-features --features http3 --test client::http3::e2e_test

# Specific test
./cargo-isolated.sh test --no-default-features --features http3 test_http3_client_get_request
```

### Prerequisites

- **HTTP/3 server support**: NetGet must be compiled with `http3` feature
- **QUIC transport**: UDP connectivity on localhost
- **No firewall blocking**: UDP port must be accessible

## Known Issues

### 1. QUIC Connection Timeout

**Issue**: QUIC handshake may timeout on slow systems

**Workaround**: Increased sleep durations (2s instead of 500ms)

**Future**: Implement retry logic or connection timeout detection

### 2. Stream ID Not Verified

**Issue**: Tests don't verify actual stream IDs

**Reason**: Stream IDs not exposed by h3/quinn easily

**Future**: Add stream ID tracking in implementation

### 3. TLS Certificate Verification Disabled

**Issue**: Tests use self-signed certs with verification disabled

**Impact**: Doesn't test real-world TLS scenarios

**Future**: Generate valid test certificates or use CA-signed test certs

### 4. No External Server Tests

**Issue**: All tests use NetGet's own HTTP/3 server

**Limitation**: Doesn't verify interoperability with other implementations

**Future**: Add optional tests against public HTTP/3 servers (Cloudflare, Google)

## Performance Considerations

### Test Speed

- **Slower than HTTP/1.1**: QUIC handshake takes longer than TCP
- **Per-request connections**: Each test creates new QUIC connection
- **TUI startup overhead**: Each NetGet instance has initialization cost

### Optimization Opportunities

1. **Connection reuse**: Keep QUIC connection alive between tests
2. **Parallel execution**: Run independent tests concurrently
3. **Mock QUIC**: Use mock QUIC transport for unit tests

## Comparison with HTTP/1.1 Tests

| Aspect | HTTP/1.1 Tests | HTTP/3 Tests |
|--------|---------------|--------------|
| **Transport** | TCP (localhost) | QUIC/UDP (localhost) |
| **Handshake** | Fast (~10ms) | Slower (~100ms) |
| **Server** | HTTP server | HTTP/3 server |
| **Features Tested** | Basic requests | Priorities, multiplexing |
| **Runtime** | ~1-2s per test | ~3-4s per test |
| **Complexity** | Low | Medium |

## Future Enhancements

### High Priority

1. **Stream ID Verification**
   - Verify server assigns correct stream IDs
   - Test multiplexing (concurrent streams)

2. **0-RTT Testing**
   - Test connection resumption
   - Verify session ticket reuse

3. **Connection Migration**
   - Simulate IP address change
   - Verify QUIC handles migration

### Medium Priority

4. **External Server Tests**
   - Test against Cloudflare QUIC
   - Verify interoperability

5. **Error Scenarios**
   - Server unavailable
   - QUIC connection refused
   - TLS handshake failure

6. **Performance Tests**
   - Measure latency vs HTTP/1.1
   - Test multiplexing throughput

### Low Priority

7. **Advanced QUIC Features**
   - Connection migration
   - Flow control
   - Congestion control

8. **WebTransport Tests**
   - If/when WebTransport support added

## Debugging Tips

### Enable Verbose Logging

Set `RUST_LOG=debug` to see QUIC/HTTP/3 internals:

```bash
RUST_LOG=debug ./cargo-isolated.sh test --features http3 test_http3_client_get_request
```

### Check QUIC Connection

Verify QUIC handshake in logs:
- Look for "QUIC connection established"
- Check for TLS handshake completion
- Verify HTTP/3 session creation

### Inspect Network Traffic

Use Wireshark to inspect QUIC packets:
```bash
# Capture UDP traffic on loopback
sudo tcpdump -i lo -w http3-test.pcap udp
```

### Common Failure Modes

1. **"Connection refused"**: HTTP/3 server not started or wrong port
2. **"TLS handshake failed"**: Certificate issues (usually skip verification)
3. **"Timeout"**: QUIC handshake took too long (increase sleep)
4. **"Protocol not supported"**: HTTP/3 feature not compiled in

## References

- **NetGet Testing Guide**: `/home/user/netget/tests/README.md`
- **QUIC Debugging**: https://github.com/quinn-rs/quinn/blob/main/docs/debugging.md
- **HTTP/3 RFC**: RFC 9114
- **Test Helpers**: `/home/user/netget/tests/helpers/client.rs`
