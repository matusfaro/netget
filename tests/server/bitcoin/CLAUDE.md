# Bitcoin P2P Protocol E2E Tests

## Overview

End-to-end tests for the Bitcoin P2P protocol server implementation. These tests validate that the LLM-controlled server correctly handles Bitcoin P2P protocol operations including version/verack handshake, ping/pong keepalive, and basic message exchanges.

## Test Strategy

**Black-box testing**: Tests spawn the NetGet binary and interact with it as real Bitcoin P2P clients would, using the `bitcoin` Rust crate to construct and parse P2P messages.

**Minimal LLM calls**: Tests are designed to minimize LLM calls while covering the essential Bitcoin P2P protocol features:
- Version/verack handshake (2-3 LLM calls)
- Ping/pong exchange (1-2 LLM calls after handshake)
- Getaddr request (1 LLM call after handshake)
- Testnet network support (2-3 LLM calls)

**Total LLM call budget**: ~8-10 calls across all test cases

## Test Cases

### 1. Version/Verack Handshake (`test_bitcoin_version_verack_handshake`)

**Purpose**: Validate the Bitcoin P2P handshake protocol

**Flow**:
1. Client sends `version` message to server
2. Server responds with its own `version` message
3. Server sends `verack` to acknowledge client's version
4. Client sends `verack` to complete handshake

**Validations**:
- Server sends valid `version` message with protocol 70015
- Server sends `verack` after version
- Messages use correct Bitcoin mainnet magic bytes

**LLM calls**: 2-3 (connection opened, version received, potentially verack received)

**Runtime**: ~2-5 seconds (depends on LLM response time)

### 2. Ping/Pong Exchange (`test_bitcoin_ping_pong`)

**Purpose**: Test Bitcoin keepalive mechanism

**Flow**:
1. Complete handshake (version/verack)
2. Client sends `ping` with random nonce
3. Server responds with `pong` containing same nonce

**Validations**:
- Server responds to ping with pong
- Pong nonce matches ping nonce
- Messages properly formatted

**LLM calls**: 3-4 (handshake + ping received)

**Runtime**: ~3-6 seconds

### 3. Getaddr Request (`test_bitcoin_getaddr`)

**Purpose**: Test peer address discovery protocol

**Flow**:
1. Complete handshake
2. Client sends `getaddr` (request peer addresses)
3. Server responds with `addr` message (can be empty)

**Validations**:
- Server accepts getaddr message
- Server responds (or timeout is acceptable for no peers)
- Response format is valid

**LLM calls**: 3-4 (handshake + getaddr received)

**Runtime**: ~3-6 seconds

### 4. Testnet Network Support (`test_bitcoin_testnet`)

**Purpose**: Validate testnet magic bytes and network separation

**Flow**:
1. Client connects with testnet magic bytes
2. Client sends testnet `version` message
3. Server responds with testnet messages

**Validations**:
- Server uses testnet magic bytes (0x0B110907) in responses
- Server accepts testnet version messages
- Protocol works identically to mainnet but with different magic

**LLM calls**: 2-3 (testnet version handling)

**Runtime**: ~2-5 seconds

## Test Execution

### Running Tests

```bash
# Single protocol test (RECOMMENDED - fast)
./cargo-isolated.sh test --no-default-features --features bitcoin --test bitcoin::e2e_test

# With Ollama lock (when running multiple tests concurrently)
OLLAMA_LOCK=1 ./cargo-isolated.sh test --no-default-features --features bitcoin --test bitcoin::e2e_test
```

### Prerequisites

1. **Release binary**: Build first to avoid slow debug builds during tests
   ```bash
   ./cargo-isolated.sh build --release --all-features
   ```

