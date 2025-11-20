# Tor Client Testing Strategy

## Test Approach

Black-box testing using real Tor network and public test destinations. The LLM controls the client based on prompts,
tests validate behavior with actual Tor connections.

## LLM Call Budget

**Target: < 15 LLM calls per test suite**

### Breakdown

1. **Directory Query Tests** (3 calls): Test consensus info, list relays, search relays
2. **Bootstrap Test** (1 call): Verify Tor client bootstraps successfully
3. **Basic HTTP Connection** (2 calls): Connect to public site and fetch page
4. **Onion Service Connection** (2 calls): Connect to `.onion` address
5. **Data Send/Receive** (2 calls): Test bidirectional communication
6. **Error Handling** (1 call): Test connection to invalid destination

**Total: ~11 LLM calls**

## Test Runtime

- **Bootstrap**: 10-30 seconds (first run, cached afterward)
- **Connection**: 2-10 seconds per connection
- **Data transfer**: 1-5 seconds
- **Total suite**: ~60-120 seconds

## Test Strategy

### Unit Tests

Not applicable - testing requires real Tor network interaction.

### E2E Tests

All tests are E2E, requiring:

- Tor network access (internet connection)
- Ollama with LLM model
- Arti bootstrap (downloads consensus on first run)

### Test Isolation

- Each test uses `--ollama-lock` to serialize LLM calls
- Arti caches consensus between tests (faster subsequent runs)
- Tests use different instructions to verify independent behavior

## Test Scenarios

### Directory Query Tests (NEW)

Testing directory query capabilities with mocks (no LLM required for most tests):

```rust
#[tokio::test]
async fn test_get_consensus_info() {
    // Bootstrap Tor client
    // Call get_consensus_info action
    // Verify relay_count, valid_after fields
}
```

**LLM Calls**: 1 (bootstrap event only)
**Expected**: Consensus metadata returned
**Validation**: JSON contains relay_count > 0, valid timestamps

```rust
#[tokio::test]
async fn test_list_relays() {
    // Bootstrap Tor client
    // Call list_relays with limit=10
    // Verify relay list returned
}
```

**LLM Calls**: 1 (bootstrap event only)
**Expected**: Array of 10 RelayInfo structs
**Validation**: Each relay has nickname, fingerprint, flags

```rust
#[tokio::test]
async fn test_search_relays_by_flags() {
    // Bootstrap Tor client
    // Call search_relays with flags=["Exit", "Fast"]
    // Verify all relays have required flags
}
```

**LLM Calls**: 1 (bootstrap event only)
**Expected**: Filtered relay list
**Validation**: All relays have Exit AND Fast flags

**Directory Tests Total**: 3 LLM calls (bootstrap only, queries are direct action invocations)

### 1. Bootstrap and Basic Connection

```rust
#[tokio::test]
async fn test_tor_basic_connection() {
    // Connect to check.torproject.org via Tor
    // Verify response contains "Congratulations. This browser is configured to use Tor."
}
```

**LLM Calls**: 2 (connect event, response event)
**Expected**: Successful connection through Tor
**Validation**: Response body contains Tor confirmation

### 2. Onion Service Connection

```rust
#[tokio::test]
async fn test_tor_onion_service() {
    // Connect to DuckDuckGo onion address
    // Verify connection and HTTP response
}
```

**LLM Calls**: 2 (connect event, response event)
**Expected**: Successful connection to `.onion` address
**Validation**: HTTP 200 response from onion service

### 3. HTTP GET Request

```rust
#[tokio::test]
async fn test_tor_http_request() {
    // Connect to httpbin.org via Tor
    // Send GET request for /anything
    // Parse and validate JSON response
}
```

**LLM Calls**: 3 (connect, send, receive)
**Expected**: LLM sends proper HTTP GET, receives JSON
**Validation**: Response JSON contains request data

### 4. Connection Error Handling

```rust
#[tokio::test]
async fn test_tor_connection_error() {
    // Try to connect to non-existent onion address
    // Verify graceful error handling
}
```

**LLM Calls**: 1 (connection failure)
**Expected**: Connection fails with timeout/error
**Validation**: Client status shows error

## Known Issues

### Flakiness Concerns

1. **Network Dependency**: Tests require internet and Tor network
    - Can fail if Tor network unavailable
    - Can fail behind restrictive firewalls
    - Mitigation: Retry logic, reasonable timeouts

2. **Bootstrap Time**: First run downloads consensus
    - Adds 10-30 seconds to first test
    - Mitigation: Cache consensus, run bootstrap test first

3. **Onion Service Availability**: Onion services can be down
    - Test onion addresses may become unavailable
    - Mitigation: Use multiple fallback addresses

4. **Exit Node Variability**: Different exit nodes have different policies
    - Some sites may be blocked by certain exits
    - Mitigation: Use widely accessible test sites

5. **LLM Behavior**: LLM may format HTTP requests differently
    - May affect test reliability
    - Mitigation: Use permissive validation (check key fields only)

### Test Environment

**Minimum Requirements:**

- Internet connection
- No Tor network blocking (some countries/networks block Tor)
- Ollama with model loaded
- ~3MB disk space for consensus cache

**CI Considerations:**

- May not work in restricted CI environments (GitHub Actions may block Tor)
- Consider marking tests as `#[ignore]` for CI, run manually
- Alternative: Mock arti-client for CI, only run real tests locally

## Running Tests

```bash
# First time (slow due to bootstrap)
./cargo-isolated.sh test --no-default-features --features tor-client --test client::tor::e2e_test

# Subsequent runs (fast, consensus cached)
./cargo-isolated.sh test --no-default-features --features tor-client --test client::tor::e2e_test

# With debug logging
RUST_LOG=debug ./cargo-isolated.sh test --no-default-features --features tor-client --test client::tor::e2e_test
```

## Privacy Notes

Tests connect to:

- Tor directory authorities (public, expected)
- Public test sites (httpbin.org, check.torproject.org)
- Public onion services (DuckDuckGo, etc.)

No personal data is sent. All connections are anonymous via Tor.

## Future Test Enhancements

1. **Mock Arti Client**: Allow CI testing without real Tor
2. **Bootstrap Progress**: Test bootstrap status events
3. **Circuit Isolation**: Verify isolated circuits
4. **Bridge Testing**: Test pluggable transports (if implemented)
5. **Performance Tests**: Measure latency, bandwidth
6. **Stress Tests**: Multiple concurrent connections

## Debug Tips

### Bootstrap Failures

```bash
# Check Tor network status
curl https://check.torproject.org/api/ip

# Check arti logs
RUST_LOG=arti_client=debug cargo test
```

### Connection Timeouts

- Increase timeout in test (Tor connections can take 10-30s)
- Check if exit node blocked destination
- Try different test destination

### Consensus Download Issues

- Delete cache: `rm -rf ~/.local/share/arti/`
- Check firewall allows Tor directory connections
- Verify system time is correct (TLS certs sensitive to clock skew)
