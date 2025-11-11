# OSPF Protocol Simulator

## Overview

**OSPF protocol simulator** that speaks real OSPF (IP protocol 89) but has **LLM-generated responses** instead of real
routing logic.

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

**Architecture**: LLM actions return `ActionResult::Output` with raw OSPF packet bytes. The mod.rs event handler
processes these results and calls `send_ospf_packet()` with the raw socket FD from `OspfState`. Currently sends to
multicast (224.0.0.5) by default; unicast destination support is TODO.

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

**Send Database Description (unicast to specific neighbor)**:

```json
{
  "type": "send_database_description",
  "router_id": "1.1.1.1",
  "area_id": "0.0.0.0",
  "sequence": 12345,
  "init": false,
  "more": true,
  "master": true,
  "destination": "192.168.1.2"
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
- Unicast destination support
    - LLM can specify "multicast" (224.0.0.5), "dr_multicast" (224.0.0.6), or unicast IP (e.g., "192.168.1.2")
    - Useful for targeted Database Description/LSU exchanges with specific neighbors

### 📋 TODO

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

## Missing Features & Implementation Guide

### 1. LSA Packet Construction (Router, Network, Summary)

**What's Missing**: Actions can send empty LSU packets, but can't construct LSA contents.

**How to Implement**:

1. Add LSA parameters to `send_link_state_update` action:
   ```json
   {
     "type": "send_link_state_update",
     "router_id": "1.1.1.1",
     "lsas": [
       {
         "type": "router",  // Router LSA (Type 1)
         "age": 0,
         "sequence": 0x80000001,
         "links": [
           {"type": "stub", "id": "10.0.0.0", "data": "255.255.255.0", "metric": 10},
           {"type": "transit", "id": "192.168.1.1", "data": "192.168.1.2", "metric": 1}
         ]
       }
     ],
     "destination": "multicast"
   }
   ```

2. Update `execute_send_link_state_update()` in actions.rs:
    - Parse `lsas` array from JSON
    - For each LSA, serialize to OSPF LSA format:
        - LSA header (20 bytes): age, options, type, link state ID, advertising router, sequence, checksum, length
        - LSA body varies by type
    - Router LSA (Type 1): links array with type, ID, data, metric
    - Network LSA (Type 2): network mask + attached routers
    - Summary LSA (Type 3): network + mask + metric

3. LSA checksum: Use Fletcher checksum over LSA header+body (same as packet checksum but excludes age field)

**Effort**: Medium (2-3 hours). Main challenge is LSA format serialization.

**Priority**: Medium. Needed for full OSPF database exchange, but simulator works without it.

### 2. Database Description Handling (DD Exchange)

**What's Missing**: Can receive DD packets, but LLM doesn't track exchange state or LSA headers.

**How to Implement**:

1. Add DD event parsing in mod.rs:
    - Extract DD flags (I, M, MS), sequence number, MTU
    - Parse LSA headers from DD body (20 bytes each)
    - Send structured JSON to LLM with LSA summaries

2. Add connection state tracking:
    - Current: `OspfNeighborState` is an enum (Down/Init/2-Way/...)
    - Add: `dd_sequence: u32`, `dd_master: bool`, `lsa_requests: Vec<LsaHeader>`

3. LLM prompt additions:
    - "You received Database Description with LSA headers: ..."
    - "You are master/slave in DD exchange"
    - "Respond with your LSA headers or send LSR for missing LSAs"

**Effort**: Low (1-2 hours). Mostly parsing + state tracking.

**Priority**: High. Required for neighbor adjacency formation.

### 3. LSR/LSU/LSAck Handling

**What's Missing**: Can send these packet types (empty), but doesn't parse incoming ones or maintain state.

**How to Implement**:

1. Parse incoming LSR in mod.rs:
    - Extract requested LSA identifiers (type, ID, advertising router)
    - Send JSON event to LLM: `{"event": "ospf_lsr", "requests": [...]}`

2. LLM decides:
    - If we "have" the LSA → `send_link_state_update` with LSA content
    - If we don't → ignore or log

3. Parse incoming LSU:
    - Extract LSAs from packet body
    - Send to LLM: `{"event": "ospf_lsu", "lsas": [...]}`
    - LLM can log, respond with LSAck, or ignore

4. Parse incoming LSAck:
    - Extract acknowledged LSA headers
    - Send to LLM for logging

**Effort**: Medium (2-3 hours). Parsing is straightforward, but LLM guidance needs refinement.

**Priority**: Medium. Nice for completeness, but not critical for basic simulator.

### 4. Periodic Hello Timer

**What's Missing**: Server only responds to incoming packets. Doesn't proactively send Hellos.

**How to Implement**:

1. Use NetGet's scheduled tasks system (see CLAUDE.md § Scheduled Tasks):
   ```rust
   // In spawn_with_llm_actions, add server-scoped task
   let hello_task = ScheduledTask {
       task_id: "hello_broadcast".to_string(),
       server_id: Some(server_id),
       connection_id: None,  // Server-scoped
       recurring: true,
       interval_secs: Some(10),  // Every 10s
       instruction: "Send OSPF Hello to multicast (224.0.0.5) with current DR/BDR state".to_string(),
   };
   ```

2. LLM receives periodic prompt, responds with `send_hello` action

3. Alternative: Implement in mod.rs with tokio::interval:
   ```rust
   let mut hello_timer = tokio::time::interval(Duration::from_secs(10));
   loop {
       tokio::select! {
           _ = hello_timer.tick() => {
               // Send multicast Hello
           }
           // ... packet receive logic
       }
   }
   ```

**Effort**: Low (1 hour with scheduled tasks, 2 hours with tokio::select).

**Priority**: High. Required for maintaining neighbor relationships (40s dead timer).

### 5. Dead Neighbor Detection (40s Timeout)

**What's Missing**: Neighbors never time out. last_hello timestamp tracked but not checked.

**How to Implement**:

1. Add connection-scoped task for each neighbor:
   ```rust
   // When neighbor transitions to Init/2-Way
   let timeout_task = ScheduledTask {
       task_id: format!("neighbor_timeout_{}", neighbor_id),
       server_id: Some(server_id),
       connection_id: Some(connection_id),
       recurring: false,
       delay_secs: Some(40),
       instruction: format!("Check if neighbor {} has sent Hello in last 40s. If not, mark Down.", neighbor_id),
   };
   ```

2. Alternative: Background task in mod.rs:
   ```rust
   tokio::spawn(async move {
       let mut check_timer = tokio::time::interval(Duration::from_secs(10));
       loop {
           check_timer.tick().await;
           let now = Instant::now();
           neighbors.lock().await.retain(|id, neighbor| {
               if now.duration_since(neighbor.last_hello).as_secs() > 40 {
                   warn!("Neighbor {} timed out", id);
                   false
               } else {
                   true
               }
           });
       }
   });
   ```

**Effort**: Low (1 hour).

**Priority**: High. Prevents stale neighbor state.

### 6. DR/BDR Election Logic

**What's Missing**: LLM manually claims DR/BDR via Hello priority. No automatic election.

**How to Implement**:

1. After receiving Hello from all neighbors, run election:
    - Highest priority router with no DR → becomes DR
    - Second-highest → becomes BDR
    - In tie, highest router ID wins

2. Update Hello responses:
    - If we won election → send Hello with dr="our_ip"
    - If we lost → send Hello with dr="winner_ip"

3. Implementation options:
    - **Pure LLM**: Send neighbor list + priorities to LLM, let it decide
    - **Hybrid**: Run election in Rust, send result to LLM for approval
    - **Manual**: Keep current behavior (LLM decides in prompts)

**Effort**: Medium (2-3 hours for full election, 30min for hybrid).

**Priority**: Low. Manual DR claiming works for simulator use case.

## References

- [RFC 2328 - OSPFv2](https://datatracker.ietf.org/doc/html/rfc2328)
- [OSPF Design Guide (Cisco)](https://www.cisco.com/c/en/us/support/docs/ip/open-shortest-path-first-ospf/7039-1.html)
- [FRR OSPF Documentation](https://docs.frrouting.org/en/latest/ospfd.html)

## Comparison: Full Router vs Simulator

| Feature         | Full OSPF Router   | NetGet Simulator |
|-----------------|--------------------|------------------|
| Packet RX/TX    | ✅                  | ✅                |
| Neighbor states | ✅                  | ✅ (partial)      |
| DR/BDR election | ✅ Algorithm        | ❌ LLM claims     |
| LSA flooding    | ✅ Automatic        | ❌ LLM manual     |
| LSDB sync       | ✅ Real sync        | ❌ No LSDB        |
| SPF calculation | ✅ Dijkstra         | ❌ None           |
| Routing table   | ✅ Real routes      | ❌ Fake routes    |
| Route install   | ✅ Kernel           | ❌ None           |
| Code complexity | ~10,000 lines      | ~500 lines       |
| Use case        | Production routing | Testing/honeypot |

**Winner**: Simulator for NetGet's use cases! 🎉
