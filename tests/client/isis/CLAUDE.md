# IS-IS Client Test Documentation

## Test Strategy

The IS-IS client tests are **primarily manual** due to environmental requirements:

1. **Root access required** - pcap needs CAP_NET_RAW capability
2. **IS-IS router required** - Real IS-IS traffic on the network
3. **Platform-specific** - Network interfaces vary by system

## Test Approach

### Unit Tests (No LLM)

**test_isis_device_listing**:

- Lists available network interfaces
- No root required
- No LLM calls
- Verifies pcap device enumeration works

**test_isis_pdu_parsing**:

- Tests basic PDU header parsing
- No network access required
- No LLM calls
- Validates IS-IS discriminator detection

**Budget**: 0 LLM calls
**Runtime**: < 1 second

### Integration Tests (With LLM)

**test_isis_client_requires_root**:

- Checks for root privileges
- Checks for Ollama availability
- Creates client instance
- No actual capture (requires IS-IS traffic)
- **Marked with `#[ignore]`** - requires root

**Budget**: 0 LLM calls (setup only)
**Runtime**: < 1 second

### Manual E2E Test

**test_isis_capture_with_llm**:

- **Requires**:
    - Root access (sudo)
    - IS-IS router on network OR packet replay
    - Ollama running
    - Correct interface name set in test
- **Captures IS-IS PDUs for 30 seconds**
- **LLM analyzes topology**
- **Marked with `#[ignore]`** - manual test only

**Budget**: Variable (depends on PDU rate, typically 1-10 LLM calls)
**Runtime**: 30+ seconds

## Running Tests

### Unit Tests (No Root)

```bash
./cargo-isolated.sh test --no-default-features --features isis --test client::isis::e2e_test test_isis_device_listing
./cargo-isolated.sh test --no-default-features --features isis --test client::isis::e2e_test test_isis_pdu_parsing
```

### Manual E2E Test (Requires Root)

```bash
# Edit test file first to set correct interface name
sudo -E ./cargo-isolated.sh test --no-default-features --features isis --test client::isis::e2e_test test_isis_capture_with_llm -- --ignored --nocapture
```

**Important**: Use `sudo -E` to preserve environment variables (CARGO_TARGET_DIR, etc.)

## Test Environment Setup

### Option 1: Real IS-IS Router

Use FRRouting (FRR) or similar routing software:

```bash
# Install FRR
sudo apt-get install frr

# Enable IS-IS daemon
sudo vim /etc/frr/daemons
# Set isisd=yes

# Configure IS-IS
sudo vim /etc/frr/isisd.conf
```

Sample IS-IS config:

```
router isis MYNET
 net 49.0001.1921.6800.1001.00
 is-type level-2-only

interface eth0
 ip router isis MYNET
 isis circuit-type level-2-only
 isis hello-interval 10
 isis hello-multiplier 3
```

### Option 2: Packet Replay

Capture IS-IS traffic with Wireshark/tcpdump, then replay:

```bash
# Capture IS-IS traffic
sudo tcpdump -i eth0 -w isis-capture.pcap 'ether proto 0xfefe or ether[14:2] = 0xfefe'

# Replay captured traffic
sudo tcpreplay -i eth0 isis-capture.pcap
```

### Option 3: Virtual Network

Use network namespaces and virtual interfaces:

```bash
# Create network namespace
sudo ip netns add isis-test

# Create veth pair
sudo ip link add veth0 type veth peer name veth1

# Move one end to namespace
sudo ip link set veth1 netns isis-test

# Bring up interfaces
sudo ip link set veth0 up
sudo ip netns exec isis-test ip link set veth1 up

# Run IS-IS router in namespace
sudo ip netns exec isis-test frr isisd -d
```

Then capture on veth0.

## LLM Call Budget

**Total Budget**: < 10 LLM calls per test run

### Breakdown:

- Device listing: 0 calls
- PDU parsing: 0 calls
- Client setup: 0 calls
- **PDU capture**: 1 call per PDU (rate-limited by capture)
    - Typical: 1-5 Hello PDUs/minute per router
    - Budget for 30 second capture: ~1-3 calls

### Why So Few Calls?

IS-IS PDUs are infrequent:

- **Hello PDUs**: Sent every 10 seconds by default
- **LSPs**: Sent on topology changes
- **CSNPs/PSNPs**: Synchronization messages

In a stable network with 1-2 routers, expect only 1-3 PDUs in 30 seconds.

## Expected Runtime

**Unit tests**: < 1 second
**Manual E2E test**: 30-60 seconds (mostly waiting for PDUs)
**Total**: ~1 minute

## Known Issues

### Platform-Specific

1. **Interface names vary**:
    - Linux: eth0, wlan0, ens33
    - macOS: en0, en1
    - Windows: Not directly supported (WinPcap required)

2. **Pcap permissions**:
    - Linux: Requires root or CAP_NET_RAW
    - macOS: Requires root or /dev/bpf permissions
    - May need to adjust `/dev/bpf*` permissions on macOS

3. **Filter syntax**:
    - BPF filter may vary slightly between platforms
    - Current filter: `ether proto 0xfefe or ether[14:2] = 0xfefe`

### Test Reliability

1. **No IS-IS traffic**: Test will timeout with no PDUs captured
2. **Wrong interface**: Must specify correct interface name
3. **Permissions**: Root required, sudo -E preserves cargo env
4. **Ollama dependency**: Requires Ollama running locally

## Test Validation

### Success Criteria

1. **Device listing**: Returns list of interfaces (may be empty on some systems)
2. **PDU parsing**: Correctly identifies IS-IS discriminator (0x83)
3. **Manual E2E**:
    - Captures at least 1 IS-IS PDU
    - LLM parses PDU type
    - No errors in pcap or LLM processing

### Expected Output

```
Starting IS-IS capture on interface: eth0
[CLIENT] ISIS client 1 capturing on eth0
ISIS client 1 captured PDU: type=L2 LAN Hello, version=1, length=27
[LLM] Captured IS-IS L2 LAN Hello PDU from neighbor
```

## Future Test Enhancements

1. **Mock IS-IS Traffic**: Generate synthetic IS-IS PDUs for testing
2. **Automated Router Setup**: Docker container with FRR for CI
3. **Integration Test**: Capture + parse + topology analysis
4. **Performance Test**: High PDU rate handling
5. **Multi-Router Test**: Capture from network with multiple routers

## Debugging

### No PDUs Captured

```bash
# Check if IS-IS traffic exists
sudo tcpdump -i eth0 -c 10 'ether proto 0xfefe or ether[14:2] = 0xfefe'

# Check interface is up
ip link show eth0

# Check IS-IS router is running
sudo systemctl status frr
```

### Permission Errors

```bash
# Check capabilities
getcap /usr/bin/dumpcap

# Run with sudo
sudo -E cargo test ...

# Or grant CAP_NET_RAW
sudo setcap cap_net_raw+ep target/debug/netget
```

### LLM Errors

```bash
# Check Ollama is running
curl http://localhost:11434/api/tags

# Check model is available
ollama list | grep qwen3-coder
```

## References

- IS-IS Protocol: ISO/IEC 10589
- FRRouting: https://frrouting.org/
- tcpreplay: https://tcpreplay.appneta.com/
- libpcap: https://www.tcpdump.org/
