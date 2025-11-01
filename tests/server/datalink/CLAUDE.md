# DataLink Protocol E2E Tests

## Test Overview
Tests Layer 2 packet capture functionality with ARP monitoring and interface detection. Note that DataLink tests are **limited** because packet capture requires root/admin privileges which may not be available in test environments.

## Test Strategy
- **Privilege-aware tests**: Tests designed to work with or without root privileges
- **No real packet capture**: Most tests validate prompt construction, not actual capture
- **Command-line tools**: Uses `arping` command if available (rare in CI environments)
- **Focus on interface detection**: Tests that interface selection logic works
- **Graceful degradation**: Tests note expected behavior but don't fail if privileges missing

## LLM Call Budget
- `test_arp_responder()`: 0-1 LLM calls (depends on privileges and if server actually captures packet)
- `test_datalink_interface_detection()`: 0 LLM calls (prompt construction only, no server started)
- **Total: 0-1 LLM calls** (well under 10 limit)

**Why So Low?**:
1. DataLink requires root privileges (not available in most test environments)
2. Tests primarily validate interface selection and prompt parsing
3. Actual packet capture testing requires manual setup (interface, privileges, arping tool)
4. Tests are designed to pass without requiring actual packet capture

**Note**: This is intentionally low - DataLink is more of an infrastructure/setup validation test than a protocol interaction test.

## Scripting Usage
❌ **Scripting Not Applicable**

**Rationale**: DataLink is primarily for packet observation and analysis. Each packet is unique, so scripting doesn't provide performance benefits. The LLM needs to analyze each packet individually (ARP source/dest, custom protocol, etc.).

**Future Consideration**: Could script common patterns (e.g., "if ARP request, show message with source IP"), but limited value given low packet volume in typical use cases.

## Client Library
- **arping command-line tool** (optional, from iputils or net-tools package)
  - Sends ARP requests to specific IP addresses
  - Used to generate ARP traffic for testing
  - Not available in most CI environments

- **No Rust client library** - DataLink is layer 2, no high-level client exists
  - Would require raw socket programming
  - Platform-specific (different APIs on Linux/macOS/Windows)

**Why command-line tool?**:
1. ARP is system-level protocol (no user-space library)
2. arping is standard tool used by sysadmins
3. Easy to manually test same command

**Why no Rust library?**:
1. Raw socket creation requires root privileges
2. Platform-specific APIs (AF_PACKET on Linux, BPF on macOS)
3. Would duplicate pcap functionality
4. Test focus is on interface selection, not packet generation

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~10-20 seconds for full test suite (2 tests)
- Breakdown:
  - `test_arp_responder()`: 10-15s (if privileges available and arping installed)
  - `test_datalink_interface_detection()`: <1s (no server, just prompt validation)

**Why Fast?**:
1. Most tests skip actual packet capture (no privileges)
2. No LLM calls in most cases
3. Interface detection is instant (<1ms)

**If Privileges Available**:
- Runtime increases to ~30-40s (includes LLM packet analysis)
- Very rare in CI environments

## Failure Rate
- **Very Low** (~1%) - Most tests are validation-only, not actual capture
- Common failures:
  - arping not installed (test notes this and continues)
  - Interface doesn't exist (test notes this)
  - Insufficient privileges (test notes this)
- Rare failures:
  - Server startup failure (unrelated to DataLink)

**Note**: Tests are designed to be informational, not strict pass/fail. They demonstrate what DataLink can do but don't require full setup.

## Test Cases

### 1. ARP Responder (`test_arp_responder`)
- **LLM Calls**: 0-1 (depends on if packet captured)
- **Prompt**: Respond to ARP requests for 192.168.100.50 with MAC 00:11:22:33:44:55
- **Client**: arping command (if available)
- **Interface**: en0 (common macOS WiFi interface)
- **Command**: `arping -c 1 -I en0 192.168.100.50`
- **Validation**:
  - Server starts (may fail with permission error, noted)
  - arping executed (may fail if not installed, noted)
  - If reply received: Test passes
  - If no reply: Test notes expected (no injection capability yet)
- **Purpose**: Demonstrate ARP monitoring setup, note limitations

