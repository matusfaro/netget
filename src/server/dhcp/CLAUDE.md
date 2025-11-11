# DHCP Protocol Implementation

## Overview

DHCP (Dynamic Host Configuration Protocol) server implementing RFC 2131 for automatic IP address assignment and network
configuration. The LLM controls DHCP DISCOVER/OFFER/REQUEST/ACK flow using structured actions.

**Status**: Beta (Core Protocol)
**RFC**: RFC 2131 (DHCP), RFC 2132 (DHCP Options)
**Port**: 67 (server), 68 (client) - UDP

## Library Choices

- **dhcproto v0.11** - DHCP protocol parsing and construction
    - Used for parsing DHCP messages (DISCOVER, REQUEST, etc.)
    - Used for constructing DHCP responses (OFFER, ACK, NAK)
    - Handles binary DHCP wire format
    - Provides Message, Opcode, MessageType, DhcpOption types
    - Feature-gated (only compiled when `dhcp` feature enabled)

**Rationale**: dhcproto is the standard Rust library for DHCP. It handles the complex binary protocol (based on BOOTP)
and provides type-safe DHCP option handling. The LLM doesn't need to understand DHCP packet structure - it just provides
semantic data (IP, subnet mask, DNS servers) and the library handles encoding.

## Architecture Decisions

### 1. Action-Based LLM Control

The LLM responds to DHCP requests with semantic actions:

- `send_dhcp_offer` - Respond to DISCOVER with IP offer
- `send_dhcp_ack` - Respond to REQUEST with IP assignment confirmation
- `send_dhcp_nak` - Reject REQUEST (invalid configuration)
- `send_dhcp_response` - Send raw hex packet (advanced)
- `ignore_request` - No response

Each action includes network configuration parameters (IP, subnet mask, router, DNS servers, lease time).

### 2. Request Context Preservation

DHCP responses must echo specific fields from the request:

- Transaction ID (xid) - Matches request to response
- Client hardware address (chaddr) - Client's MAC address
- Message type - DISCOVER → OFFER, REQUEST → ACK

**Implementation**: `DhcpRequestContext` stored in Arc<Mutex<Option<...>>>

- Set when request is parsed
- Accessed during action execution
- Contains: xid, chaddr, message_type, ciaddr, requested_ip

### 3. Stateless Per-Request Processing

Each DHCP message is independent:

- No persistent client state (no lease database)
- LLM decides IP assignments on-demand
- No IP address pool management
- Simplified implementation suitable for testing/honeypots

### 4. DHCP Options Support

Actions support common DHCP options:

- Subnet Mask (option 1)
- Router/Gateway (option 3)
- DNS Servers (option 6)
- Lease Time (option 51)
- Server Identifier (option 54)

Additional options can be added via raw `send_dhcp_response` action.

### 5. Dual Logging

- **DEBUG**: Request summary ("DHCP DISCOVER from MAC 00:11:22:33:44:55")
- **TRACE**: Full hex dump of DHCP packets (both request and response)
- Both go to netget.log and TUI Status panel

### 6. Connection Tracking

Each DHCP request creates a "connection" entry:

- Connection ID: Unique per request
- Protocol info: `ProtocolConnectionInfo::Dhcp` with recent_requests list
- Tracks message type and timestamp
- Status: Active during processing

## LLM Integration

### Event Type

**`dhcp_request`** - Triggered when DHCP client sends a message

Event parameters:

- `message_type` (string) - DISCOVER, REQUEST, INFORM, RELEASE, etc.
- `client_mac` (string) - Client MAC address (hex)
- `requested_ip` (string, optional) - IP address client wants (if any)

### Available Actions

#### `send_dhcp_offer`

Send DHCP OFFER in response to DISCOVER. Proposes IP configuration.

Parameters:

- `offered_ip` (required) - IP address to offer (e.g., "192.168.1.100")
- `server_ip` (optional) - DHCP server IP (default: "0.0.0.0")
- `subnet_mask` (optional) - Subnet mask (e.g., "255.255.255.0")
- `router` (optional) - Default gateway (e.g., "192.168.1.1")
- `dns_servers` (optional) - Array of DNS IPs (e.g., ["8.8.8.8", "8.8.4.4"])
- `lease_time` (optional) - Lease duration in seconds (default: 86400 = 24 hours)

#### `send_dhcp_ack`

Send DHCP ACK in response to REQUEST. Confirms IP assignment.

Parameters: Same as `send_dhcp_offer`, but uses `assigned_ip` instead of `offered_ip`.

#### `send_dhcp_nak`

Send DHCP NAK to reject a REQUEST.

Parameters:

- `server_ip` (optional)
- `message` (optional) - Error message to include

#### `send_dhcp_response` (Advanced)

Send custom DHCP response packet as hex string.

#### `ignore_request`

