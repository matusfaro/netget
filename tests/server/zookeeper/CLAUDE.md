# ZooKeeper Server E2E Testing

## Test Strategy

Black-box testing using the `zookeeper-async` Rust client to connect to the NetGet ZooKeeper server and verify LLM-controlled responses.

## LLM Call Budget

Target: **< 10 LLM calls**

- Test 1: Connect and getData (2 LLM calls: connection, getData)
- Test 2: Create and verify (2 LLM calls: create, getData)
- Test 3: getChildren (1 LLM call)

Total: ~5 LLM calls

## Test Infrastructure

### Client Library

**zookeeper-async v0.7**: Modern async ZooKeeper client
- Binary protocol support
- Session management
- Operation support: create, delete, getData, setData, getChildren

### Test Execution

```rust
use zookeeper_async::ZooKeeper;

#[tokio::test]
async fn test_zookeeper_server_basic() {
    // 1. Start ZooKeeper server with instruction
    // 2. Connect using zookeeper-async client
    // 3. Perform operations (getData, create, getChildren)
    // 4. Verify responses match LLM instruction
}
```

## Expected Runtime

- **Cold start**: 10-15 seconds (server spawn + LLM calls)
- **Warm start**: 5-8 seconds (cached LLM responses)

## Known Issues

1. **Binary Protocol Complexity**: ZooKeeper's Jute serialization requires careful parsing
2. **Session Management**: Session timeout handling may cause test flakiness
3. **Simplified Protocol**: Only basic operations supported (no watches, transactions, ACLs)

## Test Scenarios

### Scenario 1: Basic getData Operation

```
Instruction: "Act as a ZooKeeper server. When clients read /test, return 'hello world'."

Test Steps:
1. Connect to server
2. Call getData("/test")
3. Verify response is "hello world"
```

### Scenario 2: Create and Verify

```
Instruction: "Act as a ZooKeeper server. Allow creating znodes and return success."

Test Steps:
1. Connect to server
2. Create znode at /newnode with data "test data"
3. Call getData("/newnode") to verify
4. Verify response matches "test data"
```

### Scenario 3: getChildren Operation

```
Instruction: "Act as a ZooKeeper server. /services has three children: web, api, db."

Test Steps:
1. Connect to server
2. Call getChildren("/services")
3. Verify response contains ["web", "api", "db"]
```

## Debugging

If tests fail:

1. Check `netget.log` for ZooKeeper server logs
2. Verify binary protocol parsing (check hex dumps)
3. Ensure xid/zxid/error_code are correctly formatted
4. Check for session timeout issues
5. Verify length-prefixed message format

## Test Optimization

To minimize LLM calls:

1. **Single Connection**: Reuse connection across operations
2. **Instruction Clarity**: Provide complete instruction upfront to avoid follow-up queries
3. **No Watchers**: Avoid watch mechanism (not implemented)
4. **Simple Operations**: Stick to basic create/get/delete operations
