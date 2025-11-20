# Tor Client Testing Strategy

## Test Approach

**Mock-driven testing** using the NetGet test infrastructure. The LLM is mocked to return specific actions, while the Tor client uses real Arti library for bootstrapping.

**Note**: While LLM calls are mocked (no Ollama needed), Tor bootstrap still requires internet connection to download consensus from Tor directory authorities. This is unavoidable without mocking the entire Arti client library.

## LLM Call Budget

**Target: < 10 LLM calls per test suite** (all mocked)

### Breakdown

1. **Bootstrap Test** (2 mocked calls): Client startup, bootstrap complete event
2. **Consensus Query** (2 mocked calls): Client startup, query action
3. **List Relays** (2 mocked calls): Client startup, list action
4. **Search Relays** (2 mocked calls): Client startup, search action

**Total: 8 mocked LLM calls** (0 real Ollama calls)

## Test Runtime

- **First run**: 10-30 seconds (Arti bootstraps and downloads consensus)
- **Subsequent runs**: < 1 second (consensus cached in ~/.local/share/arti/)
- **Total suite**: ~30 seconds first run, < 5 seconds cached

## Test Strategy

### Unit Tests

Not applicable - testing requires Arti client integration.

### E2E Tests

All tests use mocked LLM responses but real Tor bootstrap:

**Requirements:**
- Internet connection (for Arti bootstrap only)
- NO Ollama required (LLM is mocked)
- Tor network access (not blocked by firewall/country)
- ~3MB disk space for consensus cache

**Mock Pattern:**
```rust
NetGetConfig::new("instruction")
    .with_mock(|mock| {
        mock
            .on_instruction_containing("...")
            .respond_with_actions(serde_json::json!([...]))
            .expect_calls(1)
            .and()
            .on_event("tor_bootstrap_complete")
            .respond_with_actions(serde_json::json!([...]))
            .expect_calls(1)
            .and()
    })
```

### Test Isolation

- Mocked LLM (no concurrent Ollama calls)
- Shared Arti consensus cache (faster subsequent runs)
- Each test uses independent client instance

## Test Scenarios

### 1. Bootstrap with Mocked LLM

**File**: `test_tor_client_bootstrap_mocked()`

Tests that Tor client can bootstrap and emit the `tor_bootstrap_complete` event.

**LLM Calls**: 2 (mocked: open_client instruction, bootstrap event)
**Expected**: Client starts, Arti bootstraps, event emitted
**Validation**: Mock expectations met, output contains "Tor" or "CLIENT"

### 2. Directory Query: Consensus Info

**File**: `test_tor_directory_query_consensus()`

Tests `get_consensus_info` action after bootstrap.

**LLM Calls**: 2 (mocked: open_client, get_consensus_info)
**Expected**: Bootstrap completes, consensus info queried
**Validation**: Mock expectations met

**Action Format**:
```json
{
    "type": "get_consensus_info"
}
```

**Expected Response**: JSON with `relay_count`, `valid_after`, `fresh_until`, `valid_until`

### 3. Directory Query: List Relays

**File**: `test_tor_directory_list_relays()`

Tests `list_relays` action with limit parameter.

**LLM Calls**: 2 (mocked: open_client, list_relays)
**Expected**: Bootstrap completes, relays listed
**Validation**: Mock expectations met

**Action Format**:
```json
{
    "type": "list_relays",
    "limit": 10
}
```

**Expected Response**: Array of RelayInfo (fingerprint, nickname, flags)

### 4. Directory Query: Search by Flags

**File**: `test_tor_directory_search_relays()`

Tests `search_relays` action with flag filter.

**LLM Calls**: 2 (mocked: open_client, search_relays)
**Expected**: Bootstrap completes, filtered relays returned
**Validation**: Mock expectations met

**Action Format**:
```json
{
    "type": "search_relays",
    "flags": ["Exit"],
    "limit": 5
}
```

**Expected Response**: Array of RelayInfo matching flags

**Note**: Flag filtering not yet implemented (tor-netdir API limitation), so all relays returned regardless of flags.

