# BOOTP Protocol Implementation

## Overview
BOOTP (Bootstrap Protocol) server implementing RFC 951 for automatic IP address assignment and boot file configuration. The LLM controls BOOTREQUEST/BOOTREPLY flow using structured actions.

**Status**: Experimental
**RFC**: RFC 951 (BOOTP), RFC 1542 (BOOTP Extensions)
**Port**: 67 (server), 68 (client) - UDP

## Library Choices
- **dhcproto v0.12** - DHCP/BOOTP protocol parsing and construction
  - Used for parsing BOOTP messages (BOOTREQUEST)
  - Used for constructing BOOTP responses (BOOTREPLY)
  - Handles binary BOOTP wire format (same as DHCP base format)
  - Provides Message, Opcode types
  - Feature-gated (only compiled when `bootp` feature enabled)

**Rationale**: BOOTP uses the same packet format as DHCP (DHCP evolved from BOOTP), so dhcproto is the natural choice. The library handles the complex binary protocol structure. BOOTP is simpler than DHCP - it doesn't use DHCP options for subnet mask, DNS, etc. Instead, these are typically configured via other means (TFTP config files, etc.).

## Architecture Decisions

### 1. Action-Based LLM Control
The LLM responds to BOOTP requests with semantic actions:
- `send_bootp_reply` - Respond to BOOTREQUEST with IP assignment and boot file location
- `send_bootp_response` - Send raw hex packet (advanced)
- `ignore_request` - No response

Each action includes network configuration parameters (IP, server IP, boot file, server hostname, gateway).

### 2. Request Context Preservation
BOOTP responses must echo specific fields from the request:
- Transaction ID (xid) - Matches request to response
- Client hardware address (chaddr) - Client's MAC address
- Operation code - BOOTREQUEST → BOOTREPLY

**Implementation**: `BootpRequestContext` stored in Arc<Mutex<Option<...>>>
- Set when request is parsed
- Accessed during action execution
- Contains: xid, chaddr, op, ciaddr, giaddr, sname, file

### 3. Stateless Per-Request Processing
Each BOOTP message is independent:
- No persistent client state
- LLM decides IP assignments on-demand
- No IP address pool management
- Simplified implementation suitable for testing/honeypots/PXE environments

### 4. Boot Configuration Support
BOOTP's primary purpose is network boot configuration:
- **Boot file name** (file field, 128 bytes) - Path to boot image on TFTP server
- **Server hostname** (sname field, 64 bytes) - Name of boot server
- **Server IP** (siaddr field) - IP address of boot/TFTP server
- **Gateway IP** (giaddr field) - For BOOTP relay agents

### 5. Dual Logging
- **DEBUG**: Request summary ("BOOTP BootRequest from MAC 00:11:22:33:44:55")
- **TRACE**: Full hex dump of BOOTP packets (both request and response)
- Both go to netget.log and TUI Status panel

### 6. Connection Tracking
Each BOOTP request creates a "connection" entry:
- Connection ID: Unique per request
- Protocol info: `ProtocolConnectionInfo::Bootp` with recent_requests list
- Tracks request type and timestamp
- Status: Active during processing

## LLM Integration

### Event Type
**`bootp_request`** - Triggered when BOOTP client sends a BOOTREQUEST

Event parameters:
- `op_code` (string) - BootRequest or BootReply
- `client_mac` (string) - Client MAC address (hex)
- `client_ip` (string) - Client IP address (if set, usually 0.0.0.0 initially)

### Available Actions

#### `send_bootp_reply`
Send BOOTP REPLY in response to BOOTREQUEST. Provides IP and boot configuration.

Parameters:
- `assigned_ip` (required) - IP address to assign (e.g., "192.168.1.100")
- `server_ip` (optional) - BOOTP/TFTP server IP (default: "0.0.0.0")
- `boot_file` (optional) - Boot file path (e.g., "boot/pxeboot.n12")
- `server_hostname` (optional) - Server hostname (e.g., "bootserver.local")
- `gateway_ip` (optional) - Gateway/relay IP (default: "0.0.0.0")

#### `send_bootp_response` (Advanced)
Send custom BOOTP response packet as hex string.

#### `ignore_request`
Don't send any response.

### Example LLM Response
```json
{
  "actions": [
    {
      "type": "send_bootp_reply",
      "assigned_ip": "192.168.1.100",
      "server_ip": "192.168.1.1",
      "boot_file": "boot/pxeboot.n12",
      "server_hostname": "bootserver.local"
    },
    {
      "type": "show_message",
      "message": "Assigned 192.168.1.100 to client 00:11:22:33:44:55"
    }
  ]
}
```

## Connection Management

### Connection Lifecycle
1. **Request Received**: UDP datagram on port 67
2. **Parse**: Extract operation code, MAC address, existing IP using dhcproto
3. **Context**: Store BootpRequestContext in protocol instance
4. **Register**: Create ConnectionId and add to ServerInstance
5. **Process**: Call LLM with `bootp_request` event
6. **Execute**: Build BOOTP response using dhcproto and context
7. **Respond**: Send UDP response to client
8. **Update**: Track bytes/packets sent/received
9. **Persist**: Connection remains in UI to show activity

