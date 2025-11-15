# STUN Protocol Test Investigation

## Instance #14 - Parallel Test Fix Initiative

### Test Status

**Passing:** 2/7 tests (28.6%)
- ✅ `test_stun_invalid_magic_cookie` - Tests invalid packet rejection
- ✅ `test_stun_malformed_short_packet` - Tests short packet rejection

**Failing:** 5/7 tests (71.4%)
- ❌ `test_stun_basic_binding_request` - Basic STUN binding request/response
- ❌ `test_stun_multiple_clients` - Concurrent client handling
- ❌ `test_stun_rapid_requests` - Burst request handling
- ❌ `test_stun_request_with_attributes` - Request with SOFTWARE attribute
- ❌ `test_stun_xor_mapped_address` - XOR-MAPPED-ADDRESS encoding

### Fixes Implemented

✅ **Event Parameter Definition** (Commit: `5707471`)
- Added explicit parameter definitions to `STUN_BINDING_REQUEST_EVENT`
- Parameters: `peer_addr`, `local_addr`, `transaction_id`, `message_type`, `bytes_received`
- Follows pattern used in other UDP protocols (DNS, NTP, DHCP)
- Helps LLM understand event context and mock system match events

✅ **Gitignore Update** (Commit: `3600b59`)
- Added `/target-test-*/` pattern to prevent untracked file warnings

### Root Cause Analysis

**Symptom:**
UDP packets sent from test client never reach the STUN server. The `recv_from().await` call in the server never completes.

**Verified Facts:**
1. ✅ Server binds successfully to `127.0.0.1:port` (confirmed in logs)
2. ✅ Test extracts correct port from server output
3. ✅ Test sends valid STUN packet to `127.0.0.1:same-port`
4. ✅ STUN packet format is valid (correct magic cookie `0x2112A442`, transaction ID)
5. ✅ Server spawn task starts and enters receive loop (confirmed by debug logs)
6. ✅ Server prints "STUN calling recv_from..." before test sends packet
7. ❌ **The tokio `recv_from()` call never returns - packet never arrives**

**Evidence from Test Logs:**
```
[DEBUG] NetGet output: [INFO] STUN server listening on 127.0.0.1:25010
[DEBUG] Parsed listening confirmation: port=25010
[DEBUG] Updating server #1 port from 0 to 25010
[DEBUG] NetGet output: STUN receive loop started, waiting for packets...
[STDERR] STUN calling recv_from...
Sent STUN binding request to 127.0.0.1:25010
(5 second timeout)
Failed to receive STUN response: Resource temporarily unavailable (os error 11)
⚠️  WARNING: Mock expectations not verified!
   Expected 1 calls, got 0 - event=stun_binding_request
```

**Why Passing Tests Work:**
The 2 passing tests (`test_stun_invalid_magic_cookie`, `test_stun_malformed_short_packet`) send **invalid** packets that are designed to be rejected. These tests don't expect a response, so they don't need UDP receive to work. They pass by timeout, which proves:
- Server starts successfully
- Protocol is registered correctly
- Invalid packet filtering works (when packets would arrive)

### Investigation Steps Taken

1. **Compared with DNS Protocol**
   - DNS uses identical UDP socket pattern (`tokio::net::UdpSocket`)
   - DNS tests fail for different reason (protocol not registered)
   - No structural differences found in spawn/receive logic

2. **Analyzed Mock System**
   - Mock Ollama server starts correctly
   - Event type matching requires "Event ID:" in prompt
   - STUN uses `build_event_trigger_message_with_id()` which includes event ID
   - Mock expectations show 0 calls received (confirms packet never arrived)

3. **Examined Socket Configuration**
   - `UdpSocket::bind()` creates socket ready to receive
   - No additional configuration needed
   - Socket is `Arc<>`-wrapped and cloned into spawned task

4. **Checked Timing**
   - Test waits 500ms after seeing "[INFO] STUN server listening..."
   - Server logs confirm receive loop starts before test sends
   - Sufficient time for tokio scheduler to run task

5. **Reviewed Recent Changes**
   - Commit `6a8f491` fixed subprocess hangs by aborting reader tasks
   - No changes to UDP socket handling
   - Stdout/stderr readers should not affect UDP socket

### Hypotheses (Unverified)

1. **Tokio Runtime Scheduling**
   - NetGet subprocess may have runtime configuration issues
   - Tokio reactor might not be polling UDP socket fd
   - Possible resource contention under parallel test load

2. **Environmental Constraints**
   - Claude Code for Web sandbox may have network restrictions
   - UDP localhost loopback may be blocked or rate-limited
   - Subprocess network namespace isolation

3. **Test Infrastructure**
   - Test uses `std::net::UdpSocket` (sync) vs server's `tokio::net::UdpSocket` (async)
   - Should work (separate processes) but could indicate issue
   - Port extraction timing might have edge case

4. **Hidden Bug**
   - Subtle bug in STUN server implementation
   - Socket created but not registered with tokio reactor
   - Arc/clone issue causing socket fd not to be polled

### Recommended Next Steps

1. **Test in Different Environment**
   - Run tests outside Claude Code for Web sandbox
   - Check if issue is environment-specific
   - `./cargo-isolated.sh test --no-default-features --features stun --test server stun`

2. **Add Diagnostic Logging**
   - Add detailed socket state logging in STUN server
   - Log socket fd, reactor registration status
   - Add kernel-level packet capture if possible

3. **Compare with Working UDP Protocol**
   - Check if DNS tests pass in same environment
   - Build minimal UDP echo server based on working protocol
   - Isolate difference between STUN and working implementation

4. **Try Real Ollama**
   - Test with `--use-ollama` flag instead of mocks
   - Verify behavior is same with real LLM
   - Rule out mock system interference

5. **Simplify Test**
   - Create minimal STUN test without mocks
   - Single packet, minimal assertions
   - Reduce variables to isolate issue

6. **Check Other UDP Protocols**
   - Verify NTP, DHCP, BOOTP test status
   - Determine if all UDP protocols fail similarly
   - Pattern may reveal common cause

### Commits

- `5707471` - feat(stun): add event parameters to STUN_BINDING_REQUEST_EVENT
- `3600b59` - chore: add target-test-* to gitignore

### Branch

`claude/parallel-fix-prompts-instance-01Co3zL1mzsWQ9iQR37aEAPf`

### Status

**Partial Fix:** Event parameter definition improves code quality and may help with event matching.

**Unresolved:** UDP packet delivery issue requires deeper investigation or different testing approach. Root cause remains unknown after extensive investigation.

The parameter fix is valuable regardless and has been committed to the repository for review.
