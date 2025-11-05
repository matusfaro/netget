# OSPF Protocol Simulator

## Overview

**OSPF protocol simulator** that speaks real OSPF (IP protocol 89) but has **LLM-generated responses** instead of real routing logic.

**Philosophy**: NetGet handles protocol details, LLM controls behavior
- ✅ Real OSPF protocol (IP 89)
- ✅ Multicast support (224.0.0.5)
- ✅ LLM generates responses
- ❌ NO real SPF calculation
- ❌ NO real routing table
- ❌ NO actual packet forwarding

**Status**: Experimental (protocol simulator)
**Spec**: [RFC 2328 (OSPFv2)](https://datatracker.ietf.org/doc/html/rfc2328)
**Requires**: Root/CAP_NET_RAW privileges

## Use Cases

### 1. OSPF Honeypot
Detect OSPF reconnaissance and attacks:
```
netget> Listen on interface 192.168.1.100 as OSPF router 10.0.0.1 in area 0
LLM: "Respond to Hellos but log all LSA requests. Advertise fake routes."
```

### 2. Protocol Testing
Test real OSPF routers with controlled responses:
```
netget> OSPF on 10.0.1.1, area 0, be the DR
LLM: "Claim DR priority 255, advertise 3 fake routes, vary LSA ages"
```

### 3. Route Injection
Inject test routes into OSPF networks:
```
netget> OSPF router, inject route 192.168.99.0/24
LLM: "Generate Router LSA with fake link to 192.168.99.0/24, metric 10"
```

### 4. OSPF Education
Learn OSPF without setting up real routers:
```
netget> Be an OSPF router, explain what you're doing
LLM: "Received Hello from 2.2.2.2. Sending Hello back with priority 1..."
```

## Architecture

### Raw Socket Implementation

Uses IP protocol 89 (not UDP):

```rust
let socket = create_ospf_raw_socket(interface_ip, true, false)?;
// - IP protocol 89
// - Joins multicast 224.0.0.5 (AllSPFRouters)
// - Requires root/CAP_NET_RAW
```

### Packet Flow

**Incoming**:
```
Real Router → OSPF packet (IP proto 89) → NetGet
                                           ↓
                               Parse IP header, extract OSPF
                                           ↓
                               Parse OSPF header (ver, type, router_id)
                                           ↓
                               Create structured JSON event
                                           ↓
                               Send to LLM with context
```

**Outgoing**:
```
LLM → JSON action → execute_action() → ActionResult::Output(packet_bytes)
                                           ↓
                               mod.rs processes protocol_results
                                           ↓
                               send_ospf_packet(socket_fd, dest_ip, bytes)
                                           ↓
                               Raw sendto() to 224.0.0.5 (multicast)
```

**Architecture**: LLM actions return `ActionResult::Output` with raw OSPF packet bytes. The mod.rs event handler processes these results and calls `send_ospf_packet()` with the raw socket FD from `OspfState`. Currently sends to multicast (224.0.0.5) by default; unicast destination support is TODO.

### No Real Routing

**What we DON'T implement**:
- SPF (Dijkstra) calculation
- Real routing table
- Route installation (netlink)
- DR/BDR election algorithm
- LSA aging timers
- Proper LSDB synchronization

**What LLM controls**:
- Whether to respond to Hellos
- What routes to advertise (fake or real)
- DR/BDR claim (just set priority)
- LSA content (manually crafted)
- Neighbor acceptance/rejection

## LLM Integration

### Event Structure (Input to LLM)

When OSPF Hello received:

```json
{
  "event": "ospf_hello",
  "data": {
    "connection_id": "conn-12345",
    "neighbor_id": "2.2.2.2",
    "neighbor_ip": "192.168.1.2",
    "area_id": "0.0.0.0",
    "network_mask": "255.255.255.0",
    "hello_interval": 10,
    "router_dead_interval": 40,
    "router_priority": 1,
    "dr": "0.0.0.0",
    "bdr": "0.0.0.0",
    "neighbors": ["1.1.1.1", "3.3.3.3"]
  }
}
```

### Action Structure (Output from LLM)

**Send Hello Response**:
```json
{
  "type": "send_hello",
  "router_id": "1.1.1.1",
  "area_id": "0.0.0.0",
  "network_mask": "255.255.255.0",
  "priority": 100,
  "dr": "1.1.1.1",
  "bdr": "0.0.0.0",
  "neighbors": ["2.2.2.2"],
  "destination": "multicast"
}
```

**Generate Fake Router LSA** (TODO):
```json
{
  "type": "send_lsa",
  "lsa_type": "router",
  "router_id": "1.1.1.1",
  "links": [
    {"type": "stub", "id": "10.0.0.0", "mask": "255.255.255.0", "metric": 10},
    {"type": "stub", "id": "10.0.1.0", "mask": "255.255.255.0", "metric": 20}
  ],
  "destination": "multicast"
}
```

## State Management

### Neighbor Tracking

```rust
struct OspfNeighbor {
    router_id: String,
    neighbor_ip: Ipv4Addr,
    state: OspfNeighborState,  // Down/Init/2-Way/...
    priority: u8,
    dr: String,
    bdr: String,
    last_hello: Instant,
}
```

**State Transitions** (simplified):
- Down → Init (receive first Hello)
- Init → 2-Way (bidirectional Hello)
- 2-Way → ExStart (form adjacency - TODO)
- ExStart → Exchange (DD packets - TODO)
- Exchange → Loading (LSR/LSU - TODO)
- Loading → Full (synchronized - TODO)

**Currently**: Only Down → Init → 2-Way implemented

### No LSDB

No real Link State Database. LLM generates LSAs on demand:
- LLM remembers what it advertised (via conversation history)
- LLM generates fake LSAs when requested
- No LSA aging, no MaxAge, no refresh timers

## Packet Construction

### Hello Packet

Built from LLM JSON action:

```rust
// OSPF Header (24 bytes)
- Version: 2
- Type: 1 (Hello)
- Packet Length: calculated
- Router ID: from LLM
- Area ID: from LLM
- Checksum: Fletcher checksum
- Auth Type: 0 (none)
- Authentication: zeros

// Hello Body
- Network Mask: from LLM
- Hello Interval: from LLM (default 10s)
- Options: 0
- Router Priority: from LLM
- Router Dead Interval: from LLM (default 40s)
- Designated Router: from LLM
- Backup DR: from LLM
- Neighbors: list from LLM
```

### LSA Packets (TODO)

Router LSA, Network LSA, Summary LSA - all generated from LLM JSON.

## Sending Packets

### Multicast vs Unicast

```rust
// Send to AllSPFRouters (224.0.0.5)
send_ospf_packet(socket_fd, Ipv4Addr::new(224, 0, 0, 5), packet_bytes)?;

// Send to specific neighbor
send_ospf_packet(socket_fd, neighbor_ip, packet_bytes)?;
```

### Raw Socket Sendto

```rust
unsafe {
    let dest_addr = libc::sockaddr_in {
        sin_family: libc::AF_INET as u16,
        sin_port: 0,  // Raw IP, no port
        sin_addr: libc::in_addr {
            s_addr: u32::from(dest_ip).to_be(),
        },
        sin_zero: [0; 8],
    };

    libc::sendto(
        socket_fd,
        packet.as_ptr() as *const libc::c_void,
        packet.len(),
        0,
        &dest_addr as *const _ as *const libc::sockaddr,
        std::mem::size_of::<libc::sockaddr_in>() as u32,
    );
}
```

## Current Implementation Status

### ✅ Completed
- Raw IP socket creation (protocol 89)
- Multicast group join (224.0.0.5)
- IP header parsing
- OSPF header parsing
- Hello packet parsing
- Neighbor state tracking (Down/Init/2-Way)
- Structured JSON events to LLM
- Hello packet construction
- Packet transmission function
- Connect LLM actions to packet sending
  - LLM JSON responses converted to OSPF packets
  - Packets sent to multicast (224.0.0.5) by default
  - Full logging (dual tracing + status_tx)

### 📋 TODO
- Unicast destination support (send to specific neighbor instead of multicast)
- LSA packet construction (Router, Network, Summary)
- Database Description handling
- LSR/LSU/LSAck handling
- Periodic Hello timer
- Dead neighbor detection (40s timeout)
- DR/BDR claim logic

## Testing

### With Real OSPF Router (FRR)

**Setup FRR**:
```bash
# On Linux router
sudo apt install frr
sudo vi /etc/frr/ospfd.conf

router ospf
  network 192.168.1.0/24 area 0

interface eth0
  ip ospf hello-interval 10
  ip ospf dead-interval 40
```

**Start NetGet** (requires root):
```bash
sudo ./netget
netget> Listen on interface 192.168.1.100 as OSPF router 192.168.1.100 in area 0

# FRR will send Hellos to 224.0.0.5
# NetGet receives, parses, sends event to LLM
# LLM decides response
# NetGet sends Hello back
```

**Observe FRR**:
```bash
sudo vtysh -c "show ip ospf neighbor"
# Should see NetGet router if LLM responds correctly
```

### Current Limitations

**Cannot test yet**:
- Full adjacency formation (need DD/LSR/LSU)
- Route advertisement (need LSA generation)
- Route redistribution
- SPF calculation (we don't do this)
- Multi-area OSPF

**Can test**:
- Hello packet exchange
- Neighbor discovery
- State transitions (Down/Init/2-Way)
- Multicast reception
- Protocol parsing

## Security Considerations

### Honeypot Mode

Detect OSPF attacks:
- Neighbor scanning
- Route poisoning attempts
- LSA flooding
- Area spoofing

LLM can log malicious behavior and respond defensively.

### Route Injection Risks

**Be careful**: Advertising fake routes in production networks can:
- Cause routing loops
- Black-hole traffic
- Break connectivity
- Trigger security alerts

**Use only**:
- Test networks
- Isolated environments
- With network admin permission

## Examples

### Passive OSPF Listener

```
netget> OSPF on 10.0.0.1, area 0, priority 0, log all packets

LLM receives Hellos, logs them, doesn't respond (priority 0 = never DR)
```

### Aggressive DR Claim

```
netget> OSPF router 10.0.0.1, area 0, become DR immediately

LLM responds with:
{
  "type": "send_hello",
  "priority": 255,
  "dr": "10.0.0.1",
  "bdr": "0.0.0.0"
}
```

### Fake Route Advertisement (TODO)

```
netget> OSPF router, advertise fake routes to 192.168.99.0/24

LLM generates:
{
  "type": "send_lsa",
  "lsa_type": "router",
  "links": [{"id": "192.168.99.0", "mask": "255.255.255.0", "metric": 1}]
}
```

## References

- [RFC 2328 - OSPFv2](https://datatracker.ietf.org/doc/html/rfc2328)
- [OSPF Design Guide (Cisco)](https://www.cisco.com/c/en/us/support/docs/ip/open-shortest-path-first-ospf/7039-1.html)
- [FRR OSPF Documentation](https://docs.frrouting.org/en/latest/ospfd.html)

## Comparison: Full Router vs Simulator

| Feature | Full OSPF Router | NetGet Simulator |
|---------|------------------|------------------|
| Packet RX/TX | ✅ | ✅ |
| Neighbor states | ✅ | ✅ (partial) |
| DR/BDR election | ✅ Algorithm | ❌ LLM claims |
| LSA flooding | ✅ Automatic | ❌ LLM manual |
| LSDB sync | ✅ Real sync | ❌ No LSDB |
| SPF calculation | ✅ Dijkstra | ❌ None |
| Routing table | ✅ Real routes | ❌ Fake routes |
| Route install | ✅ Kernel | ❌ None |
| Code complexity | ~10,000 lines | ~500 lines |
| Use case | Production routing | Testing/honeypot |

**Winner**: Simulator for NetGet's use cases! 🎉
