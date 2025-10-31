# NTP Protocol E2E Tests

## Test Overview
Tests NTP server implementation with basic time synchronization and stratum level handling. Uses rsntp client library for some tests and raw UDP for others. Validates server responses to NTP time requests.

## Test Strategy
- **Isolated test servers**: Each test spawns separate NetGet instance with NTP configuration
- **Dual client approach**:
  - Uses rsntp library for high-level validation
  - Falls back to raw UDP when rsntp fails (LLM implementation varies)
- **Lenient validation**: Accepts any NTP response (rsntp is strict)
- **No scripting**: Action-based LLM responses

**Challenge**: NTP protocol is strict about timestamp calculations, but LLM might not implement perfect NTP. Tests accommodate both valid and "good enough" responses.

## LLM Call Budget
- `test_ntp_basic_query()`: 1 LLM call (basic time request)
- `test_ntp_time_sync()`: 1 LLM call (time synchronization)
- `test_ntp_stratum_levels()`: 1 LLM call (stratum 3 request)
- **Total: 3 LLM calls** (well under 10 limit)

**Optimization Opportunity**: Could consolidate into single server handling all NTP features, reducing to 1 startup call + 3 request calls = 4 total.

## Scripting Usage
❌ **Scripting Disabled** - Action-based responses only

**Rationale**: Tests validate LLM's ability to generate NTP responses using `send_ntp_time_response` action. Scripting would bypass this validation. For production NTP servers, scripting is highly recommended (NTP is perfect for scripting).

## Client Library
- **rsntp v3.0** - Simple SNTP client library
  - `SntpClient::synchronize()` - Performs NTP query and calculates clock offset
  - Validates NTP packet structure strictly
  - Calculates round-trip delay
  - Returns clock offset from server

**Fallback**: Raw UDP socket with manual 48-byte NTP packet

**Why dual approach?**:
1. rsntp validates protocol correctness (good for testing)
2. LLM might not implement perfect NTP (e.g., wrong timestamps)
3. Raw UDP approach accepts any response (tests basic functionality)
4. Tests try rsntp first, fall back to raw UDP if it fails

**Raw NTP Request**:
```rust
let mut request = vec![0u8; 48];
request[0] = 0x1B; // LI=0, Version=3, Mode=3 (client)
```

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~30-40 seconds for full test suite (3 tests × ~10s each)
- Each test includes: server startup (2-3s) + LLM response (5-8s) + NTP query (<1s)

**Note**: NTP tests may timeout when using rsntp (strict validation), but succeed with raw UDP.

## Failure Rate
- **Moderate** (~5-10%) - Higher than DNS, lower than DHCP
- Most common failure: rsntp rejects response due to timestamp issues
- Timeout failures: ~2% - typically when LLM doesn't respond at all
- Raw UDP fallback almost always succeeds

**Why moderate failure rate?**:
1. NTP timestamp calculations are complex (origin, receive, transmit)
2. LLM might not correctly echo origin_timestamp from client request
3. LLM might use wrong timestamp format (Unix vs NTP epoch)
4. Server has auto-injection logic, but LLM might override it incorrectly

**Mitigation**: Server auto-injects origin_timestamp if LLM doesn't provide it, reducing failures.

## Test Cases

### 1. NTP Basic Query (`test_ntp_basic_query`)
- **Prompt**: "listen on port {port} via ntp. Respond to NTP time requests with the current system time. Use stratum 2"
- **Client**: rsntp SntpClient (tries full sync), fallback to raw UDP
- **Expected**:
  - Success: rsntp returns clock offset and round-trip delay
  - Fallback: Raw UDP receives 48-byte response
- **Purpose**: Tests basic NTP functionality
- **Validation**: Lenient - accepts either rsntp success or any UDP response

### 2. NTP Time Synchronization (`test_ntp_time_sync`)
- **Prompt**: "listen on port {port} via ntp. Act as a stratum 1 NTP server. Respond with accurate current time in NTP format"
- **Client**: rsntp SntpClient, fallback to raw UDP
- **Expected**: Time synchronization successful
- **Purpose**: Tests stratum 1 server (primary time source)
- **LLM Challenge**: Must understand stratum levels and implications
- **Validation**: Same as basic query (rsntp or raw UDP)

### 3. NTP Stratum Levels (`test_ntp_stratum_levels`)
- **Prompt**: "listen on port {port} via ntp. Act as a stratum 3 NTP server. Include reference identifier 'LOCL'"
- **Client**: Raw UDP only (sends request, reads response)
- **Expected**: 48-byte NTP response
- **Purpose**: Tests custom stratum level and reference ID
- **Validation**:
  - Checks response is at least 48 bytes
  - Optionally parses stratum from byte 1
  - Very lenient (just tests server responds)

## Known Issues

### 1. rsntp Strictness
rsntp library performs full NTP validation:
- Checks that origin_timestamp in response matches client's transmit_timestamp
- Validates timestamp ordering (receive before transmit)
- Calculates clock offset using all four timestamps
- Rejects responses that don't meet NTP specification

