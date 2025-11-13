# ZooKeeper Server E2E Testing

## Test Strategy

Black-box testing using manually constructed ZooKeeper binary protocol messages to verify LLM-controlled responses.

## Tests Implemented

### 1. test_zookeeper_get_data
**Purpose**: Verify getData operation returns correct data
**LLM Calls**: 2 (server startup + getData request)
**Flow**:
1. Start ZooKeeper server with mock
2. Connect via TCP
3. Send binary getData request for /config/database
4. Verify response: xid=1, zxid=100, error_code=0

**Mock Configuration**:
- Event: `zookeeper_request` with operation="getData", path="/config/database"
- Action: `zookeeper_response` with xid=1, zxid=100, error_code=0, data_hex=<postgres url>

### 2. test_zookeeper_get_children
**Purpose**: Verify getChildren operation returns child list
**LLM Calls**: 2 (server startup + getChildren request)
**Flow**:
1. Start ZooKeeper server with mock
2. Connect via TCP
3. Send binary getChildren request for /services
4. Verify response: xid=2, zxid=200, error_code=0

**Mock Configuration**:
- Event: `zookeeper_request` with operation="getChildren", path="/services"
- Action: `zookeeper_response` with xid=2, zxid=200, error_code=0, data_hex=<array of children>

### 3. test_zookeeper_error_response
**Purpose**: Verify error responses for nonexistent nodes
**LLM Calls**: 2 (server startup + getData request)
**Flow**:
1. Start ZooKeeper server with mock
2. Connect via TCP
3. Send binary getData request for /nonexistent
4. Verify response: xid=3, zxid=300, error_code=-101 (NONODE)

**Mock Configuration**:
- Event: `zookeeper_request` with operation="getData", path="/nonexistent"
- Action: `zookeeper_response` with xid=3, zxid=300, error_code=-101

## LLM Call Budget

**Total LLM Calls**: 6 (3 tests × 2 calls each)
**Budget Compliance**: ✓ Under 10 calls

## Test Infrastructure

### Binary Protocol Construction

Tests manually build ZooKeeper binary protocol messages:

**Request Format**:
```
[4 bytes length][4 bytes xid][4 bytes op_type][variable data]
```

**Response Format**:
```
[4 bytes length][4 bytes xid][8 bytes zxid][4 bytes error_code][variable data]
```

**Operation Types**:
- 4: getData
- 8: getChildren
- Others: create (1), delete (2), setData (5), etc.

### Helper Functions

- `build_get_data_request(xid, path)` - Constructs getData request
- `build_get_children_request(xid, path)` - Constructs getChildren request
- `parse_response_header(data)` - Parses response header (length, xid, zxid, error_code)

## Mock Pattern

All tests follow this pattern:

```rust
let config = NetGetConfig::new(prompt)
    .with_mock(|mock| {
        mock
            // Mock server startup
            .on_instruction_containing("ZooKeeper")
            .respond_with_actions(json!([{
                "type": "open_server",
                "port": 0,
                "base_stack": "ZooKeeper",
                "instruction": "..."
            }]))
            .expect_calls(1)
            .and()
            // Mock ZooKeeper request
            .on_event("zookeeper_request")
            .and_event_data_contains("operation", "getData")
            .and_event_data_contains("path", "/config/database")
            .respond_with_actions(json!([{
                "type": "zookeeper_response",
                "xid": 1,
                "zxid": 100,
                "error_code": 0,
                "data_hex": "..."
            }]))
            .expect_calls(1)
            .and()
    });
```

## Expected Runtime

- **Mock mode** (default): < 2 seconds per test
- **Real Ollama mode**: 5-10 seconds per test

## Running Tests

### Mock Mode (Default, No Ollama Required)
```bash
# Run all ZooKeeper tests
./test-e2e.sh zookeeper

# With cargo directly
cargo test --features zookeeper --test server::zookeeper::e2e_test
```

### Real Ollama Mode (Optional Validation)
```bash
# Requires Ollama running with qwen3-coder:30b
./test-e2e.sh --use-ollama zookeeper

# With cargo
cargo test --features zookeeper -- --use-ollama
```

### Build First (Recommended)
```bash
./cargo-isolated.sh build --release --features zookeeper
```

## Known Limitations

### 1. Binary Protocol Complexity
**Issue**: ZooKeeper uses Jute serialization which is complex
**Impact**: Tests use simplified binary format for basic operations only
**Workaround**: Tests cover common operations (getData, getChildren, errors)

### 2. No Real Client Library
**Note**: Tests don't use zookeeper-async or similar client library
**Reason**: Simplifies testing - direct binary protocol control
**Impact**: Tests verify protocol basics but not complex client interactions

### 3. Simplified Data Encoding
**Issue**: data_hex field bypasses Jute serialization
**Impact**: LLM provides hex-encoded data directly
**Workaround**: Works well for simple string data, sufficient for testing

## Test Reliability

**Timeouts**:
- Server initialization: 500ms
- Response wait: 5 seconds
- Total per test: ~2 seconds (mock mode), ~10 seconds (real Ollama)

**Failure Modes**:
- LLM doesn't understand ZooKeeper protocol → Bad response format
- LLM returns wrong error codes → Assertion failures
- Binary protocol parsing issues → Response header parse fails

**Retry Logic**: None. Tests fail fast on errors.

## Privacy & Offline

All tests:
- ✓ Use localhost only (127.0.0.1)
- ✓ No external connections
- ✓ Work completely offline
- ✓ No network access required (mock mode)

## Debugging

### Enable Trace Logging
```bash
RUST_LOG=trace ./cargo-isolated.sh test --features zookeeper -- --nocapture
```

### Inspect Binary Packets
```rust
println!("Request hex: {}", hex::encode(&request));
println!("Response hex: {}", hex::encode(&buffer[..n]));
```

### Common Issues

**No response**:
- LLM didn't understand request format
- Check server logs for parsing errors

**Wrong response format**:
- Response header size < 20 bytes
- Binary protocol mismatch

**Timeout**:
- LLM took too long to respond
- Increase timeout in test

## Future Enhancements

1. **Create Operation**: Test znode creation
2. **Delete Operation**: Test znode deletion
3. **SetData Operation**: Test data updates
4. **Watch Mechanism**: Test watches (when implemented)
5. **Session Management**: Test connection lifecycle
6. **ACL Operations**: Test access control (when implemented)

## References

- ZooKeeper Protocol: https://zookeeper.apache.org/doc/current/zookeeperProgrammers.html
- Jute Serialization: https://zookeeper.apache.org/doc/current/jute.html
- Implementation: `src/server/zookeeper/CLAUDE.md`
