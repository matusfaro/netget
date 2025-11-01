# Tor Relay Protocol E2E Tests

## Test Overview

Tests validate the Tor Relay exit relay implementation by establishing TLS connections and verifying the relay accepts circuit creation and stream requests.

**Note**: Full Tor protocol testing requires implementing a complete ntor handshake and cell encryption client, which is beyond the scope of E2E tests. Tests verify server startup, TLS connectivity, and cell processing infrastructure.

## Test Strategy

**Minimal E2E Testing**: Due to complexity of Tor protocol (ntor handshake, cell encryption, circuit management), E2E tests focus on infrastructure:
- Server starts correctly with Tor Relay stack
- TLS connection establishment works
- Server accepts cell data without crashing
- Server processes CREATE2 cells (even if handshake fails)

**Real Protocol Testing**: Requires full Tor client implementation:
- Complete ntor handshake with key derivation
- AES-128-CTR encryption/decryption
- Circuit multiplexing
- Stream management
- Flow control (SENDME cells)

**Verification Approach**: Infrastructure + unit tests + manual testing with real Tor client

## LLM Call Budget

### Test: `test_tor_relay_with_http_server`
- **Server startup**: 1 LLM call (interprets prompt, starts relay)
- **TLS connection test**: 0 LLM calls (protocol infrastructure only)
- **Cell send test**: 0 LLM calls (verifies server doesn't crash)

**Total: 1 LLM call per test run**

### Future Tests (Not Implemented)
- Full ntor handshake test: Would require 1 LLM call (but needs complete client)
- Stream establishment test: Would require 1 LLM call (needs circuit first)
- Data forwarding test: Would require 1 LLM call (needs active stream)

**Estimated Budget if Full Client Implemented**: 3-4 LLM calls

**Current Budget**: 1 LLM call (infrastructure testing only)

## Scripting Usage

**Scripting NOT Applicable**: Tor Relay protocol is deterministic cryptographic protocol with no LLM decision points during cell processing:
- ntor handshake follows exact mathematical specification
- Cell encryption/decryption is deterministic AES-CTR
- Flow control windows follow fixed rules
- Stream forwarding is transparent TCP proxy

**LLM Use Cases**:
- Policy decisions (exit policy, relay flags) - not yet implemented
- Monitoring and statistics interpretation
- Circuit creation event logging

**Scripting Status**: Disabled (`ServerConfig::new_no_scripts()` in tests)

## Client Library

**No Standard Client Library Available**: Tor protocol requires:
1. **Manual TLS Client**: `tokio_rustls::TlsConnector` with custom certificate verifier
2. **Manual Cell Construction**: Building 514-byte cells (4 circid + 1 cmd + 509 payload)
3. **Manual ntor Handshake**: Would require x25519-dalek, sha2, hmac, hkdf implementation
4. **Manual Cell Encryption**: Would require AES-128-CTR cipher state management

**Test Implementation**:
- Custom `NoCertVerifier` for self-signed certificates
- Manual cell structure (514 bytes, CREATE2 command)
- No encryption (server handles gracefully)

**Why Not Use Arti?**:
- Arti is a full Tor client (thousands of lines)
- Overkill for E2E testing
- Would hide implementation details we're testing

## Expected Runtime

**Model**: qwen3-coder:30b (default model)

**Test Duration**:
- Server startup: ~3 seconds (includes LLM call + TLS cert generation)
- TLS connection: ~100ms
- Cell send/receive: ~100ms
- Total: **~3-4 seconds per test**

**Comparison**: Much faster than full protocol test would be (which would require multiple LLM calls and complex client logic)

## Failure Rate

**Current Status**: **Stable** (< 1% failure rate)

**Potential Failure Modes**:
- Ollama timeout on slow machines (startup LLM call)
- TLS handshake failure (rare, certificate generation issue)
- Port already in use (resolved by port 0 dynamic allocation)

**Not Tested** (Expected Failure if Attempted):
- ntor handshake validation (requires complete client)
- Cell encryption correctness (requires AES-CTR client)
- Flow control SENDME (requires multi-cell exchange)

## Test Cases

### 1. `test_tor_relay_with_http_server`

**Purpose**: Verify Tor Relay server starts, accepts TLS connections, and processes cell data

**Setup**:
1. Start HTTP test server on localhost (destination for exit traffic)
2. Start NetGet Tor Relay with prompt: "Start a Tor exit relay on port 0 that allows connections to localhost"
3. Verify stack name: `ETH>IP>TCP>TLS>TorRelay`

**Test Steps**:
1. Establish TLS connection with custom certificate verifier
2. Send CREATE2 cell (circuit_id=1, invalid handshake data)
3. Attempt to read response with 2-second timeout

**Expected Behavior**:
- Server accepts TLS connection
- Server doesn't crash on receiving cell
- Server either responds with error or closes gracefully

**Assertions**:
- TLS connection succeeds
- Server processes cell without panic
- Response or timeout (both acceptable for invalid cell)

**LLM Calls**: 1 (server startup only)

**Note**: Test includes HTTP server for future exit relay testing when full client is implemented

## Known Issues

### Limitation: No Full Protocol Testing
- Tests verify infrastructure, not protocol correctness
- ntor handshake tested in unit tests (circuit.rs)
- Cell encryption tested in unit tests
- Flow control tested in unit tests

**Rationale**: Implementing full Tor client for E2E tests would be:
- 500+ lines of crypto code
- Duplicate effort (Arti already exists)
- Maintenance burden
- Not focused on testing NetGet's implementation

### Future Enhancement: Manual Tor Client
If needed, could implement minimal client for E2E testing:
- Use `x25519-dalek` for ntor handshake
- Use `aes` + `ctr` for cell encryption
- Implement BEGIN → DATA → END sequence
- Test actual exit relay functionality

**Estimated Effort**: 2-3 hours implementation + 1 hour testing

## Test Infrastructure

### Helper Servers
- **HTTP Test Server**: Listens on localhost, responds to requests
- Used for validating exit relay forwards traffic correctly
- Current test doesn't exercise this (needs full client)

### TLS Configuration
- `aws-lc-rs` crypto provider (required for rustls 0.23+)
- TLS 1.3 protocol version
- Custom `NoCertVerifier` accepts self-signed certificates
- ServerName: "tor-relay.local"

### Test Utilities
- `start_netget_relay()`: Starts server with standard configuration
- `start_test_http_server()`: Spawns local HTTP echo server
- `assert_stack_name()`: Validates correct protocol stack

## Comparison with Other Protocols

**Similar Complexity**:
- SSH: Also uses cryptographic handshake (but ssh2-rs client exists)
- TLS protocols: Self-contained (tokio-rustls handles both sides)

**Tor Unique Challenges**:
- No client library suitable for testing
- Multi-layer encryption (circuit crypto + stream multiplexing)
- Complex state machine (circuits, streams, flow control)
- Requires understanding of Tor specification

**Test Approach**: Infrastructure testing + unit tests + manual validation

## Manual Testing Instructions

To test Tor Relay with real Tor client:

1. **Start NetGet Relay**:
   ```bash
   ./cargo-isolated.sh run --features tor-relay --release
   # Prompt: "Start a Tor exit relay on port 9001"
   ```

2. **Configure Tor Client** (torrc):
   ```
   UseBridges 0
   TestingTorNetwork 1
   EntryNodes <relay-fingerprint>
   ExitNodes <relay-fingerprint>
   ```

3. **Connect Through Relay**:
   ```bash
   curl --socks5 127.0.0.1:9050 http://example.com
   ```

4. **Verify**:
   - NetGet logs show circuit creation
   - NetGet logs show stream opening
   - NetGet logs show data forwarding
   - curl receives response

**Expected Logs**:
```
[INFO] Circuit 0x00000001 created
[INFO] BEGIN stream 1 → example.com:80
[DEBUG] Stream 1 forwarded 123 bytes
```