### BOOTP Flow
Standard BOOTP follows simple 2-message exchange:
1. **BOOTREQUEST** (client broadcast on port 68 → server port 67) - Client requests IP and boot info
2. **BOOTREPLY** (server port 67 → client port 68) - Server provides IP and boot file location

NetGet implements the server side (listens for BOOTREQUEST, sends BOOTREPLY).

## Differences from DHCP

### What BOOTP Has:
- Basic IP assignment (yiaddr field)
- Boot file name (file field, 128 bytes)
- Server name (sname field, 64 bytes)
- Server IP (siaddr field)
- Gateway/relay support (giaddr field)

### What BOOTP Lacks (compared to DHCP):
- No DHCP options (subnet mask, DNS, lease time, etc.)
- No DISCOVER/OFFER/REQUEST/ACK state machine (just REQUEST/REPLY)
- No dynamic lease management
- No address renewal or release
- No vendor-specific options

**Migration Note**: DHCP extended BOOTP by adding the options field. A BOOTP packet is valid DHCP, but DHCP adds rich configuration via options.

## Known Limitations

### 1. No Subnet/DNS Configuration
- BOOTP doesn't include subnet mask, DNS servers, or lease time
- These are typically configured via TFTP config files after boot
- Workaround: Use DHCP instead for full network configuration

### 2. No Relay Agent State
- Basic relay support via giaddr field
- No advanced relay agent options
- Assumes simple network topology

### 3. No IP Pool Management
- No persistent IP assignment tracking
- LLM decides IPs on-demand (could assign duplicates)
- No conflict detection

**Use Case**: Testing, honeypots, PXE boot environments, demonstrations. Not suitable for production BOOTP server.

### 4. No Boot File Validation
- Server doesn't verify boot file exists
- No TFTP integration (separate protocol)
- LLM specifies file path, but file must exist on TFTP server

### 5. Feature-Gated
- BOOTP parsing requires `bootp` feature flag
- Without feature: Server accepts packets but can't parse them
- Action execution returns error if feature not enabled

## Example Prompts

### PXE Boot Server
```
listen on port 67 via bootp
When receiving BOOTREQUEST, assign IP addresses from 192.168.1.100 onwards
Respond with:
  - Server IP: 192.168.1.1
  - Boot file: boot/pxeboot.n12
  - Server hostname: pxeserver.local
```

### Static IP Assignment by MAC
```
listen on port 67 via bootp
For MAC 00:11:22:33:44:55, always assign 192.168.1.50 with boot file "linux/vmlinuz"
For MAC 00:11:22:33:44:66, always assign 192.168.1.51 with boot file "windows/bootmgr.efi"
For other clients, assign from 192.168.1.100 onwards with default boot file "boot/default.pxe"
```

### Network Boot with Gateway
```
listen on port 67 via bootp
Assign IPs in range 10.0.0.100 to 10.0.0.200
Use server IP 10.0.0.1, gateway 10.0.0.1
Boot file: tftp/netboot.img
Server hostname: netboot.example.com
```

### Honeypot Mode
```
listen on port 67 via bootp
Accept all BOOTREQUEST messages but respond with fake boot configurations
Assign random IPs in 10.255.0.0/16 range
Use boot file "honeypot/trap.bin" and server IP 10.255.255.1
Log all MAC addresses for analysis
```

## Performance Characteristics

### Latency
- **With Scripting**: Sub-millisecond (script handles requests)
- **Without Scripting**: 2-5 seconds (one LLM call per request)
- dhcproto parsing: ~50-100 microseconds
- dhcproto encoding: ~50-100 microseconds

### Throughput
- **With Scripting**: Thousands of requests per second
- **Without Scripting**: Limited by LLM (~0.2-0.5 requests/sec)
- BOOTP traffic is typically low volume (diskless clients boot once)

### Scripting Compatibility
BOOTP is excellent candidate for scripting:
- Simple request/response pattern
- Deterministic IP assignment logic
- PXE environments often have static MAC-to-IP mappings

When scripting enabled:
- Server startup generates script (1 LLM call)
- All requests handled by script (0 LLM calls)
- Script can implement MAC-based static assignments, IP pools, etc.

## Use Cases

### 1. PXE Boot Environment
BOOTP is commonly used with PXE (Preboot Execution Environment):
- Client broadcasts BOOTREQUEST
- BOOTP server responds with IP and boot file name
- Client downloads boot file via TFTP
- Client boots network OS

### 2. Diskless Workstations
Historical use case (1980s-1990s):
- Workstations with no local storage
- Boot OS entirely from network
- BOOTP provides initial network configuration

### 3. Network Boot Testing
Modern use case:
- Testing PXE boot configurations
- Validating boot file paths
- Simulating boot servers for QA

### 4. Honeypots
Security research:
- Attract network boot scans
- Log unauthorized boot attempts
- Analyze attacker boot file requests

## References
- [RFC 951: Bootstrap Protocol (BOOTP)](https://datatracker.ietf.org/doc/html/rfc951)
- [RFC 1542: Clarifications and Extensions for BOOTP](https://datatracker.ietf.org/doc/html/rfc1542)
- [dhcproto Documentation](https://docs.rs/dhcproto/latest/dhcproto/)
- [BOOTP vs DHCP Comparison](https://www.ietf.org/rfc/rfc2131.txt)
- [PXE Specification (Intel)](https://www.intel.com/content/www/us/en/architecture-and-technology/preboot-execution-environment.html)
