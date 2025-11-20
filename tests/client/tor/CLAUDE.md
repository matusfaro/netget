# Tor Client Testing Strategy

## Test Approach

**Fully local E2E testing** using NetGet's tor_directory server. The Tor client is configured to use a localhost directory server instead of the real Tor network.

**Key Innovation**: Arti's `TorClientConfig` can be configured with custom `fallback_caches` pointing to localhost, enabling completely offline testing!

## LLM Call Budget

**Target: < 10 LLM calls per test suite** (all mocked)

### Breakdown

1. **Local Directory Bootstrap** (4 mocked calls): Server startup, directory request, client startup, bootstrap event
2. **Directory Query Test** (6 mocked calls): Server startup, directory request, client startup, bootstrap, query action, response

**Total: 10 mocked LLM calls** (0 real Ollama calls, 0 internet required)

## Test Runtime

- **< 5 seconds per test** - Local directory server responds instantly
- **No internet required** - Fully offline testing
- **No Tor network dependency** - Uses local tor_directory server

## Test Strategy

### E2E Tests with Local Directory

All tests use a local tor_directory server + Tor client configured to use it:

**Requirements:**
- NO internet required ✓
- NO Ollama required (LLM is mocked) ✓
- NO Tor network access required ✓
- Only requires tor_directory + tor features ✓

**How it works:**
1. Start local tor_directory server (serves mock consensus)
2. Start Tor client with `directory_server` startup parameter
3. Arti configures custom FallbackDir pointing to localhost
4. Client bootstraps from local directory (instant)
5. Tests verify directory queries work

## Test Scenarios

### 1. Local Directory Bootstrap

**File**: `test_tor_client_with_local_directory()`

Tests that Tor client can bootstrap from local tor_directory server.

**LLM Calls**: 4 (mocked: server startup, directory request, client startup, bootstrap event)
**Setup**: Starts tor_directory server with 3 mock relays
**Expected**: Client bootstraps successfully using local directory
**Validation**: Mock expectations met, output shows Tor activity

**Mock consensus format**:
```
network-status-version 3
vote-status consensus
consensus-method 35
valid-after 2025-01-01 00:00:00
fresh-until 2025-01-01 01:00:00
valid-until 2025-01-01 03:00:00
r TestRelay1 AAAAAA... 127.0.0.1 9001 0 0
s Exit Fast Guard Running Stable Valid
w Bandwidth=1000
```

### 2. Directory Query with Local Server

**File**: `test_tor_directory_query_local()`

Tests directory query actions (`get_consensus_info`, `list_relays`) with local directory.

**LLM Calls**: 6 (mocked: server setup, requests, client bootstrap, query)
**Expected**: Client queries local consensus successfully
**Validation**: Mock expectations met, directory queries work

## Implementation Details

### Custom Directory Configuration

The Tor client accepts an optional `directory_server` startup parameter:

```json
{
  "type": "open_client",
  "protocol": "Tor",
  "remote_addr": "example.com:80",
  "startup_params": {
    "directory_server": "127.0.0.1:9030"
  }
}
```

When provided, the client configures `TorClientConfig` with a custom `FallbackDir`:

```rust
let mut fallback = FallbackDir::builder();
fallback
    .rsa_identity([0x42; 20].into())  // Dummy identity for testing
    .ed_identity([0x99; 32].into())
    .orports()
    .push(addr);  // localhost:port

let mut bld = TorClientConfig::builder();
bld.tor_network().set_fallback_caches(vec![fallback]);
```

This tells Arti to contact our local tor_directory server instead of real Tor authorities.

### Directory Protocol Requirements

The local tor_directory server must serve valid Tor consensus documents:
- `network-status-version 3` format
- Valid time ranges (valid-after, fresh-until, valid-until)
- At least one relay entry with `r` (router), `s` (status flags), `w` (bandwidth)
- Proper newline formatting (\n)

See `tests/server/tor_directory/e2e_test.rs` for consensus examples.

## Running Tests

