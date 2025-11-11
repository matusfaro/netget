# Bitcoin RPC Client E2E Tests

## Test Strategy

Black-box testing using NetGet binary with LLM instructions. Tests verify:

1. Client can connect to Bitcoin RPC endpoint (HTTP JSON-RPC)
2. Client can execute RPC commands
3. Client protocol is correctly identified

## Test Scenarios

### 1. Connection Test

**File:** `e2e_test.rs::test_bitcoin_client_connection`

**Setup:**

- HTTP server mock (simulates Bitcoin Core RPC)
- Server responds to POST with JSON-RPC format

**Test:**

- Client connects with RPC URL (http://user:pass@host:port)
- LLM initiates blockchain info query
- Verify client identifies as Bitcoin protocol

**LLM Calls:** 2 (server startup, client connect)

**Expected Runtime:** 1-2 seconds

### 2. RPC Command Test

**File:** `e2e_test.rs::test_bitcoin_client_rpc_command`

**Setup:**

- HTTP server mock logging POST requests

**Test:**

- Client connects to RPC endpoint
- Client executes getblockchaininfo command
- Verify protocol name is "Bitcoin"

**LLM Calls:** 2 (server startup, client connect + command)

**Expected Runtime:** 1-2 seconds

## LLM Call Budget

**Total LLM Calls:** 4 (2 tests × 2 calls each)

**Breakdown:**

- Server startup: 1 call per test
- Client connection + initial RPC: 1 call per test

**Why So Few:**

- Black-box testing (verify connectivity, not full RPC functionality)
- Mock server (no real Bitcoin Core node required)
- Focus on client initialization and protocol identification

## Test Limitations

### No Real Bitcoin Core Node

- Tests use HTTP server mock, not actual bitcoind
- Cannot verify full RPC protocol compliance
- Cannot test blockchain data parsing

### No Transaction Testing

- No wallet operations
- No transaction submission
- No mempool monitoring

### Minimal RPC Coverage

- Only tests connection and basic command execution
- Full RPC method coverage would require Bitcoin Core

## Running Tests

```bash
# Build with Bitcoin feature
./cargo-isolated.sh build --no-default-features --features bitcoin

# Run Bitcoin client E2E tests
./cargo-isolated.sh test --no-default-features --features bitcoin --test client::bitcoin::e2e_test
```

## Known Issues

None at this time.

## Future Test Enhancements

1. **Bitcoin Core Integration:**
    - Use Bitcoin Core in regtest mode
    - Test real blockchain queries
    - Verify transaction submission

2. **RPC Method Coverage:**
    - Test getblock, getrawtransaction
    - Test mempool queries
    - Test peer info queries

3. **Error Handling:**
    - Test invalid RPC URLs
    - Test authentication failures
    - Test malformed JSON-RPC responses

4. **LLM Follow-up Actions:**
    - Test multi-step queries (get block hash → get block)
    - Test mempool monitoring loop
    - Test transaction analysis workflow
