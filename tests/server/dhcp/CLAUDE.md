# DHCP Protocol E2E Tests

## Test Overview
Tests DHCP server implementation with DISCOVER/OFFER and REQUEST/ACK flows. Validates lease options and network configuration. Uses manual DHCP packet construction and parsing (no high-level DHCP client library available).

## Test Strategy
- **Isolated test servers**: Each test spawns separate NetGet instance with DHCP configuration
- **Manual packet construction**: Creates DHCP messages from scratch (no library)
- **Raw UDP sockets**: Uses std::net::UdpSocket for sending/receiving
- **Partial validation**: Tests that server responds, but doesn't fully validate packet structure
- **No scripting**: Action-based LLM responses

**Note**: DHCP testing is more challenging than DNS/NTP because there's no mature Rust DHCP client library. Tests construct raw DHCP packets manually.

## LLM Call Budget
- `test_dhcp_discover_offer()`: 1 LLM call (DISCOVER → OFFER)
- `test_dhcp_request_ack()`: 1 LLM call (REQUEST → ACK)
- `test_dhcp_lease_options()`: 1 LLM call (DISCOVER with options)
- **Total: 3 LLM calls** (well under 10 limit)

**Optimization Opportunity**: Could consolidate into single server handling all DHCP message types, reducing to 1 startup call + 3 request calls = 4 total.

## Scripting Usage
❌ **Scripting Disabled** - Action-based responses only

**Rationale**: Tests validate LLM's ability to generate DHCP responses using structured actions (send_dhcp_offer, send_dhcp_ack). Scripting would bypass this validation.

## Client Library
- **Manual DHCP packet construction** - No library used
  - `create_dhcp_discover()` - Builds DISCOVER packet from scratch
  - `parse_dhcp_message_type()` - Extracts message type from response
  - Raw UDP socket for transport

**Why manual construction?**:
1. No mature Rust DHCP client library available
2. dhcproto is server-focused (Message struct works but awkward for clients)
3. DHCP packet structure is relatively simple
4. Tests only need basic DISCOVER/REQUEST messages

**Packet Structure**:
- 300 bytes: BOOTP header + DHCP options
- Transaction ID: Random 32-bit value
- Client MAC: Hardcoded (00:11:22:33:44:55)
- Magic cookie: 99.130.83.99 (DHCP signature)
- Options: Message Type (53), End (255)

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~30-40 seconds for full test suite (3 tests × ~10s each)
- Each test includes: server startup (2-3s) + LLM response (5-8s) + UDP exchange (<1s)

**Note**: DHCP tests may timeout more frequently than DNS because:
- LLM may not understand DHCP protocol as well as DNS
- Manual packet construction may have subtle errors
- DHCP servers often ignore malformed requests silently

## Failure Rate
- **Moderate** (~10-15%) - Higher than DNS/NTP
- Most common failure: LLM doesn't send DHCP response at all
- Timeout failures: ~5% - 5-second socket timeout
- Parsing failures: Rare - only parse message type, very lenient

**Why higher failure rate?**:
1. DHCP is more complex than DNS (4-message flow, many options)
2. LLM might not understand DISCOVER→OFFER→REQUEST→ACK sequence
3. Manual packet construction might trigger edge cases in dhcproto parsing
4. Tests are lenient (accept any response, not just valid DHCP)

## Test Cases

