# IGMP E2E Test Documentation

## Test Strategy

The IGMP E2E tests verify that the LLM-controlled IGMP server correctly handles multicast group membership protocol
operations. Since there's no standard Rust IGMP client library, tests manually construct IGMP packets and verify
responses.

## Test Organization

All tests are in `tests/server/igmp/e2e_test.rs` with feature gate `#[cfg(all(test, feature = "igmp"))]`.

## Test Cases

### 1. General Query Response (`test_igmp_general_query_response`)

**Purpose**: Verify server responds to general membership queries for joined groups

**Test Flow**:

1. Start server with instruction to join 239.255.255.250
2. Send IGMPv2 Membership Query with group address 0.0.0.0 (general query)
3. Verify server responds with Membership Report (type 0x16) for 239.255.255.250

**LLM Calls**: 1 (server startup)

**Expected Runtime**: ~5 seconds

**Validation**:

- Response message type is 0x16 (Membership Report)
- Group address in report is 239.255.255.250

### 2. Group-Specific Query (`test_igmp_group_specific_query`)

**Purpose**: Verify server only responds to queries for joined groups

**Test Flow**:

1. Start server with instruction to join 224.0.1.1 and 239.1.2.3
2. Send group-specific query for 224.0.1.1 (joined group)
3. Verify server responds with report
4. Send group-specific query for 225.0.0.1 (non-joined group)
5. Verify server doesn't respond or responds appropriately

**LLM Calls**: 1 (server startup)

**Expected Runtime**: ~7 seconds

**Validation**:

- Server responds to queries for joined groups
- Server ignores or handles queries for non-joined groups gracefully

### 3. Report from Peer (`test_igmp_report_from_peer`)

**Purpose**: Verify server accepts IGMP reports from other hosts

**Test Flow**:

1. Start server with instruction to join 224.1.1.1
2. Send Membership Report from "peer" for 224.1.1.1
3. Verify server accepts packet without errors

**LLM Calls**: 1 (server startup)

**Expected Runtime**: ~5 seconds

**Validation**:

- Server accepts peer reports
- No crashes or errors
- (Optional) Server may suppress own reports per IGMP spec

### 4. Multiple Groups (`test_igmp_multiple_groups`)

**Purpose**: Comprehensive test with multiple groups and general query

**Test Flow**:

1. Start server with instruction to join 224.0.0.251 (mDNS) and 239.255.255.250 (SSDP)
2. Send general membership query
3. Verify server sends at least one report

**LLM Calls**: 1 (server startup)

**Expected Runtime**: ~8 seconds

**Validation**:

- Receives at least 1 membership report
- Reports are for joined groups

## LLM Call Budget

**Total LLM Calls**: 4 (one per test)

**Budget Compliance**: ✓ Well under 10 calls

**Efficiency**: Each test reuses server instance for multiple operations within the test

## Test Infrastructure

### Packet Construction

Tests manually construct IGMPv2 packets:

- `build_igmp_query()` - Membership Query (type 0x11)
- `build_igmp_report()` - Membership Report (type 0x16)
- `calculate_checksum()` - RFC 1071 Internet Checksum

### Packet Format (8 bytes)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Type      | Max Resp Time |           Checksum            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Group Address                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Transport

**Implementation**: IGMP server uses raw IP sockets (IPPROTO_IGMP)

**Testing**: Tests use `std::net::UdpSocket` for simplicity

- Tests send raw IGMP packets via UDP to server port
- Server implementation uses actual raw sockets with root privileges
- Test approach validates protocol logic without requiring root for test execution

**Production**: Server uses `libc::socket()` with SOCK_RAW and IPPROTO_IGMP

## Known Limitations

### 1. Test Transport

**Note**: Tests use UDP sockets while server uses raw IP sockets

**Reason**:

- Avoids requiring root privileges for test execution
- Simplifies test setup and CI/CD integration
- Server still receives/processes packets correctly

**Impact**: Tests verify protocol logic and LLM decision-making

**Production Deployment**: Server requires root or CAP_NET_RAW capability

### 2. Real Network Testing

**Limitation**: Tests use localhost loopback

**Reason**: Privacy and offline operation requirements

**Impact**: Doesn't test actual multicast routing on real networks

**For Production**: Test on real multicast-enabled network with routers sending queries

### 3. Report Suppression Timing

**Issue**: IGMPv2 includes random delay and report suppression

**Reason**: Tests use short timeouts for speed

**Impact**: May not fully test report suppression behavior

**Workaround**: Tests note that report suppression is optional behavior

## Running Tests

### Run all IGMP tests:

```bash
./cargo-isolated.sh test --no-default-features --features igmp --test server::igmp::e2e_test
```

### Run specific test:

```bash
./cargo-isolated.sh test --no-default-features --features igmp --test server::igmp::e2e_test -- test_igmp_general_query_response
```

### Prerequisites:

1. Build release binary first:
   ```bash
   ./cargo-isolated.sh build --release --features igmp
   ```

2. Ensure Ollama is running with qwen3-coder:30b model

## Test Reliability

**Timeouts**:

- Server initialization: 3 seconds
- Response wait: 5 seconds (first attempt), 2 seconds (subsequent)
- Total per test: 5-8 seconds

**Failure Modes**:

- LLM doesn't understand IGMP protocol → Bad response format
- LLM doesn't join groups → No reports sent
- LLM responds to wrong queries → Assertion failures

**Retry Logic**: None currently. Tests fail fast on errors.

## Privacy & Offline

All tests use:

- Localhost only (127.0.0.1)
- No external connections
- Multicast groups are standard (mDNS, SSDP) or test addresses

Tests work completely offline after Ollama model is downloaded.

## Future Enhancements

1. **Raw Socket Tests** (when implemented):
   ```rust
   // Verify raw IP socket with IPPROTO_IGMP
   // Verify IP_ADD_MEMBERSHIP/IP_DROP_MEMBERSHIP
   ```

2. **IGMPv3 Tests**:
   ```rust
   // Test source filtering (INCLUDE/EXCLUDE modes)
   // Test IGMPv3 report format (type 0x22)
   ```

3. **Router Mode Tests** (if implemented):
   ```rust
   // Verify router sends periodic queries
   // Verify group-specific queries after leave
   ```

4. **Performance Tests**:
   ```rust
   // Test query response timing
   // Test report suppression with multiple hosts
   ```

## Debugging

### Enable trace logging:

```bash
RUST_LOG=trace ./cargo-isolated.sh test --features igmp -- test_igmp_general_query_response --nocapture
```

### Inspect packets:

```rust
println!("Packet hex: {}", hex::encode(&packet));
```

### Common issues:

- **No response**: LLM didn't join group or didn't understand query
- **Wrong group**: LLM joined different group than expected
- **Invalid packet**: Checksum error or malformed response
- **Timeout**: LLM took too long to respond

## References

- RFC 2236: Internet Group Management Protocol, Version 2
- RFC 3376: Internet Group Management Protocol, Version 3
- Implementation: `src/server/igmp/CLAUDE.md`
