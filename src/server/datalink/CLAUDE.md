# DataLink Protocol Implementation

## Overview
Layer 2 (Data Link) packet capture and injection using libpcap. Allows the LLM to observe and respond to Ethernet frames directly, bypassing IP/TCP/UDP layers. Primary use cases: ARP monitoring, custom layer 2 protocols, network packet analysis, and honeypot operations at the lowest network level.

**Status**: Beta (Layer 2 Protocol)
**Layer**: OSI Layer 2 (Data Link)
**Interface**: Any network interface (eth0, en0, wlan0, etc.)

## Library Choices

### Core Packet Capture
- **pcap v1.1** - Rust bindings to libpcap
  - Low-level packet capture at data link layer
  - Promiscuous mode support (capture all packets on interface)
  - BPF (Berkeley Packet Filter) for packet filtering
  - Blocking API (wrapped in tokio::task::spawn_blocking)

- **libpcap** (system library, required dependency)
  - Industry-standard packet capture library (used by tcpdump, Wireshark)
  - Cross-platform (Linux, macOS, Windows with WinPcap)
  - Requires elevated privileges (root/admin)

- **Manual frame parsing** - No high-level protocol library
  - LLM receives raw Ethernet frame bytes
  - LLM must parse frame structure (destination MAC, source MAC, EtherType, payload)
  - Allows complete flexibility for custom protocols

**Rationale**: libpcap is the de facto standard for packet capture. The pcap crate provides thin Rust bindings. We don't use high-level parsing libraries (like pnet) because the LLM can interpret raw bytes and adapt to any protocol (ARP, custom, etc.).

## Architecture Decisions

### 1. Blocking I/O with Tokio Bridge
pcap has blocking API, Tokio is async:

**Solution**:
- Spawn packet capture in `tokio::task::spawn_blocking()`
- Blocking task runs pcap capture loop
- For each packet, spawn async task via `runtime.spawn()` for LLM processing
- This allows LLM processing to be async while pcap is blocking

**Tradeoff**: Extra task spawning overhead, but necessary for pcap API compatibility.

### 2. No Packet Injection (Yet)
Current implementation is **capture-only**:
- LLM can observe packets (receive events)
- LLM cannot inject packets (no send capability)
- Actions limited to: `show_message`, `ignore_packet`

**Why?**:
1. Packet injection requires root/admin privileges (same as capture)
2. Injection API in pcap crate is less mature
3. Raw socket creation is platform-specific
4. Focus on monitoring/honeypot use cases first

**Future Enhancement**: Add `send_frame` action that:
- Takes hex-encoded Ethernet frame from LLM
- Converts to bytes
- Injects via `pcap::Capture::sendpacket()`
- Allows LLM to respond to ARP, implement custom protocols

### 3. Interface Selection
User specifies network interface in prompt:
- Example: "listen on interface eth0 via datalink"
- Server uses `Device::list()` to find interface by name
- Fails if interface doesn't exist or lacks permissions

**Common Interfaces**:
- Linux: eth0, eth1, wlan0, lo
- macOS: en0, en1, lo0
- Windows: "\Device\NPF_{GUID}"

### 4. Promiscuous Mode
Capture is always in **promiscuous mode**:
- Captures all packets on network segment (not just packets addressed to this host)
- Requires elevated privileges
- Essential for network monitoring and honeypot scenarios

**Security**: Only works on same network segment (same switch/hub). Can't capture packets on other segments without physical access.

### 5. BPF Filtering
Optional packet filtering via Berkeley Packet Filter:
- Example filter: "arp" (only ARP packets)
- Example filter: "tcp port 80" (only HTTP packets)
- Filter applied at kernel level (efficient, no user-space overhead)

**Prompt Format**:
```
listen on interface eth0 via datalink with filter "arp"
```

**Common Filters**:
- `arp` - Only ARP requests/responses
- `icmp` - Only ICMP (ping) packets
- `tcp port 80` - Only HTTP traffic
- `host 192.168.1.1` - Only packets to/from specific IP

### 6. Hex Encoding
Packets always passed to LLM as hex strings:
- Binary data (MAC addresses, EtherType, payload) not human-readable
- Hex format: "00112233445566778899aabbccddeeff..."
- LLM must parse hex to understand packet structure