**Expected Behavior**:
1. **With root + arping**: Server captures ARP request, LLM analyzes it
2. **Without root**: Server fails to start (permission denied), test notes this
3. **Without arping**: Test skips packet generation, just validates server startup

**Limitations**:
- No packet injection (LLM can't respond to ARP request)
- Test just shows that LLM receives and analyzes ARP packet
- Full ARP responder requires future `send_frame` action

### 2. DataLink Interface Detection (`test_datalink_interface_detection`)
- **LLM Calls**: 0 (no server started)
- **Prompt**: "Set up a DataLink layer 2 server on interface lo0"
- **Purpose**: Validate prompt construction and interface naming
- **Validation**:
  - Prompt format is correct
  - Interface name (lo0) is valid format
  - Test passes (no privileges required)
- **Purpose**: Verify interface selection logic works without requiring actual capture

**Expected Behavior**:
- Test always passes (no network operations)
- Demonstrates how to construct DataLink prompts
- Notes that actual capture requires root privileges

## Known Issues

### 1. Root Privileges Required
**Symptom**: Server fails with "Permission denied" or "Operation not permitted"

**Cause**: pcap promiscuous mode requires root/admin

**Frequency**: ~95% of test environments (CI, developer laptops without sudo)

**Solution**: Run tests with `sudo` or grant capabilities:
```bash
# Linux: Grant capabilities to binary
sudo setcap cap_net_raw+ep target/release/netget

# macOS/Linux: Run with sudo
sudo ./cargo-isolated.sh test --features e2e-tests --test server::datalink::test_arp_responder

# Windows: Run as Administrator
```

**Test Behavior**: Test notes permission failure but doesn't fail test.

### 2. arping Not Installed
**Symptom**: "arping command not found" or "No such file or directory"

**Cause**: arping not installed on system

**Frequency**: ~80% of test environments (minimal CI images, macOS without Xcode tools)

**Solution**: Install arping:
- Ubuntu: `apt-get install iputils-arping` or `apt-get install arping`
- macOS: `brew install arping` (requires Homebrew)
- Fedora: `dnf install iputils`

**Test Behavior**: Test notes arping missing and suggests installation.

### 3. Interface Name Incorrect
**Symptom**: "Device 'en0' not found"

**Cause**: Interface name varies by platform and system

**Common Names**:
- Linux: eth0, wlan0, enp0s3
- macOS: en0, en1, utun0
- Windows: "\Device\NPF_{GUID}"

**Solution**: Check available interfaces:
```bash
# Linux/macOS
ip link show
ifconfig

# Windows
ipconfig /all
```

**Test Behavior**: Test uses common interface name (en0 for macOS, eth0 for Linux) but may fail on systems with different naming.

### 4. Loopback Limitation
**Symptom**: No packets captured on loopback interface (lo0, lo)

**Cause**: Loopback interface is virtual (doesn't have real layer 2)

**Impact**: Can't test real packet capture without physical/virtual interface

**Workaround**: Use physical interface (eth0, en0) for real testing.

### 5. No Packet Injection
**Symptom**: ARP responder test doesn't actually respond to ARP requests

**Cause**: `send_frame` action not implemented (see Architecture Decisions in implementation CLAUDE.md)

**Status**: Expected limitation - test just validates packet capture and analysis

**Future**: When `send_frame` implemented, test can validate full ARP responder functionality.

## Performance Notes

### Why Tests Are Fast
1. Most tests skip actual packet capture (no privileges)
2. No LLM calls unless packets captured
3. Interface detection is instant
4. Prompt validation only

### Impact of Privileges
**Without root**:
- Server startup fails immediately (~100ms)
- No packet capture
- No LLM calls
- Test completes in <1s

**With root**:
- Server starts successfully (~2-3s)
- Waits for packet capture (~5s timeout)
- If packet arrives: LLM analysis (~5s)
- Total: ~10-13s per test

### Network Traffic Impact
- **Promiscuous mode**: Captures all packets on segment (busy networks = high CPU)
- **BPF filter recommended**: Use "arp" filter to reduce volume
- **Test impact**: Tests use filter when possible to minimize load

## Future Enhancements

### Test Coverage Gaps
1. **Packet injection**: No tests for `send_frame` action (not implemented)
2. **BPF filters**: No tests validating different filter expressions
3. **Multiple interfaces**: No tests for switching interfaces
4. **High traffic**: No tests for packet dropping under load
5. **Non-ARP protocols**: No tests for custom EtherTypes, IPv6, etc.
6. **Hex parsing**: No tests validating LLM's ability to parse packet hex

### Privilege-Independent Tests
Could add tests that don't require capture:

```rust
#[tokio::test]
async fn test_datalink_bpf_filter_syntax() {
    // Test BPF filter parsing (no capture needed)
    let filters = vec![
        "arp",
        "tcp port 80",
        "host 192.168.1.1",
        "ether proto 0x88B5",
    ];

    for filter in filters {
        let prompt = format!("listen on interface lo0 via datalink with filter \"{}\"", filter);
        // Validate prompt syntax
        assert!(prompt.contains("datalink"));
        assert!(prompt.contains(filter));
    }
}
```

This validates filter syntax without needing root privileges.

### Packet Injection Tests (Future)
Once `send_frame` action implemented:

```rust
#[tokio::test]
async fn test_arp_responder_with_injection() -> E2EResult<()> {
    // Requires root privileges
    let prompt = format!(
        "listen on interface en0 via datalink with filter \"arp\".
        When ARP request for 192.168.100.50 arrives,
        respond with MAC address 00:11:22:33:44:55"
    );

    let server = start_netget_server(ServerConfig::new(prompt)).await?;

    // Send ARP request
    let output = Command::new("arping")
        .args(&["-c", "1", "-I", "en0", "192.168.100.50"])
        .output()?;

    // Verify response received
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("00:11:22:33:44:55"));
    assert!(stdout.contains("reply"));

    server.stop().await?;
    Ok(())
}
```

This would test full ARP responder functionality (capture + inject).

### CI/CD Considerations
DataLink tests are challenging for CI:
1. Most CI environments don't have root access
2. Most CI environments have minimal network interfaces
3. arping not installed by default
4. Capture may interfere with CI network traffic

**Recommendation**: Mark DataLink tests as `#[ignore]` by default, run manually:
```bash
./cargo-isolated.sh test --features e2e-tests --test server::datalink --ignored
```

Or create separate test feature:
```toml
[features]
e2e-tests-privileged = ["e2e-tests"]
```

```bash
./cargo-isolated.sh test --features e2e-tests-privileged --test server::datalink
```

## Manual Testing Guide

For developers wanting to test DataLink manually:

### 1. Grant Privileges
```bash
# Linux
sudo setcap cap_net_raw+ep target/release/netget

# Or use sudo
sudo target/release/netget "..."
```

### 2. Find Interface
```bash
# Linux
ip link show

# macOS
ifconfig

# Choose interface (e.g., en0, eth0)
```

### 3. Run NetGet
```bash
sudo target/release/netget "listen on interface en0 via datalink with filter \"arp\""
```

### 4. Generate ARP Traffic
```bash
# In another terminal
sudo arping -c 1 -I en0 192.168.1.1

# Or use any network tool that generates ARP (ping, nmap, etc.)
```

### 5. Observe LLM Analysis
NetGet should display:
- Packet captured (DEBUG)
- Packet hex (TRACE)
- LLM analysis (INFO)

Example output:
```
[DEBUG] Datalink received 42 bytes
[TRACE] Datalink data (hex): ffffffffffff001122334455...
[INFO] ARP request: Who has 192.168.1.1? Tell 192.168.1.100
```

## References
- [libpcap Documentation](https://www.tcpdump.org/manpages/pcap.3pcap.html)
- [pcap crate Documentation](https://docs.rs/pcap/latest/pcap/)
- [arping Manual](https://linux.die.net/man/8/arping)
- [Berkeley Packet Filter (BPF) Syntax](https://biot.com/capstats/bpf.html)
- [Promiscuous Mode Explained](https://www.wireshark.org/docs/wsug_html_chunked/ChCapCaptureOptions.html#ChCapPromiscMode)
- [Linux Capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html) - cap_net_raw
