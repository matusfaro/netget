# DataLink Client Implementation

## Overview

The DataLink client provides LLM-controlled raw Ethernet frame injection and capture at Layer 2 (Data Link layer). This enables custom protocol testing, ARP spoofing detection, network monitoring, and Ethernet frame analysis.

## Architecture

### Library Choice

**Primary Library:** `pcap` crate (v2.2)
- **Rationale:** Industry-standard libpcap wrapper for packet capture/injection
- **Capabilities:**
  - Raw frame injection via `sendpacket()`
  - Promiscuous mode frame capture
  - BPF (Berkeley Packet Filter) filtering
  - Cross-platform (Linux, macOS, Windows)
- **Limitations:**
  - Requires root/CAP_NET_RAW privileges
  - Blocking I/O (requires `spawn_blocking`)
  - Platform-specific behavior differences

### Connection Model

Unlike TCP/UDP clients, the DataLink client:
1. **Interface-based**: Opens a network interface (e.g., `eth0`, `en0`) instead of connecting to a remote address
2. **Bidirectional**: Can both inject frames and capture frames in promiscuous mode
3. **Blocking Operations**: pcap is blocking, so we use `tokio::spawn_blocking` for the capture/injection loop
4. **Channel-based Injection**: Uses `mpsc::unbounded_channel` to send injection commands from async context to blocking pcap thread

### State Management

- **ConnectionState**: Tracks LLM processing state (Idle/Processing/Accumulating)
- **ClientData**: Holds queued frames and LLM memory
- **InjectionCommand**: Channel message for frame injection requests

### Dual Mode Operation

1. **Injection-only Mode** (`promiscuous: false`):
   - LLM can inject frames via `inject_frame` action
   - No frame capture
   - Suitable for ARP spoofing tests, custom protocol injection

2. **Capture + Injection Mode** (`promiscuous: true`):
   - Captures all frames on interface
   - LLM analyzes captured frames
   - LLM can inject response frames
   - Requires root/CAP_NET_RAW for promiscuous mode

## LLM Integration

### Events

1. **datalink_frame_injected**: Triggered when frame is successfully injected
   - Parameters: `frame_length` (number)

2. **datalink_frame_captured**: Triggered when frame is captured (promiscuous mode only)
   - Parameters: `frame_hex` (hex string), `frame_length` (number)

### Actions

#### Async Actions (User-triggered)

1. **inject_frame**: Inject raw Ethernet frame
   ```json
   {
     "type": "inject_frame",
     "frame_hex": "ffffffffffff001122334455080600010800060400010011223344550a0000010000000000000a000002"
   }
   ```

2. **disconnect**: Close the DataLink client and release interface
   ```json
   {
     "type": "disconnect"
   }
   ```

#### Sync Actions (Response to captured frames)

1. **inject_frame**: Inject frame in response to captured frame
   - Same parameters as async version

2. **wait_for_more**: Wait for more frames before responding
   ```json
   {
     "type": "wait_for_more"
   }
   ```

### Frame Format

The LLM must construct complete Ethernet frames including:
- **Destination MAC** (6 bytes): Target MAC address (or broadcast `ffffffffffff`)
- **Source MAC** (6 bytes): Sender MAC address
- **EtherType** (2 bytes): Protocol type (e.g., `0x0806` for ARP, `0x0800` for IPv4)
- **Payload**: Protocol-specific data
- **FCS** (4 bytes): Frame Check Sequence (optional, often added by hardware)

Example ARP request frame:
```
ff ff ff ff ff ff  // Destination MAC (broadcast)
00 11 22 33 44 55  // Source MAC
08 06              // EtherType (ARP)
00 01              // Hardware type (Ethernet)
08 00              // Protocol type (IPv4)
06                 // Hardware address length
04                 // Protocol address length
00 01              // Operation (ARP request)
00 11 22 33 44 55  // Sender MAC
0a 00 00 01        // Sender IP (10.0.0.1)
00 00 00 00 00 00  // Target MAC (unknown)
0a 00 00 02        // Target IP (10.0.0.2)
```

## Use Cases

1. **ARP Testing**: Inject ARP requests/replies for network discovery
2. **Custom L2 Protocols**: Test proprietary Ethernet protocols
3. **Network Monitoring**: Capture and analyze Ethernet traffic
4. **ARP Spoofing Detection**: Monitor for duplicate ARP replies
5. **MAC Address Analysis**: Track MAC address usage on network
6. **Frame Timing Analysis**: Measure frame arrival times

## Limitations

1. **Privilege Requirement**: Requires root or CAP_NET_RAW capability
2. **Blocking I/O**: pcap is blocking, so operations run in `spawn_blocking`
3. **No Automatic FCS**: Frame Check Sequence often handled by NIC hardware
4. **Platform Differences**: libpcap behavior varies across OS
5. **No TCP/UDP**: This is Layer 2 only - for Layer 3+ protocols, use TCP/UDP/IP clients
6. **Performance**: Frame capture can be high-volume, may overwhelm LLM
7. **Dummy SocketAddr**: Returns `127.0.0.1:0` since DataLink doesn't use sockets

## Startup Parameters

- **interface** (required, string): Network interface name (e.g., `eth0`, `en0`, `wlan0`)
- **promiscuous** (optional, boolean): Enable promiscuous mode for frame capture (default: `false`)

Example:
```json
{
  "interface": "eth0",
  "promiscuous": true
}
```

## Testing Strategy

See `tests/client/datalink/CLAUDE.md` for E2E testing approach.

## Security Considerations

**CRITICAL**: Raw frame injection can disrupt networks and violate regulations. Only use on authorized test networks:
- ✅ Isolated lab environments
- ✅ Virtual networks (VMs, containers)
- ✅ Personal test setups
- ❌ Production networks
- ❌ Public networks
- ❌ Networks you don't own

ARP spoofing and MAC address manipulation can be malicious - use responsibly.

## Implementation Notes

1. **Channel-based Injection**: The async LLM code sends injection commands via `mpsc::unbounded_channel` to the blocking pcap thread
2. **State Machine**: Prevents concurrent LLM calls (Idle → Processing → Accumulating)
3. **Dual Logging**: Uses both `tracing` macros and `status_tx` for TUI updates
4. **Error Handling**: Injection failures are logged but don't crash the client
5. **Graceful Shutdown**: Client tracks disconnection via `ClientStatus::Disconnected`

## Future Enhancements

- BPF filtering support (filter captured frames)
- Multiple interface support
- Frame statistics (capture rate, injection rate)
- Frame timing control (scheduled injection)
- VLAN tag support
- Jumbo frame support
