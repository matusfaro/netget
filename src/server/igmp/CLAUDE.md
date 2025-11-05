# IGMP Protocol Implementation

## Overview

IGMP (Internet Group Management Protocol) is used by IPv4 hosts and adjacent routers to establish multicast group memberships. This implementation provides LLM control over multicast group management and IGMP message handling.

## Protocol Details

**Standard**: RFC 2236 (IGMPv2), RFC 3376 (IGMPv3)
**Transport**: IP Protocol 2 (raw IP packets)
**Port**: N/A (operates at IP layer)
**Versions Supported**: IGMPv1, IGMPv2 (partial IGMPv3)

### Message Types

1. **Membership Query (0x11)**: Sent by routers to discover which groups have members
   - General Query: group_address = 0.0.0.0
   - Group-Specific Query: group_address = specific multicast group

2. **Membership Report (0x16)**: Sent by hosts to join a group or respond to queries
   - IGMPv1: type 0x12
   - IGMPv2: type 0x16
   - IGMPv3: type 0x22

3. **Leave Group (0x17)**: Sent by hosts to leave a group (IGMPv2+)

### IGMP Packet Format (8 bytes minimum)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Type      | Max Resp Time |           Checksum            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Group Address                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

## Library Choices

### Server Library

**Current**: `tokio::net::UdpSocket` (placeholder)

**Production**: Should use `socket2::Socket` with:
- Domain: `AF_INET` (IPv4)
- Type: `SOCK_RAW`
- Protocol: `IPPROTO_IGMP` (2)

**Why raw sockets required**:
- IGMP operates at IP layer (protocol 2), not UDP/TCP
- Requires root/CAP_NET_RAW privileges
- Need to set IP_HDRINCL or similar options
- Must handle IP header construction

**Current Limitation**: Raw socket implementation requires elevated privileges and is platform-specific. The current implementation uses UDP as a placeholder for testing.

### Client Library (for testing)

**Option 1**: Manual packet construction with `socket2`
**Option 2**: Use system multicast join/leave (automatic IGMP)

## LLM Integration

### Control Points

**Async Actions** (require external state changes):
- `join_group` - Join a multicast group
- `leave_group` - Leave a multicast group

**Sync Actions** (immediate packet responses):
- `send_membership_report` - Send IGMP Membership Report
- `send_leave_group` - Send IGMP Leave Group message
- `ignore_message` - Don't respond to this message

### Events

1. **igmp_query_received**: Router querying for group members
   - Parameters: query_type, group_address, max_response_time
   - Common response: send_membership_report (if member of queried group)

2. **igmp_report_received**: Another host reporting group membership
   - Parameters: group_address
   - Common response: ignore_message (suppress own report per IGMP)

3. **igmp_leave_received**: Another host leaving a group
   - Parameters: group_address
   - Common response: ignore_message

### LLM Decision Making

The LLM controls:
1. **Group Membership**: Which multicast groups to join/leave
2. **Query Responses**: Whether to respond to membership queries
3. **Report Suppression**: IGMPv2 includes report suppression - if we hear another host's report, we can cancel our own
4. **Timing**: When to send unsolicited reports

## Logging Strategy

Follows NetGet dual logging pattern (tracing + status_tx):

- **ERROR**: Failed to parse IGMP packet, socket errors
- **WARN**: Raw socket limitations, privilege issues
- **INFO**: Group join/leave events, LLM messages
- **DEBUG**: Message summaries (type, group, source)
- **TRACE**: Full packet hex dumps

Examples:
```rust
debug!("IGMP received from {}: {}", peer_addr, igmp_msg.description());
let _ = status_tx.send(format!("[DEBUG] IGMP received from {}: {}", peer_addr, igmp_msg.description()));

trace!("IGMP data (hex): {}", hex_str);
let _ = status_tx.send(format!("[TRACE] IGMP data (hex): {}", hex_str));
```

## Architecture Details

### Connection Tracking

IGMP is connectionless (like UDP). We track:
- Recent peers that sent IGMP messages
- Joined multicast groups
- Last activity timestamp

### State Machine

Server maintains `IgmpServerState`:
- `joined_groups`: Set of Ipv4Addr representing joined multicast groups

### Packet Processing Flow

1. Receive IGMP packet from socket
2. Parse packet (type, max_response_time, group_address, checksum)
3. Create connection state with protocol info
4. Determine event type based on message type
5. Call LLM with event
6. Execute actions:
   - Sync: Build and send IGMP response packet
   - Async: Update joined_groups state, perform socket operations

## Implementation Limitations

### Current Limitations

1. **Raw Socket Placeholder**: Uses UDP socket instead of raw IP socket
   - Production needs: `socket2::Socket` with `SOCK_RAW` and `IPPROTO_IGMP`
   - Requires: Root privileges or `CAP_NET_RAW` capability

2. **Multicast Join/Leave**: Currently simulated
   - Production needs: `socket.join_multicast_v4()` and `leave_multicast_v4()`
   - Requires: Raw socket with proper setup

3. **IGMPv3 Support**: Partial
   - Can parse IGMPv3 reports (type 0x22)
   - Cannot construct IGMPv3 reports with source lists

4. **Router Functionality**: Not implemented
   - Current implementation is host-side only
   - Router would need to send queries and track group members

### Future Enhancements

1. **Full Raw Socket Support**:
   ```rust
   use socket2::{Socket, Domain, Type, Protocol};

   let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::IGMPV2))?;
   socket.set_header_included(true)?;
   ```

2. **IGMPv3 Source Filtering**:
   - INCLUDE mode: specific sources
   - EXCLUDE mode: all except specific sources

3. **Router Mode**:
   - Send periodic general queries
   - Track per-interface group membership
   - Handle leave group messages with group-specific queries

4. **Automatic Report Suppression**:
   - Random delay before sending reports
   - Cancel report if another host reports first

## Example Prompts

### Basic Multicast Group Member

```
Create an IGMP server that joins multicast group 239.255.255.250 and
responds to membership queries with reports for that group.
```

Expected behavior:
1. Server starts
2. LLM decides to join 239.255.255.250
3. On query for 0.0.0.0 (general), sends report for 239.255.255.250
4. On query for 239.255.255.250, sends report
5. On query for other groups, ignores

### SSDP/UPnP Device

```
Create an IGMP server for a UPnP device. Join the SSDP multicast group
239.255.255.250 and respond to all membership queries.
```

Expected behavior:
1. Join SSDP multicast group (239.255.255.250)
2. Respond to all general queries
3. Respond to group-specific queries for SSDP group

### Report Suppression Example

```
Create an IGMP server that implements report suppression. Join group
224.0.1.1, but if you receive another host's report for that group
within the max response time window, don't send your own report.
```

Expected behavior:
1. Join group 224.0.1.1
2. On general query, set timer to send report
3. If receive another host's report for 224.0.1.1, suppress own report
4. If timer expires without seeing report, send report

## Testing Notes

See `tests/server/igmp/CLAUDE.md` for E2E testing strategy.

Key testing considerations:
- Manual IGMP packet construction required
- Test on loopback or isolated network
- May require root privileges for raw sockets
- Multicast routing must be enabled on test interface

## References

- RFC 1112: Host Extensions for IP Multicasting
- RFC 2236: Internet Group Management Protocol, Version 2
- RFC 3376: Internet Group Management Protocol, Version 3
- RFC 4604: Using Internet Group Management Protocol Version 3 (IGMPv3) and Multicast Listener Discovery Protocol Version 2 (MLDv2) for Source-Specific Multicast
