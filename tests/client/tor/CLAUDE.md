# Tor Client Testing Strategy

## Test Approach

**Tor client testing has unique constraints** due to the Arti library's architecture:

1. **Arti requires real Tor network** - The arti-client library must bootstrap from real Tor directory authorities to download consensus
2. **No easy mocking** - Arti doesn't provide simple configuration to use mock directories without significant setup (chutney test network)
3. **Internet dependency unavoidable** - First bootstrap requires internet, but consensus is cached (~3MB in ~/.local/share/arti/)

**Current Status**: Protocol infrastructure tests only. Directory query functionality requires manual testing.

## LLM Call Budget

**Target: < 5 LLM calls per test suite** (all mocked)

### Breakdown

1. **Protocol Registration** (1 mocked call): Verify Tor protocol exists
2. **Action Definitions** (1 mocked call): Verify actions are defined

**Total: 2 mocked LLM calls** (0 real Ollama calls)

## Test Runtime

- **< 1 second** - Only protocol infrastructure tests, no actual Tor operations

## Test Strategy

### Unit Tests

Not applicable - testing requires Arti client integration, which requires Tor network.

### E2E Tests

**Smoke tests only** - Verify protocol registration and action definitions without actual Tor bootstrap.

**Requirements:**
- NO internet required
- NO Ollama required (LLM is mocked)
- NO Tor network access required

**What's NOT tested**:
- Actual Tor bootstrap
- Directory queries (get_consensus_info, list_relays, search_relays)
- Tor connections (.onion addresses, exit nodes)

### Manual Testing Required

Directory query functionality must be tested manually:
```bash
# Start netget with Tor client
cargo run --no-default-features --features tor

# In netget, try commands like:
> Connect via Tor and query consensus metadata
> Connect via Tor and list 10 relays
```

## Test Scenarios

### 1. Protocol Registration

**File**: `test_tor_protocol_registered()`

Tests that Tor client protocol is registered in the client registry.

**LLM Calls**: 1 (mocked: acknowledge instruction)
**Expected**: Protocol exists, can be referenced
**Validation**: Mock expectations met

### 2. Action Definitions

**File**: `test_tor_actions_defined()`

Tests that Tor client actions are defined (even if not functional in tests).

**LLM Calls**: 1 (mocked: acknowledge instruction)
**Expected**: Actions are queryable
**Validation**: Mock expectations met

## Known Limitations

### 1. Arti Bootstrap Requirement

**Issue**: Arti library requires bootstrapping from real Tor directory authorities.

**Why**: arti-client::TorClient::create_bootstrapped() connects to hardcoded Tor directory authorities to download consensus. This is not configurable without:
- Using chutney (Tor test network tool) - requires complex setup
- Modifying Arti source code
- Using experimental Arti APIs that aren't stable

**Impact**: Directory queries cannot be tested in automated E2E tests

**Mitigation**: Manual testing only for directory queries

### 2. No Mock Support

**Issue**: Arti doesn't provide test doubles or mock interfaces.

**Why**: Arti is designed for production Tor usage, not unit testing. Creating mocks would require:
- Mocking the entire TorClient, DirMgr, and NetDir types
- Generating fake consensus data
- Implementing Tor directory protocol

**Impact**: Can't test directory query logic without real Tor

**Mitigation**: Smoke tests verify protocol exists, manual testing for functionality

### 3. Consensus Cache Location

**Issue**: Arti caches consensus in ~/.local/share/arti/, which persists between runs.

**Why**: This is Arti's default behavior and can't be easily overridden.

**Impact**: Tests that do run Tor share cached consensus (not isolated)

**Mitigation**: For manual testing, cache speeds up subsequent runs (good thing)

## Running Tests

```bash
# Run smoke tests (no internet required)
cargo test --no-default-features --features tor --test client -- tor_client_tests

# With parallel execution
cargo test --no-default-features --features tor --test client -- tor_client_tests --test-threads=100
```

**Expected output**:
```
test tor_client_tests::test_tor_protocol_registered ... ok (0.5s)
test tor_client_tests::test_tor_actions_defined ... ok (0.5s)
```

## Manual Testing

To test directory query functionality:

```bash
# 1. Build with Tor feature
cargo build --no-default-features --features tor

# 2. Run netget
cargo run --no-default-features --features tor

# 3. Test directory queries (requires internet for first bootstrap)
> Connect via Tor to unused:80 with instruction: Query consensus metadata and show relay count
> Connect via Tor to unused:80 with instruction: List first 10 relays from directory
> Connect via Tor to unused:80 with instruction: Search for Exit relays and show 5 results
```

**First run**: 10-30 seconds (downloads consensus)
**Subsequent runs**: < 1 second (uses cached consensus)

## Future Enhancements

### Option 1: Chutney Test Network

Set up a local Tor test network using chutney:
- Requires: tor daemon, chutney tool, network configuration
- Complexity: High (30+ minutes setup)
- Benefit: Fully local testing with real Tor protocol
- Drawback: Not practical for quick test runs

### Option 2: Mock Arti with Test Doubles

Create test doubles for Arti types:
- Create FakeTorClient that implements same interface
- Generate fake consensus data
- Store in test fixtures
- Complexity: Medium (need to understand Arti internals)
- Benefit: Fast, isolated tests
- Drawback: Significant maintenance burden

### Option 3: LLM Configuration for Directories

Allow LLM to configure Arti's directory authorities:
- Add action: configure_tor_authorities with authority list
- Update TorClient to use custom TorClientConfig
- Requires: Understanding Arti's configuration API
- Complexity: Medium
- Benefit: Flexible testing with custom directories
- Drawback: Still requires running directory server

**Recommendation**: Option 2 (Mock Arti) is most practical long-term, but requires significant upfront work. For now, smoke tests + manual testing is acceptable.

## Privacy Notes

Smoke tests: No network connections.

Manual testing connects to:
- Tor directory authorities (public, expected) - downloads consensus
- No destination connections (directory queries only)
- No personal data sent

## Debug Tips

### Test Failures

If smoke tests fail:
```bash
# Check compilation
cargo build --no-default-features --features tor

# Check protocol registration
cargo run --no-default-features --features tor -- --help | grep -i tor

# Run tests with output
cargo test --no-default-features --features tor --test client -- tor_client_tests --nocapture
```

### Manual Testing Issues

If bootstrap fails during manual testing:
```bash
# Check Tor network accessibility
curl -I https://check.torproject.org

# Clear consensus cache and retry
rm -rf ~/.local/share/arti/
cargo run --no-default-features --features tor

# Check Arti logs
RUST_LOG=arti_client=debug cargo run --no-default-features --features tor
```