Don't send any response.

### Example LLM Response

```json
{
  "actions": [
    {
      "type": "send_dhcp_offer",
      "offered_ip": "192.168.1.100",
      "subnet_mask": "255.255.255.0",
      "router": "192.168.1.1",
      "dns_servers": ["8.8.8.8", "8.8.4.4"],
      "lease_time": 86400
    },
    {
      "type": "show_message",
      "message": "Offered 192.168.1.100 to client 00:11:22:33:44:55"
    }
  ]
}
```

## Connection Management

### Connection Lifecycle

1. **Request Received**: UDP datagram on port 67
2. **Parse**: Extract message type, MAC address, requested IP using dhcproto
3. **Context**: Store DhcpRequestContext in protocol instance
4. **Register**: Create ConnectionId and add to ServerInstance
5. **Process**: Call LLM with `dhcp_request` event
6. **Execute**: Build DHCP response using dhcproto and context
7. **Respond**: Send UDP response to client
8. **Update**: Track bytes/packets sent/received
9. **Persist**: Connection remains in UI to show activity

### DHCP Flow

Standard DHCP follows 4-message exchange (DORA):

1. **DISCOVER** (client broadcast) → LLM sends **OFFER**
2. **REQUEST** (client requests offered IP) → LLM sends **ACK**

NetGet implements both sides but doesn't enforce state machine - LLM decides responses.

## Known Limitations

### 1. No Lease Management

- No persistent lease database
- No tracking of assigned IPs
- No IP address pool or allocation logic
- LLM decides IPs on-demand (could assign duplicates)

**Use Case**: Testing, honeypots, demonstrations. Not suitable for production DHCP server.

### 2. No DHCP Relay Support

- Doesn't handle DHCP relay agents (giaddr field)
- No support for DHCP across subnets
- Assumes direct client-server communication

### 3. Limited DHCP Options

Action-based interface supports common options only:

- Subnet Mask, Router, DNS, Lease Time
- Missing: Domain Name, NTP servers, TFTP server, Boot file, etc.
- Workaround: Use `send_dhcp_response` for custom options

### 4. No DHCP Decline/Release Tracking

- Ignores DHCPRELEASE messages
- No handling of DHCPDECLINE (address already in use)
- No conflict detection

### 5. No DHCPv6 Support

- IPv4 only (RFC 2131)
- No support for IPv6 address assignment (RFC 8415)

### 6. Feature-Gated

- DHCP parsing requires `dhcp` feature flag
- Without feature: Server accepts packets but can't parse them
- Action execution returns error if feature not enabled

## Example Prompts

### Basic DHCP Server

```
listen on port 67 via dhcp
When receiving DHCP DISCOVER, offer IP addresses from 192.168.1.100 onwards
When receiving DHCP REQUEST, acknowledge with:
  - Subnet mask: 255.255.255.0
  - Router: 192.168.1.1
  - DNS: 8.8.8.8
  - Lease time: 24 hours
```

### DHCP Server with IP Range

```
listen on port 67 via dhcp
Assign IPs in range 10.0.0.100 to 10.0.0.200
Use subnet mask 255.255.255.0, gateway 10.0.0.1, DNS 1.1.1.1
Lease time: 12 hours
```

### DHCP Server with Rejection

```
listen on port 67 via dhcp
Only assign IPs to MAC addresses starting with 00:11:22
For other clients, send DHCP NAK with message "Unauthorized device"
```

### Static IP Assignment

```
listen on port 67 via dhcp
For MAC 00:11:22:33:44:55, always assign 192.168.1.50
For MAC 00:11:22:33:44:66, always assign 192.168.1.51
For other clients, assign from 192.168.1.100 onwards
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
- DHCP traffic is typically low volume (client requests IP once)

### Scripting Compatibility

DHCP is good candidate for scripting:

- Repetitive request/response pattern
- Deterministic IP assignment logic
- Simple state machine (DISCOVER→OFFER, REQUEST→ACK)

When scripting enabled:

- Server startup generates script (1 LLM call)
- All requests handled by script (0 LLM calls)
- Script can implement IP pool logic, static assignments, etc.

## References

- [RFC 2131: Dynamic Host Configuration Protocol](https://datatracker.ietf.org/doc/html/rfc2131)
- [RFC 2132: DHCP Options and BOOTP Vendor Extensions](https://datatracker.ietf.org/doc/html/rfc2132)
- [dhcproto Documentation](https://docs.rs/dhcproto/latest/dhcproto/)
- [DHCP Message Types (IANA)](https://www.iana.org/assignments/bootp-dhcp-parameters/bootp-dhcp-parameters.xhtml)
- [DHCP Options (IANA)](https://www.iana.org/assignments/bootp-dhcp-parameters/bootp-dhcp-parameters.xhtml#options)
