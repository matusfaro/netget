# XMPP E2E Testing

## Testing Strategy

XMPP E2E tests validate core protocol functionality using manual TCP clients that send XML streams and stanzas.

### Test Coverage

1. **Stream Initialization** (`test_xmpp_stream_header`)
    - Tests XML stream header exchange
    - Validates server responds with proper stream:stream element
    - LLM calls: 1 (stream header response)

2. **Message Exchange** (`test_xmpp_message`)
    - Tests basic message stanza handling
    - Validates message echo functionality
    - LLM calls: 2 (stream header + message echo)

3. **Presence Handling** (`test_xmpp_presence`)
    - Tests presence stanza processing
    - Validates presence acknowledgment
    - LLM calls: 2 (stream header + presence response)

### LLM Call Budget

**Target**: < 10 LLM calls per test suite
**Actual**: ~5 LLM calls total (1-2 per test)

Tests are designed to minimize LLM calls:

- Simple, focused prompts
- Single server instance per test
- Limited interaction sequences
- No complex multi-stanza exchanges

### Test Runtime

**Typical Runtime**: 30-60 seconds per test

- 10s LLM response timeout per interaction
- 3 tests × 2 interactions × 10s = ~60s max
- Actual runtime usually faster (LLM responds in 2-5s)

### Test Client

Tests use manual TCP client with AsyncReadExt/AsyncWriteExt:

- Send raw XML strings
- Read XML responses into buffers
- Simple pattern matching for validation
- No XMPP library required

### Known Issues

1. **Stream Parsing**: LLM receives entire buffer, not parsed stanzas
    - May struggle with complex XML or partial stanzas
    - Works fine for simple, complete stanzas

2. **Timeout Sensitivity**: 10-second timeout per LLM call
    - May need adjustment for slower models
    - Test uses best-effort validation (doesn't fail on timeout)

3. **XML Validation**: Tests use simple string matching
    - No strict XML schema validation
    - Accepts various valid XML formats

### Running Tests

```bash
# Build release binary first (required)
./cargo-isolated.sh build --release --features xmpp

# Run XMPP E2E tests
./cargo-isolated.sh test --no-default-features --features xmpp --test server::xmpp::test
```

**Important**: Always build release binary before running E2E tests. Tests spawn the actual NetGet binary.

### Test Prompts

#### Stream Header Test

```
listen on port {AVAILABLE_PORT} via xmpp. When clients connect and send an XML stream header,
respond with: <?xml version='1.0'?><stream:stream xmlns='jabber:client'
xmlns:stream='http://etherx.jabber.org/streams' from='localhost' id='stream-123' version='1.0'>
```

#### Message Echo Test

```
listen on port {AVAILABLE_PORT} via xmpp domain=localhost. When clients send an XML stream header,
respond with server stream header and features. When clients send a message stanza, extract the body
text and echo it back with: <message from='bot@localhost' to='[sender]' type='chat'><body>Echo: [body]</body></message>
```

#### Presence Test

```
listen on port {AVAILABLE_PORT} via xmpp. When clients connect, respond with stream header.
When clients send a presence stanza, acknowledge with: <presence from='server@localhost'
type='available'><status>Server online</status></presence>
```

### Privacy & Offline Mode

All tests:

- ✓ Use localhost only (127.0.0.1)
- ✓ No external connections
- ✓ Work offline
- ✓ No network access required (except to local Ollama)

### Future Enhancements

1. **Authentication Tests**: Test SASL PLAIN authentication flow
2. **IQ Tests**: Test info/query stanzas (roster, bind, etc.)
3. **Multi-Stanza**: Test multiple messages in single stream
4. **Error Handling**: Test malformed XML, invalid stanzas
5. **Stream Restart**: Test post-authentication stream restart

### Performance Optimization

Current tests are reasonably optimized:

- 3 independent tests (can run in parallel with `--test-threads`)
- Simple prompts (reduce LLM processing time)
- Short interaction sequences (minimize network round-trips)
- Pattern matching (avoid expensive XML parsing in tests)

No further optimization needed at this stage.
