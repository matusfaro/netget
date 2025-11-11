# IGMP Client Implementation

## Overview

The IGMP (Internet Group Management Protocol) client enables NetGet to join and leave multicast groups, receive
multicast data, and send multicast packets. This implementation uses socket options for multicast group management,
which doesn't require root privileges for basic operations.

## Library Choices

### Primary Approach: Socket Options (socket2)

- **Library**: `socket2` v0.5 (already a NetGet dependency)
- **Purpose**: Multicast group join/leave using `IP_ADD_MEMBERSHIP` and `IP_DROP_MEMBERSHIP`
- **Privileges**: No root required for receiving multicast
- **Pros**:
    - Simple and portable
    - Kernel handles IGMP protocol messages automatically
    - Works on all platforms (Linux, macOS, Windows)
    - No raw socket privileges needed
- **Cons**:
    - Cannot manually craft IGMP packets
    - Relies on kernel's IGMP implementation

### Data Transport: tokio::net::UdpSocket

- **Library**: `tokio::net::UdpSocket` (tokio standard library)
- **Purpose**: Receiving multicast UDP datagrams and sending multicast data
- **Integration**: Seamless async I/O with tokio runtime

## Architecture

### Connection Model

The IGMP client is **connectionless** but maintains **active listening state**:

1. **Bind Phase**: Create UDP socket bound to `0.0.0.0:PORT` (or user-specified)
2. **Active State**: Client remains active to receive multicast data
3. **Group Management**: Join/leave multicast groups dynamically via LLM actions
4. **Reception Loop**: Async loop receives multicast datagrams from joined groups

### State Machine

```
Idle → Processing → Idle
       ↓
   Accumulating (if queued data exists)
```

- **Idle**: Waiting for multicast data
- **Processing**: LLM handling current datagram
- **Accumulating**: Queued data exists, waiting for LLM to finish

### Multicast Group Tracking

The client tracks joined multicast groups in `IgmpClientData::joined_groups` (HashSet<Ipv4Addr>). This allows:

- Preventing duplicate joins
- Tracking active memberships
- Clean leave on client shutdown (future enhancement)

## LLM Integration

### Events

1. **igmp_connected** - Triggered when client binds and is ready
    - Parameters: `local_addr` (socket bind address)

2. **igmp_data_received** - Triggered on multicast datagram reception
    - Parameters:
        - `data_hex`: Hexadecimal-encoded datagram payload
        - `data_length`: Payload size in bytes
        - `source_addr`: Sender's IP:port

### Actions

#### Async Actions (User-triggered)

1. **join_multicast_group**
    - Joins an IPv4 multicast group
    - Parameters:
        - `multicast_addr`: Multicast IP (e.g., `239.1.2.3`)
        - `interface_addr`: Local interface (default: `0.0.0.0` for any)
    - Effect: Kernel sends IGMP Membership Report
    - Example:
      ```json
      {
        "type": "join_multicast_group",
        "multicast_addr": "239.1.2.3",
        "interface_addr": "0.0.0.0"
      }
      ```

2. **leave_multicast_group**
    - Leaves an IPv4 multicast group
    - Parameters: Same as join
    - Effect: Kernel sends IGMP Leave Group message
    - Example:
      ```json
      {
        "type": "leave_multicast_group",
        "multicast_addr": "239.1.2.3"
      }
      ```

3. **send_multicast**
    - Sends data to a multicast group
    - Parameters:
        - `multicast_addr`: Destination multicast IP
        - `port`: Destination port
        - `data_hex`: Hex-encoded payload
    - Example:
      ```json
      {
        "type": "send_multicast",
        "multicast_addr": "239.1.2.3",
        "port": 5000,
        "data_hex": "48656c6c6f"
      }
      ```

#### Sync Actions (Response to events)

1. **wait_for_more** - Accumulate more multicast data before responding

## Implementation Details

### Multicast Join/Leave

The implementation uses `socket2::Socket::join_multicast_v4()` and `leave_multicast_v4()`:

```rust
let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
sock.join_multicast_v4(&multicast_ip, &interface_ip)?; // Kernel sends IGMP report
sock.leave_multicast_v4(&multicast_ip, &interface_ip)?; // Kernel sends IGMP leave
```

**Important**: These operations trigger the kernel to send IGMP protocol messages (Membership Report, Leave Group). The
client doesn't construct raw IGMP packets.

### Multicast Reception

The UDP socket automatically receives multicast datagrams once joined to a group:

