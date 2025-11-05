# HTTP/3 Protocol E2E Tests

## Test Overview
Tests the HTTP/3 server implementation with stream multiplexing and encrypted transport. Validates that the LLM can handle bidirectional streams over HTTP/3 connections with built-in TLS 1.3 encryption.

## Test Strategy
- **Isolated test servers**: Each test spawns a separate NetGet instance with specific instructions
- **Quinn client**: Uses `quinn::Endpoint` for HTTP/3 client connections
- **Self-signed certificates**: Server uses generated self-signed certs, client skips verification
- **Stream-focused**: Tests individual stream operations and multiplexing
- **Fast validation**: 10-second timeout per operation, 15 seconds for concurrent streams

## LLM Call Budget
- `test_http3_echo()`: 2 LLM calls (connection opened + stream data)
- `test_http3_custom_response()`: 2 LLM calls (connection opened + PING command)
- `test_http3_multiple_streams()`: 4 LLM calls (connection opened + 3 concurrent streams)
- **Total: 8 LLM calls** (well under 10 limit)

**Optimization**: Each test starts a new server to ensure clean state. Could be optimized by using a single comprehensive server, but current approach provides better isolation.

## Scripting Usage
❌ **Scripting Disabled** - Action-based responses only

**Rationale**: HTTP/3 tests use simple echo and command-response patterns that work well with action-based LLM responses. The protocol is more complex than TCP but tests remain straightforward.

## Client Library
- **quinn v0.11** - Pure Rust async HTTP/3 client
- **rustls** - TLS 1.3 for encryption (required by HTTP/3)
- **webpki-roots** - Root certificates for TLS (though we skip verification for self-signed certs)

**Why quinn?**:
1. Same library used by server (ensures compatibility)
2. Pure Rust, no external dependencies
3. Full async support with tokio
4. Built-in TLS 1.3 (mandatory for HTTP/3)
5. Stream multiplexing matches server implementation

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~80-90 seconds for full test suite (3 tests × ~25-30s each)
- Each test includes: server startup (2-3s) + TLS handshake (1-2s) + LLM responses (2-4 calls × 5-8s) + validation (<1s)
- Multiple streams test takes longer due to concurrent LLM calls

## Failure Rate
- **Medium** (~10-15%) - Occasional TLS handshake timeouts or LLM response format issues
- Most failures: LLM doesn't echo exactly or takes too long to respond
- Timeout failures: ~5% - usually TLS handshake delays or Ollama overload
- Certificate errors: Should be rare with proper `SkipServerVerification` implementation

## Test Cases

### 1. HTTP/3 Echo (`test_http3_echo`)
- **Prompt**: Echo server on HTTP/3
- **Client**: Opens bidirectional stream, sends "Hello, HTTP/3!"
- **Expected**: Exact echo of sent data
- **Purpose**: Tests basic HTTP/3 stream communication and LLM data handling

**LLM Calls**: 2 (connection opened, data received)

### 2. HTTP/3 Custom Response (`test_http3_custom_response`)
- **Prompt**: Respond to PING with PONG
- **Client**: Opens stream, sends "PING"
- **Expected**: Response contains "PONG"
- **Purpose**: Tests LLM's ability to parse commands and generate custom responses

**LLM Calls**: 2 (connection opened, PING command)

### 3. HTTP/3 Multiple Streams (`test_http3_multiple_streams`)
- **Prompt**: Echo server handling multiple concurrent streams
- **Client**: Opens 3 bidirectional streams concurrently, sends different data on each
- **Expected**: Each stream receives echo of its own data independently
- **Purpose**: Tests HTTP/3 stream multiplexing and concurrent LLM handling

**LLM Calls**: 4 (1 connection opened + 3 concurrent stream data)

## Known Issues

### 1. TLS Handshake Timeout
HTTP/3 requires TLS 1.3 handshake before any data transfer. Under heavy load or slow systems, the handshake may timeout.

**Mitigation**: 10-second timeout provides buffer, but may need increase for slow environments.