### 1. DHCP DISCOVER/OFFER (`test_dhcp_discover_offer`)
- **Prompt**: "listen on port {port} via dhcp. When receiving DHCP DISCOVER messages, respond with DHCP OFFER. Offer IP addresses in the 192.168.1.0/24 range starting from 192.168.1.100"
- **Client**: Sends DHCP DISCOVER packet
- **Expected**: Receives any UDP response (ideally DHCP OFFER)
- **Purpose**: Tests basic DHCP DISCOVER handling
- **Validation**: Accepts any response (tries to parse message type but doesn't require it)
- **Note**: Test is lenient because LLM might send response in unexpected format

### 2. DHCP REQUEST/ACK (`test_dhcp_request_ack`)
- **Prompt**: "listen on port {port} via dhcp. Handle DHCP DISCOVER and REQUEST messages. Assign IP addresses from 192.168.1.100 onwards. Respond with OFFER to DISCOVER and ACK to REQUEST"
- **Client**: Sends DHCP REQUEST packet (modified DISCOVER with message type = 3)
- **Expected**: Receives UDP response
- **Purpose**: Tests REQUEST message handling
- **LLM Challenge**: Must understand two different message types in same prompt
- **Validation**: Lenient - accepts any response

### 3. DHCP Lease Options (`test_dhcp_lease_options`)
- **Prompt**: "listen on port {port} via dhcp. Respond to DHCP requests with: IP address 192.168.1.100, subnet mask 255.255.255.0, gateway 192.168.1.1, DNS server 8.8.8.8, lease time 86400 seconds"
- **Client**: Sends DHCP DISCOVER
- **Expected**: Receives response with options
- **Purpose**: Tests DHCP options (subnet mask, router, DNS, lease time)
- **Validation**: Just checks that response was received (doesn't parse options)
- **Note**: Full option validation would require parsing entire DHCP packet

## Known Issues

### 1. Lenient Validation
Tests accept ANY UDP response as success, even if it's not a valid DHCP packet. This is intentional due to:
- LLM response variability
- Complexity of full DHCP packet parsing in test code
- Goal: Test that LLM attempts to respond, not protocol perfection

**Future Improvement**: Use dhcproto to parse responses and validate:
- Message type (OFFER, ACK)
- Offered/assigned IP address
- DHCP options presence and values

### 2. No Full DORA Flow Test
Tests send individual messages (DISCOVER or REQUEST) but don't test full 4-message flow:
1. Client DISCOVER
2. Server OFFER
3. Client REQUEST
4. Server ACK

**Reason**: Would require stateful test logic and multiple LLM calls per test.

**Future Enhancement**: Add test that performs complete DHCP lease acquisition.

### 3. Manual Packet Construction Fragility
`create_dhcp_discover()` hardcodes packet structure:
- Fixed MAC address
- No DHCP options beyond message type
- Minimal BOOTP fields populated

This might not trigger all DHCP server code paths.

### 4. No Broadcast Flag Testing
DHCP can use broadcast or unicast for responses. Tests don't verify which is used (accept both).

### 5. No NAK Testing
No test for DHCP NAK (rejection). Would require:
- Prompt instructing server to reject certain requests
- Client sending REQUEST for invalid IP
- Parsing NAK message type from response

## Performance Notes

### Why No dhcproto Client?
The dhcproto library provides `Message` type but:
1. Designed for server-side use (parsing client requests, building server responses)
2. Client-side usage is awkward (manually setting all BOOTP fields)
3. Simpler to construct raw packets for testing

For production DHCP client, dhcproto would be appropriate but requires more code.

### Socket Timeout
5-second read timeout provides balance:
- Allows for slow LLM responses (typically 2-5s)
- Fails fast on non-responsive servers
- Longer than DNS (DNS is faster protocol)

### DHCP Protocol Characteristics
DHCP is slower than DNS because:
- Larger packets (300+ bytes vs 50-200 for DNS)
- More complex processing (multiple message types, options, state)
- 4-message flow for full lease (vs 1 query/response for DNS)

## Future Enhancements

### Test Coverage Gaps
1. **Full DORA flow**: Complete DISCOVER→OFFER→REQUEST→ACK sequence
2. **DHCP RELEASE**: Client releasing IP address
3. **DHCP RENEW**: Client renewing lease
4. **DHCP DECLINE**: Client rejecting offered IP
5. **DHCP INFORM**: Client requesting configuration without IP
6. **Option validation**: Parse and verify subnet mask, router, DNS
7. **Lease time validation**: Verify lease time in ACK response
8. **Server identifier**: Verify server IP in responses
9. **Transaction ID**: Verify xid is echoed correctly
10. **MAC address**: Verify chaddr is echoed correctly

### Better Validation
Currently tests only check for "any response". Should validate:
- Response is valid DHCP packet (magic cookie present)
- Message type is correct (OFFER for DISCOVER, ACK for REQUEST)
- Offered IP is in expected range
- Required options are present

Could use dhcproto::v4::Message::decode() for validation:
```rust
match v4::Message::decode(&mut Decoder::new(&buffer[..n])) {
    Ok(msg) => {
        assert_eq!(msg.message_type(), Some(MessageType::Offer));
        assert_eq!(msg.yiaddr(), expected_ip);
        // Validate options...
    }
    Err(e) => panic!("Invalid DHCP response: {}", e),
}
```

### Consolidation Opportunity
All three tests could share one server:
```rust
let prompt = format!(
    "listen on port {} via dhcp.
    For DHCP DISCOVER: Send OFFER with IP 192.168.1.100-200
    For DHCP REQUEST: Send ACK with same IP, subnet mask 255.255.255.0, gateway 192.168.1.1, DNS 8.8.8.8
    Lease time: 86400 seconds",
    port
);
```

This would reduce from 3 servers to 1, saving ~6-9 seconds.

### Scripting Mode Test
Add test with scripting enabled:
- Verify script handles DISCOVER and REQUEST correctly
- Ensure script assigns IPs deterministically
- Test throughput improvement (should be 1000x faster)

## References
- [RFC 2131: DHCP Protocol](https://datatracker.ietf.org/doc/html/rfc2131)
- [RFC 2132: DHCP Options](https://datatracker.ietf.org/doc/html/rfc2132)
- [RFC 951: Bootstrap Protocol (BOOTP)](https://datatracker.ietf.org/doc/html/rfc951)
- [dhcproto Documentation](https://docs.rs/dhcproto/latest/dhcproto/)
- [DHCP Message Format](https://en.wikipedia.org/wiki/Dynamic_Host_Configuration_Protocol#DHCP_message_format)