## Known Issues

### 1. Internet Dependency

**Issue**: Tests require internet for Arti bootstrap (downloads consensus from Tor directory authorities)

**Mitigation**:
- Bootstrap is cached (~3MB in ~/.local/share/arti/)
- Subsequent test runs use cached consensus (< 1s)
- Tests fail fast if Tor network unavailable

**Future**: Mock Arti client for fully offline testing

### 2. Tor Network Blocking

**Issue**: Some countries/networks block Tor directory connections

**Symptoms**:
- Bootstrap timeout (> 30s)
- Connection refused errors

**Mitigation**:
- Skip tests in restricted environments
- Use `#[ignore]` attribute if needed for CI
- Document known blocked regions

### 3. Consensus Staleness

**Issue**: Cached consensus expires after ~3 hours

**Symptoms**:
- Tests suddenly slow down (re-downloading consensus)

**Mitigation**:
- Arti automatically refreshes stale consensus
- Tests remain functional, just slower

### 4. API Limitations

**Issue**: tor-netdir doesn't expose relay flags/nicknames in public API

**Impact**:
- RelayInfo returns placeholder values (fingerprint only)
- Flag filtering not implemented (all relays returned)

**Mitigation**:
- Tests verify API surface, not relay details
- Update when Arti exposes more metadata

## Running Tests

```bash
# Run all Tor client tests (requires internet for first run)
./test-e2e.sh tor

# Or with cargo directly
cargo test --no-default-features --features tor --test client -- tor_client_tests

# With parallel execution (recommended)
cargo test --no-default-features --features tor --test client -- tor_client_tests --test-threads=100

# Clear consensus cache (force re-bootstrap)
rm -rf ~/.local/share/arti/
```

**First Run** (slow):
```
test tor_client_tests::test_tor_client_bootstrap_mocked ... ok (28.3s)
test tor_client_tests::test_tor_directory_query_consensus ... ok (0.5s)
test tor_client_tests::test_tor_directory_list_relays ... ok (0.5s)
test tor_client_tests::test_tor_directory_search_relays ... ok (0.5s)
```

**Subsequent Runs** (fast, cached):
```
test tor_client_tests::test_tor_client_bootstrap_mocked ... ok (0.3s)
test tor_client_tests::test_tor_directory_query_consensus ... ok (0.2s)
test tor_client_tests::test_tor_directory_list_relays ... ok (0.2s)
test tor_client_tests::test_tor_directory_search_relays ... ok (0.2s)
```

## Privacy Notes

Tests connect to:
- Tor directory authorities (public, expected) - downloads consensus
- No destination connections (directory queries only)
- No personal data sent

All connections are for Tor consensus download only (same as running any Tor client).

## Debug Tips

### Bootstrap Failures

```bash
# Check if Tor network is accessible
curl -I https://check.torproject.org

# Check Arti logs
RUST_LOG=arti_client=debug cargo test --features tor --test client -- tor_client_tests::test_tor_client_bootstrap_mocked

# Verify consensus cache
ls -lh ~/.local/share/arti/
```

### Test Hangs

- Increase timeout (bootstrap can take 30s on slow networks)
- Check firewall allows outbound connections to Tor ports (9001, 9030, 443)
- Verify system time is correct (TLS sensitive to clock skew)

### Mock Verification Failures

- Check that actions match expected format exactly
- Verify event names match constants in `actions.rs`
- Use `--nocapture` to see mock mismatch details:
  ```bash
  cargo test --features tor --test client -- tor_client_tests --nocapture
  ```

## Future Enhancements

1. **Mock Arti Client**: Fully offline tests without real Tor bootstrap
2. **Connection Tests**: Test actual connections through Tor (onion services, exit nodes)
3. **Circuit Management**: Test circuit building and isolation
4. **Performance**: Measure bootstrap time, query latency
5. **Stress Testing**: Multiple concurrent clients, many queries
6. **Flag Filtering**: Implement when tor-netdir exposes flag checking API