**Example ARP Request Hex**:
```
ffffffffffff     <- Destination MAC (broadcast)
001122334455     <- Source MAC
0806             <- EtherType (ARP)
0001             <- Hardware type (Ethernet)
0800             <- Protocol type (IPv4)
06               <- Hardware address length
04               <- Protocol address length
0001             <- Operation (ARP request)
...              <- Sender/target MAC/IP addresses
```

### 7. No Connection Concept
DataLink is stateless:
- No connections (unlike TCP)
- No sessions (unlike HTTP)
- Each packet is independent event
- LLM processes each packet separately

**UI Display**: "Connection" in UI is just placeholder (not protocol requirement).

### 8. Dual Logging
All operations use **dual logging**:
- **DEBUG**: Packet summary (length, source interface)
- **TRACE**: Full packet hex dump
- **INFO**: LLM messages and high-level analysis
- **ERROR**: Capture errors, pcap failures, permission issues
- All logs go to both `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model
The LLM responds to DataLink events with actions:

**Events**:
- `datalink_packet_captured` - Ethernet frame captured from interface
  - Parameters: `packet_length`, `packet_hex`

**Available Actions**:
- `show_message` - Display analysis of packet (e.g., "ARP request from 192.168.1.1")
- `ignore_packet` - Don't process packet (no action)
- Common actions: `update_instruction`, etc.

**Future Actions** (not yet implemented):
- `send_frame` - Inject Ethernet frame (hex-encoded)
- `log_packet` - Log packet to file for later analysis

### Example LLM Responses

**ARP Request Analysis**:
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "ARP request: Who has 192.168.1.1? Tell 192.168.1.100"
    }
  ]
}
```

**Ignore Non-ARP**:
```json
{
  "actions": [
    {
      "type": "ignore_packet"
    }
  ]
}
```