```rust
let socket = UdpSocket::bind("0.0.0.0:PORT").await?;
// After join_multicast_group, socket receives multicast data
let (n, peer_addr) = socket.recv_from(&mut buffer).await?;
```

### Multicast Sending

Sending to a multicast group uses standard UDP send:

```rust
socket.send_to(&data, "239.1.2.3:5000").await?;
```

**TTL Consideration**: Default TTL=1 (link-local). For broader multicast, set TTL explicitly (future enhancement).

## Limitations

### 1. IPv4 Only

- Current implementation supports only IPv4 multicast (224.0.0.0/4)
- IPv6 multicast (ff00::/8) requires different socket options (`IPV6_ADD_MEMBERSHIP`)

### 2. No Raw IGMP Packet Construction

- Cannot manually craft IGMP packets (requires raw sockets + root)
- Cannot send custom IGMP queries or reports
- Relies entirely on kernel's IGMP implementation

### 3. Single Interface Binding

- Socket binds to `0.0.0.0` (all interfaces) by default
- Cannot explicitly select interface for joins without additional socket configuration

### 4. No IGMP Version Control

- Kernel chooses IGMP version (IGMPv1/v2/v3) based on router behavior
- Client cannot force specific IGMP version

### 5. No Group-Source Filtering (IGMPv3)

- Cannot use source-specific multicast (SSM)
- Would require `IP_ADD_SOURCE_MEMBERSHIP` socket option (future enhancement)

## Testing Considerations

### Local Testing

Multicast works on localhost but has limitations:

- **Loopback Multicast**: Enabled by default on most systems
- **Same Machine**: Can test sender/receiver on same host
- **Firewall**: Ensure multicast traffic (224.0.0.0/4) is not blocked

### Network Testing

For real multicast testing:

- **Multicast-Capable Network**: Router must support IGMP
- **IGMP Querier**: At least one IGMP querier on subnet
- **TTL**: Ensure TTL > 1 for multi-hop multicast

### Common Multicast Groups

- **224.0.0.1**: All hosts on subnet
- **224.0.0.2**: All routers on subnet
- **239.0.0.0/8**: Administratively scoped (organization-local)

## Example Prompts

1. **Join Group and Listen**:
   ```
   Join multicast group 239.1.2.3 and wait for data
   ```

2. **Send Hello Message**:
   ```
   Send "Hello Multicast" to group 239.1.2.3 port 5000
   ```

3. **Leave Group**:
   ```
   Leave multicast group 239.1.2.3
   ```

4. **Monitor Multiple Groups**:
   ```
   Join groups 239.1.1.1 and 239.1.2.2, log all received data
   ```

## Future Enhancements

1. **IPv6 Multicast Support**: Add `IPV6_ADD_MEMBERSHIP`
2. **TTL Control**: Allow LLM to set multicast TTL
3. **Source-Specific Multicast (SSM)**: Use `IP_ADD_SOURCE_MEMBERSHIP` for IGMPv3
4. **Interface Selection**: Bind to specific network interface
5. **Multicast Loopback Control**: Enable/disable receiving own sent multicast
6. **Raw IGMP Mode**: Optional raw socket mode for crafting IGMP packets (requires root)

## Comparison to Server Implementation

NetGet also has an IGMP **server** (`src/server/igmp/`), which differs:

| Feature          | Client                                | Server                                |
|------------------|---------------------------------------|---------------------------------------|
| **Purpose**      | Join/leave groups, receive multicast  | Respond to IGMP queries (router-like) |
| **Socket Type**  | UDP socket (port 0 or user-specified) | Raw IP socket (protocol 2)            |
| **Privileges**   | No root required                      | Requires root/CAP_NET_RAW             |
| **IGMP Packets** | Kernel handles                        | Manual construction                   |
| **Use Case**     | Multicast data consumer               | IGMP protocol testing                 |

## Security Considerations

1. **Multicast Amplification**: Be cautious sending to large multicast groups
2. **Firewall Rules**: Multicast may be blocked by firewalls
3. **Local Network Only**: Most multicast is scoped to local networks (TTL=1)
4. **No Authentication**: Multicast has no built-in authentication mechanism

## References

- RFC 1112: Host Extensions for IP Multicasting (IGMPv1)
- RFC 2236: Internet Group Management Protocol, Version 2 (IGMPv2)
- RFC 3376: Internet Group Management Protocol, Version 3 (IGMPv3)
- RFC 4607: Source-Specific Multicast for IP