### 2. Certificate Verification Skip
Tests use `SkipServerVerification` to accept self-signed certificates. This is test-only code - production clients should use proper certificate validation.

**Implementation**: Custom `ServerCertVerifier` that always returns `Ok(...)`.

### 3. ALPN Protocol Matching
Server uses custom ALPN: `h3`. Client must specify the same ALPN or connection will fail.

**Verification**: Check client config includes `client_crypto.alpn_protocols = vec![b"h3".to_vec()]`

### 4. Stream Finish Required
HTTP/3 streams must call `send.finish()` to signal no more data. Without this, the receiver will wait indefinitely.

**Pattern**: Always call `send.finish()` after `send.write_all()`.

### 5. LLM Response Timing for Concurrent Streams
When testing multiple streams, LLM may process them sequentially due to Ollama lock. This doesn't affect correctness but impacts timing.

**Expected**: 15-second timeout accommodates sequential processing of 3 streams.

## Performance Notes

### TLS Overhead
HTTP/3 mandates TLS 1.3 encryption, adding ~1 RTT to connection establishment and CPU overhead for encryption/decryption.

**Impact**: Slightly slower than raw TCP tests, but provides security by default.

### Stream Multiplexing Benefit
Multiple streams on one connection avoid repeated TLS handshakes. Test 3 demonstrates this - single connection, multiple streams.

**Advantage**: More efficient than opening multiple TCP connections.

### UDP-Based
HTTP/3 is UDP-based, which may be affected by UDP buffer sizes and packet loss. Tests run on localhost with no packet loss.

**Production Note**: Real-world HTTP/3 may need congestion control tuning.

## Future Enhancements

### Test Coverage Gaps
1. **Unidirectional streams**: Only bidirectional streams tested
2. **Connection migration**: No tests for HTTP/3's connection migration feature
3. **0-RTT**: No tests for early data (0-RTT resumption)
4. **Flow control**: No tests for stream flow control limits
5. **Datagram frames**: No tests for unreliable DATAGRAM support
6. **Large data transfers**: No tests for multi-MB payloads
7. **Stream closure**: No explicit tests for half-close semantics

### Consolidation Opportunity
All three tests could use a single comprehensive server:

```rust
let prompt = format!(
    "listen on port {} via http3.
    - Echo back all data received on streams
    - When you receive 'PING': respond 'PONG'
    - Handle multiple concurrent streams independently",
    port
);
```

This would reduce from 3 server spawns to 1, saving ~6-9 seconds of test time.

### Additional Test Ideas
- **Stream priority**: Test if HTTP/3 stream priorities work (requires quinn support)
- **Connection timeout**: Test idle connection timeout
- **Large stream count**: Test 50+ concurrent streams
- **Binary data**: Test hex-encoded binary protocol
- **Wait for more**: Test `wait_for_more` action on incomplete data

## References
- [RFC 9000: HTTP/3 Transport](https://datatracker.ietf.org/doc/html/rfc9000)
- [RFC 9001: HTTP/3 TLS](https://datatracker.ietf.org/doc/html/rfc9001)
- [Quinn Documentation](https://docs.rs/quinn/)
- [HTTP/3 Working Group](https://http3wg.org/)

## Debug Tips

### Connection Failures
If tests fail with "Connection timeout":
1. Check server started successfully (look for "HTTP/3 server listening" in logs)
2. Verify port is available (netstat/lsof)
3. Check firewall rules (unlikely on localhost)
4. Increase timeout duration

### TLS Errors
If tests fail with TLS errors:
1. Verify ALPN protocol matches (`h3`)
2. Check `SkipServerVerification` is used
3. Ensure rustls versions match between server and client

### Stream Errors
If stream operations fail:
1. Verify `send.finish()` is called
2. Check for early stream close by server
3. Look for LLM errors in server logs
4. Verify LLM is generating `send_http3_data` actions

### LLM Response Issues
If LLM doesn't respond correctly:
1. Check prompt clarity in test
2. Review server logs for LLM errors
3. Verify action parsing (should see `send_http3_data` actions)
4. Check Ollama is running and responsive