**Custom Protocol Detection**:
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Unknown EtherType 0x88B5 - possibly proprietary protocol"
    }
  ]
}
```

## Connection Management

### No Connection State
DataLink has no connection concept:
- Each packet is independent event
- No handshake, no teardown
- No state persistence between packets

### Packet Processing Flow
1. pcap captures packet from interface
2. Packet data copied to `Bytes` buffer
3. Convert to hex string
4. Create `datalink_packet_captured` event
5. Spawn async task to call LLM
6. LLM analyzes packet and returns actions
7. Actions executed (show_message, ignore_packet, etc.)
8. Loop continues for next packet

### Concurrent Packet Processing
- Each packet spawned in separate tokio task
- No queueing (unlike TCP protocols)
- Packets from different sources processed in parallel
- Ollama lock serializes LLM calls but not pcap capture

## Known Limitations

### 1. Requires Root/Admin Privileges
**Why**: Promiscuous mode requires raw socket access

**Workaround**:
- Linux: Run as root or use `sudo setcap cap_net_raw+ep /path/to/netget`
- macOS: Run as root or use `sudo`
- Windows: Run as Administrator

**Test Impact**: E2E tests may fail if not run with privileges.

### 2. No Packet Injection
- LLM can observe packets but not send responses
- Can't implement ARP responder, custom protocols, etc.
- Actions limited to analysis and logging

**Future Enhancement**: Add `send_frame` action (see Architecture Decisions).

### 3. Performance Impact of LLM Processing
- Each packet triggers LLM call (~5s)
- High-traffic networks generate many packets
- pcap capture may drop packets if LLM too slow

**Workaround**: Use BPF filters to reduce packet volume (e.g., "arp" instead of capturing all traffic).

### 4. No Packet Parsing Library
- LLM must parse raw hex (MAC addresses, EtherType, etc.)
- No helper functions for common protocols (ARP, ICMP, etc.)
- LLM may make parsing mistakes

**Rationale**: Keeps implementation flexible - LLM can adapt to any protocol. Adding parsing library would limit to known protocols.

### 5. Platform-Specific Interface Names
- Interface names vary by OS (eth0 vs en0 vs \Device\...)
- User must know correct interface name
- No interface discovery UI

**Workaround**: Use `Device::list()` to list available interfaces (could be exposed as command).

### 6. Capture Loop Blocks Tokio Thread
- pcap capture loop is blocking
- Runs in `spawn_blocking()` which uses separate thread pool
- May exhaust blocking thread pool under high load

**Rationale**: pcap API is inherently blocking - no async alternative without rewriting pcap.

## Example Prompts

### ARP Monitor
```
listen on interface eth0 via datalink with filter "arp"
For each ARP packet, analyze and display:
- Request type (ARP request or reply)
- Source IP and MAC address
- Target IP and MAC address
Log any unusual ARP patterns (ARP spoofing, gratuitous ARP, etc.)
```

### Packet Analyzer
```
listen on interface en0 via datalink
For each packet, identify:
- EtherType (IPv4, IPv6, ARP, custom)
- Source and destination MAC addresses
- If IPv4: source and destination IP
- If ARP: operation and addresses
Display summary for each packet
```

### Layer 2 Honeypot
```
listen on interface eth0 via datalink
Monitor for suspicious activity:
- ARP requests for non-existent IPs (network scanning)
- Unusual EtherTypes (custom protocols)
- Broadcast storms (repeated broadcast packets)
Log all suspicious packets with timestamp and details
```

### Custom Protocol Monitor
```
listen on interface eth0 via datalink with filter "ether proto 0x88B5"
Monitor for custom protocol (EtherType 0x88B5)
Parse payload as:
- Byte 0: Command type
- Bytes 1-4: Sequence number
- Bytes 5+: Data
Display command type and sequence number for each packet
```

## Performance Characteristics

### Latency
- Packet capture: <1ms (kernel-level)
- Hex encoding: <1ms
- LLM processing: 2-5s (typical)
- Total: ~2-5s per packet (LLM dominates)

**Impact**: On high-traffic networks (>1 packet/sec), pcap may drop packets.

**Solution**: Use BPF filters to reduce volume.

### Throughput
- **Low traffic** (<1 pkt/sec): All packets processed
- **Medium traffic** (1-10 pkt/sec): Some packets may queue (async spawning helps)
- **High traffic** (>10 pkt/sec): Many packets dropped (LLM too slow)

**Best Use Cases**: Low-traffic protocols (ARP, custom protocols), not high-traffic (HTTP, streaming).

### Concurrency
- Each packet processed in separate task
- Ollama lock serializes LLM calls
- pcap capture runs in dedicated blocking thread
- No CPU bottleneck (LLM API is bottleneck)

### Memory
- Each packet allocates buffer (~1500 bytes typical, 65535 max)
- Hex encoding allocates string (2× packet size)
- Minimal memory overhead (<10MB total)

## Security Considerations

### Privilege Escalation
- Requires root/admin for promiscuous mode
- Security risk: running untrusted code with elevated privileges
- **Mitigation**: NetGet itself is open-source and auditable

### Privacy
- Promiscuous mode captures ALL packets on network segment
- May capture sensitive data (passwords, credentials if sent in cleartext)
- **Compliance**: May violate privacy laws in some jurisdictions without consent

### Network Impact
- Promiscuous mode doesn't inject traffic (passive observation)
- No impact on network performance (listening only)
- Future injection capability could disrupt network

### Honeypot Usage
DataLink is excellent for honeypots:
- Detect network scanning (ARP sweeps)
- Log attack patterns (custom protocol probes)
- Identify attacker MAC addresses
- Monitor lateral movement within network segment

## Use Cases

### 1. ARP Monitoring
- Detect ARP spoofing attacks
- Monitor ARP cache behavior
- Track IP-to-MAC mappings
- Identify network topology changes

### 2. Custom Protocol Development
- Test custom layer 2 protocols
- Debug protocol implementations
- Monitor protocol behavior
- Analyze protocol efficiency

### 3. Network Forensics
- Capture packets for later analysis
- Identify protocol usage patterns
- Detect anomalies (unusual EtherTypes)
- Track device behavior (MAC address tracking)

### 4. Educational
- Learn packet structure
- Understand Ethernet framing
- Study protocol behavior
- Experiment with BPF filters

### 5. Honeypot Operations
- Detect reconnaissance (ARP scanning)
- Log attack traffic
- Identify attacker devices
- Monitor malicious behavior

## References
- [libpcap Documentation](https://www.tcpdump.org/manpages/pcap.3pcap.html)
- [pcap crate Documentation](https://docs.rs/pcap/latest/pcap/)
- [Berkeley Packet Filter (BPF) Syntax](https://biot.com/capstats/bpf.html)
- [Ethernet Frame Format](https://en.wikipedia.org/wiki/Ethernet_frame)
- [ARP Protocol (RFC 826)](https://datatracker.ietf.org/doc/html/rfc826)
- [EtherType List](https://en.wikipedia.org/wiki/EtherType)
- [Wireshark](https://www.wireshark.org/) - For manual packet analysis and testing