```bash
# Run all Tor client tests (fully local, no internet)
cargo test --no-default-features --features tor --test client -- tor_client_tests

# With parallel execution (recommended)
cargo test --no-default-features --features tor --test client -- tor_client_tests --test-threads=100

# Single test
cargo test --no-default-features --features tor --test client -- test_tor_client_with_local_directory
```

**Expected output**:
```
test tor_client_tests::test_tor_client_with_local_directory ... ok (3.2s)
test tor_client_tests::test_tor_directory_query_local ... ok (3.5s)
```

## Test Isolation

- Each test starts independent tor_directory server on random port
- Mocked LLM (no concurrent Ollama calls)
- No shared state between tests
- Arti bootstraps from localhost only (no internet traffic)

## Known Limitations

### 1. Simplified Consensus Documents

**Issue**: Tests use minimal consensus documents (basic relay entries)

**Why**: Full Tor consensus is complex (100+ KB, cryptographic signatures, etc.)

**Impact**: Directory queries return limited relay information

**Mitigation**: Sufficient for testing infrastructure, not protocol compliance

### 2. Dummy Relay Identities

**Issue**: Mock relays use dummy RSA/Ed25519 identities

**Why**: Real identities require key generation and signing

**Impact**: Can't test cryptographic validation

**Mitigation**: Tests focus on API surface, not Tor security

### 3. No Actual Tor Connections

**Issue**: Tests don't make actual connections through Tor network

**Why**: Would require running real relays

**Impact**: Can't test .onion addresses or exit connections

**Mitigation**: Future tests can add relay simulation (separate feature)

## Advantages Over Internet-Based Testing

### Before (with internet dependency):
- ❌ Required real Tor network access
- ❌ 10-30 second bootstrap time
- ❌ Flaky (network issues, Tor blocks)
- ❌ Privacy concerns (connecting to Tor)
- ❌ Can't run in restricted environments

### Now (fully local):
- ✅ NO internet required
- ✅ < 5 second test time
- ✅ 100% reliable (no network)
- ✅ Complete privacy (localhost only)
- ✅ Works anywhere (firewall, CI, offline)

## Future Enhancements

1. **Relay Simulation**: Add mock Tor relays for testing actual connections
2. **Circuit Building**: Test circuit construction through multiple local relays
3. **Onion Services**: Test .onion address resolution with local services
4. **Consensus Validation**: Add cryptographic signature validation tests
5. **Directory Caching**: Test Arti's directory caching behavior

## Debug Tips

### Test Failures

If tests fail:
```bash
# Check compilation
cargo build --no-default-features --features tor

# Run tests with output
cargo test --no-default-features --features tor --test client -- tor_client_tests --nocapture

# Check if tor_directory server works
cargo test --no-default-features --features tor_directory --test server::tor_directory::e2e_test
```

### Bootstrap Issues

If Arti fails to bootstrap from local directory:
```bash
# Enable Arti debug logging
RUST_LOG=arti_client=debug,tor_dirmgr=debug cargo test --features tor --test client -- test_tor_client_with_local_directory --nocapture

# Check consensus format
# Arti is strict about consensus format - missing fields cause errors
```

### Mock Verification Failures

If mocks don't match:
```bash
# Run with nocapture to see mock details
cargo test --features tor --test client -- --nocapture

# Check that:
# 1. Server mock expects "tor_directory_request" event
# 2. Client mock provides "directory_server" in startup_params
# 3. Consensus format is valid (network-status-version 3, valid times, relay entries)
```

## Privacy Notes

All tests are fully local:
- Tor directory: localhost only (127.0.0.1)
- Tor client: connects to localhost only
- NO external connections
- NO Tor network traffic
- NO personal data sent anywhere

## Comparison with Other Clients

| Client | Testing Approach | Internet Required | Test Time |
|--------|------------------|-------------------|-----------|
| HTTP | Server-client localhost | NO | < 1s |
| Redis | Server-client localhost | NO | < 1s |
| TCP | Server-client localhost | NO | < 1s |
| **Tor** | **tor_directory + tor client** | **NO** | **< 5s** |
| SSH | Requires SSH server | Depends | 2-5s |

Tor client achieves the same local testing benefits as simpler protocols!