2. **Ollama running**: Ensure Ollama is running locally (default: http://localhost:11434)

3. **Model available**: Ensure the default model (qwen3-coder:30b) is pulled:
   ```bash
   ollama pull qwen3-coder:30b
   ```

## Client Library

**Test client**: Uses `bitcoin` Rust crate (v0.32) for message construction and parsing

**Message building**:
- `RawNetworkMessage::new(magic, payload)` - Construct messages
- `consensus_encode()` - Serialize to wire format

**Message parsing**:
- `RawNetworkMessage::consensus_decode()` - Deserialize from wire

**Why this library**: Industry-standard, comprehensive Bitcoin protocol support, used by real Bitcoin software

## Known Issues & Limitations

### LLM Behavior

1. **Non-deterministic responses**: LLM may respond differently to same input
   - Tests use flexible assertions (e.g., accept timeout for getaddr)
   - Tests verify protocol correctness, not specific LLM decisions

2. **Handshake order**: LLM may send version before or after receiving peer's version
   - Tests handle both scenarios
   - Both approaches are valid per Bitcoin P2P protocol

3. **Optional responses**: Some messages may not get responses
   - E.g., getaddr when server has no peers to share
   - Tests accept both response and timeout

### Protocol Coverage

**Tested**:
- Version/verack handshake
- Ping/pong keepalive
- Basic message types (getaddr)
- Multiple networks (mainnet, testnet)

**Not tested** (out of scope for E2E):
- Block/transaction data exchange
- Inventory (inv/getdata) protocol
- Bloom filters
- Compact block relay
- BIP324 encrypted P2P

**Why limited coverage**: This is an LLM-controlled honeypot, not a real full node. E2E tests focus on protocol handshake and basic message handling that demonstrates LLM control.

## Test Infrastructure

### Helpers Used

- `start_netget_server()` - Spawns NetGet binary with prompt
- `ServerConfig::new()` - Configures server with custom instruction
- `read_bitcoin_message()` - Helper to read and parse Bitcoin messages
- `build_version_message()` - Constructs valid version messages

### Timeouts

- Connection timeout: 5 seconds
- Message read timeout: 120 seconds (allows LLM processing time)
- Server startup delay: 2 seconds

**Why 120s read timeout**: LLM inference can take 10-60+ seconds depending on model size and load. Tests use generous timeouts to avoid flaky failures.

## Efficiency Notes

### LLM Call Minimization

1. **Single comprehensive prompt**: Each test uses one instruction covering full scenario
2. **No scripting mode**: Bitcoin P2P requires stateful protocol handling (handshake, nonces)
3. **Reuse connections**: Tests establish handshake once, then test additional features

### Total Runtime

- **Best case**: 10-20 seconds (fast LLM responses)
- **Typical**: 30-60 seconds (normal LLM latency)
- **Worst case**: 2-3 minutes (slow LLM or retries)

## Privacy & Security

- **Localhost only**: All tests use 127.0.0.1 (no external connections)
- **Offline capable**: Tests work without internet (only local Ollama required)
- **No real blockchain**: No actual Bitcoin blockchain data or transactions
- **Testnet safe**: Testnet tests use fake data, not real testnet network

## Example Prompts

### Minimal handshake
```
listen on port 0 via bitcoin. Handle version/verack handshake.
```

### Ping/pong
```
listen on port 0 via bitcoin. Complete handshake, respond to ping with pong using same nonce.
```

### Multi-feature
```
listen on port 0 via bitcoin with network=mainnet.
Complete version/verack handshake.
Respond to ping with pong.
Respond to getaddr with empty addr list.
```

## Debugging

### Common failures

1. **Timeout reading message**: LLM took too long or didn't respond
   - Check Ollama logs
   - Verify model is loaded
   - Check netget.log for errors

2. **Parse error**: Invalid Bitcoin message format
   - Check netget.log TRACE level for hex dumps
   - Verify LLM is constructing valid messages
   - May indicate LLM hallucination or prompt issue

3. **Wrong message type**: LLM sent unexpected message
   - Review LLM instruction in test
   - Check if prompt is clear enough
   - May be acceptable (test assertions should be flexible)

### Log analysis

```bash
# View full message hex dumps
grep "TRACE.*Bitcoin P2P" netget.log

# Check LLM responses
grep "LLM.*Bitcoin" netget.log

# See parsed message types
grep "Parsed Bitcoin message" netget.log
```