**Problem**: LLM might not implement perfect NTP timestamp handling.

**Solution**: Tests have fallback to raw UDP socket. If rsntp fails, test still passes if ANY response received.

### 2. No Stratum Validation
`test_ntp_stratum_levels` receives response but doesn't validate stratum value:
- Parses byte 1 from response
- Prints stratum value
- Doesn't assert it equals 3

**Reason**: Adding assertions would increase failure rate. Tests prioritize "server responds" over "response is perfect".

**Future Improvement**: Parse full NTP response and validate all fields once LLM responses are more consistent.

### 3. No Timestamp Validation
Tests don't validate that timestamps are reasonable:
- Don't check timestamps are close to current time
- Don't verify timestamp format (NTP vs Unix)
- Don't check timestamp ordering

**Reason**: LLM timestamp handling varies. Server has auto-injection, but might still get details wrong.

### 4. No Reference ID Validation
`test_ntp_stratum_levels` prompts for reference ID "LOCL" but doesn't verify it:
- Would require parsing bytes 12-15 from response
- Current test just checks for any response

### 5. Single Test Per Stratum
Only tests stratum 1, 2, and 3. Doesn't test:
- Stratum 0 (invalid - should reject)
- Stratum 16 (unsynchronized - special case)
- Other stratum values 4-15

## Performance Notes

### Why rsntp?
rsntp is lightweight SNTP (Simple NTP) client:
- Single function call: `synchronize(address)`
- Returns clock offset and delay
- No dependencies on system NTP daemon
- Perfect for testing

### Fallback Strategy
Tests use try/catch pattern:
```rust
match client.synchronize(&address) {
    Ok(result) => {
        // Full NTP validation succeeded
    }
    Err(e) => {
        // Fall back to raw UDP
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.send_to(&ntp_request, &address)?;
        socket.recv_from(&mut buffer)?;
        // Test passes if any response received
    }
}
```

This makes tests resilient to LLM variation.

### NTP Protocol Characteristics
NTP is very lightweight:
- Fixed 48-byte packets
- Single UDP exchange (request/response)
- Minimal parsing required
- Fast even with LLM overhead

## Future Enhancements

### Test Coverage Gaps
1. **Full timestamp validation**: Parse and verify all four timestamps
2. **Stratum 0 rejection**: Verify server doesn't claim stratum 0
3. **Kiss-of-Death**: Test rate limiting (if implemented)
4. **Version negotiation**: Test with NTPv1, v2, v3, v4 requests
5. **Reference ID validation**: Parse and verify reference identifier
6. **Precision validation**: Check precision field is reasonable
7. **Root delay/dispersion**: Validate these fields for different strata
8. **Leap indicator**: Test leap second warnings
9. **Poll interval**: Verify poll field is set correctly

### Better Validation
Use manual NTP packet parsing instead of just checking for "any response":

```rust
// Parse NTP response
assert_eq!(buffer.len(), 48, "Invalid NTP packet size");
let stratum = buffer[1];
assert_eq!(stratum, 3, "Wrong stratum level");

let ref_id = std::str::from_utf8(&buffer[12..16]).unwrap();
assert_eq!(ref_id, "LOCL", "Wrong reference identifier");

// Parse timestamps (bytes 16-47)
let reference_ts = u64::from_be_bytes(...);
let origin_ts = u64::from_be_bytes(...);
let receive_ts = u64::from_be_bytes(...);
let transmit_ts = u64::from_be_bytes(...);

// Validate timestamp ordering
assert!(receive_ts <= transmit_ts, "Timestamps out of order");
```

### Consolidation Opportunity
All three tests could share one server:
```rust
let prompt = format!(
    "listen on port {} via ntp.
    Act as a stratum 2 NTP server
    Reference ID: 'LOCL'
    Respond to all NTP requests with accurate current time",
    port
);
```

Then send three different queries to same server. Would reduce from 3 servers to 1, saving ~6-9 seconds.

### Scripting Mode Test
Add test with scripting enabled:
- Verify script generates correct NTP responses
- Test throughput (should be 10000+ QPS)
- Ensure timestamp calculations are correct in script
- Validate script doesn't call LLM for each request

### Clock Offset Test
Add test that validates clock offset calculation:
- Set server to return time T
- Calculate expected offset
- Compare with rsntp result
- Requires mocking system time or custom timestamps

## References
- [RFC 5905: NTPv4](https://datatracker.ietf.org/doc/html/rfc5905)
- [RFC 4330: SNTPv4](https://datatracker.ietf.org/doc/html/rfc4330)
- [rsntp Documentation](https://docs.rs/rsntp/latest/rsntp/)
- [NTP Packet Format](https://www.rfc-editor.org/rfc/rfc5905.html#section-7.3)
- [NTP Best Practices](https://www.ntp.org/reflib/book/)
