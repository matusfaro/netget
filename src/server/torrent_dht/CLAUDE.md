# BitTorrent DHT Protocol - Implementation

## Overview

The BitTorrent DHT (Distributed Hash Table) is a UDP-based peer discovery system using Kademlia DHT. It allows
decentralized peer location without centralized trackers. This implementation provides a fully LLM-controlled DHT node
that can respond to DHT queries.

## Protocol Specification

- **Base Protocol**: UDP (connectionless)
- **Encoding**: Bencode (KRPC - Kademlia RPC)
- **DHT Type**: Kademlia (160-bit node IDs, XOR distance metric)
- **Port**: Typically 6881 (user-configurable)
- **RFC/BEP**: BEP 5 (DHT Protocol)

## Architecture

### Server Implementation (`mod.rs`)

**Library Choice**: Pure Tokio + serde_bencode

- No external DHT library (KRPC parsing is straightforward)
- `tokio::net::UdpSocket` for datagram handling
- `serde_bencode` (0.2) for KRPC message encoding/decoding
- Stateless design (each query handled independently)

**Key Components**:

```rust
pub struct TorrentDhtServer;

impl TorrentDhtServer {
    pub async fn spawn_with_llm_actions(...) -> Result<SocketAddr>
    fn parse_krpc_message(data: &[u8]) -> Result<(String, serde_json::Value)>
    fn bencode_to_json(value: &serde_bencode::value::Value) -> serde_json::Value
}
```

**Connection Flow**:

1. Bind UDP socket
2. Receive datagram (up to 65535 bytes)
3. Parse bencode KRPC message
4. Identify query type (ping, find_node, get_peers, announce_peer)
5. Convert to JSON for LLM
6. LLM returns action (send_ping_response, send_find_node_response, etc.)
7. Encode response in bencode KRPC format
8. Send datagram back to peer
9. Continue listening (UDP is connectionless)

### KRPC Message Format

**Query Message**:

```python
{
  "t": "aa",                    # Transaction ID (2 bytes, arbitrary)
  "y": "q",                     # Message type: "q" = query
  "q": "ping",                  # Query method name
  "a": {                        # Arguments dictionary
    "id": "<20-byte node ID>"
  }
}
```

**Response Message**:

```python
{
  "t": "aa",                    # Same transaction ID as query
  "y": "r",                     # Message type: "r" = response
  "r": {                        # Response dictionary
    "id": "<20-byte node ID>"   # Responder's node ID
  }
}
```

**Error Message**:

```python
{
  "t": "aa",                    # Transaction ID
  "y": "e",                     # Message type: "e" = error
  "e": [201, "Error message"]   # Error code + description
}
```

### LLM Actions (`actions.rs`)

**Protocol Trait Implementation**: `Server` trait from `crate::llm::actions::protocol_trait`

**Sync Actions** (network-triggered):

1. **send_ping_response** - Respond to DHT ping
    - Parameters: `transaction_id` (hex string), `node_id` (hex string, optional)
    - Output: Bencode KRPC response
    - Example:
   ```json
   {
     "type": "send_ping_response",
     "transaction_id": "aa",
     "node_id": "0123456789abcdef0123456789abcdef01234567"
   }
   ```

2. **send_find_node_response** - Return closest nodes to target
    - Parameters: `transaction_id`, `node_id`, `nodes` (array of {id, ip, port})
    - Output: Bencode response with compact node info (26 bytes per node: 20 ID + 4 IP + 2 port)
    - Example:
   ```json
   {
     "type": "send_find_node_response",
     "transaction_id": "aa",
     "node_id": "0123456789abcdef0123456789abcdef01234567",
     "nodes": [
       {"id": "fedcba9876543210fedcba9876543210fedcba98", "ip": "192.168.1.100", "port": 6881}
     ]
   }
   ```

3. **send_get_peers_response** - Return peers for info_hash (or closest nodes)
    - Parameters: `transaction_id`, `node_id`, `token`, `peers` (optional array)
    - Output: Bencode response with compact peer list (6 bytes per peer: 4 IP + 2 port)
    - Example:
   ```json
   {
     "type": "send_get_peers_response",
     "transaction_id": "aa",
     "node_id": "0123456789abcdef0123456789abcdef01234567",
     "token": "aoeusnth",
     "peers": [
       {"ip": "192.168.1.100", "port": 51413}
     ]
   }
   ```

**Event Types** (incoming queries):

1. **dht_ping_query** - DHT node health check
    - Payload: `{transaction_id: "aa", id: "node_id_hex"}`
    - Purpose: Verify node is alive

2. **dht_find_node_query** - Request closest nodes to target ID
    - Payload: `{transaction_id: "aa", id: "querier_node_id", target: "target_node_id"}`
    - Purpose: Kademlia routing table population

3. **dht_get_peers_query** - Request peers for info_hash
    - Payload: `{transaction_id: "aa", id: "querier_node_id", info_hash: "torrent_info_hash"}`
    - Purpose: Peer discovery for specific torrent

4. **dht_announce_peer_query** (not implemented yet)
    - Payload: `{transaction_id: "aa", id: "querier_node_id", info_hash: "...", port: 6881, token: "..."}`
    - Purpose: Announce peer's participation in torrent

### Compact Encoding

**Compact Node Info** (26 bytes per node):

- 20 bytes: Node ID
- 4 bytes: IPv4 address (network byte order)
- 2 bytes: Port (network byte order)

**Compact Peer Info** (6 bytes per peer):

- 4 bytes: IPv4 address
- 2 bytes: Port

**Encoding Implementation**:

```rust
// Nodes
let mut compact = id;                                    // 20 bytes
compact.extend_from_slice(&ip_parts);                   // 4 bytes
compact.extend_from_slice(&port.to_be_bytes());         // 2 bytes

// Peers
let mut compact = ip_parts;                              // 4 bytes
compact.extend_from_slice(&port.to_be_bytes());         // 2 bytes
```

### Bencode ↔ JSON Conversion

**bencode_to_json()**: Recursively converts bencode to JSON

- `Value::Int` → `json!(i)`
- `Value::Bytes` → UTF-8 string if printable, else hex string
- `Value::List` → JSON array
- `Value::Dict` → JSON object

**Example**:

```rust
// Bencode: d1:ti2e1:y1:q1:q4:ping1:ad2:id20:abcdefghij0123456789ee
// JSON: {"t": 2, "y": "q", "q": "ping", "a": {"id": "6162636465666768696a30313233343536373839"}}
```

## LLM Integration

### Instruction Guidelines

**Example Instruction**:

```
You are a BitTorrent DHT node. Respond to ping queries with your node ID. For find_node queries, return a list of nearby nodes. For get_peers queries, return known peers for the info_hash if available, otherwise return nodes.
```

**Behavior Control**:

- **Node ID**: LLM can use a fixed node ID (e.g., all zeros, or random) or generate per-response
- **Routing Table**: LLM can maintain a routing table in conversation history or return empty/random nodes
- **Peer Storage**: LLM can track announced peers or always return empty peer lists
- **Token Generation**: LLM should generate tokens for get_peers (required for announce_peer validation)

### Typical LLM Response Flow

**Ping Query**:

1. LLM receives: `{transaction_id: "aa", id: "abcd..."}`
2. LLM returns: `{type: "send_ping_response", transaction_id: "aa", node_id: "0000..."}`

**Find Node Query**:

1. LLM receives: `{transaction_id: "bb", id: "abcd...", target: "1234..."}`
2. LLM returns closest nodes (or random nodes if no routing table):
   ```json
   {
     "type": "send_find_node_response",
     "transaction_id": "bb",
     "nodes": [
       {"id": "1111...", "ip": "192.168.1.10", "port": 6881},
       {"id": "2222...", "ip": "192.168.1.20", "port": 6881}
     ]
   }
   ```

**Get Peers Query**:

1. LLM receives: `{transaction_id: "cc", id: "abcd...", info_hash: "xyz..."}`
2. If LLM knows peers: Return peer list
3. If LLM doesn't know: Return nodes (fallback to find_node behavior)
4. LLM must include token for future announce_peer

## Logging Strategy

**DEBUG Level**:

- Datagram received (size, peer address)
- Query type identified
- LLM call initiated
- Response sent (size)

**TRACE Level**:

- Full datagram (hex)
- Parsed KRPC structure
- Full response (hex)

**INFO Level**:

- LLM-generated messages

**ERROR Level**:

- Bencode parse errors
- LLM call failures
- Socket errors

## Connection State Tracking

**ProtocolConnectionInfo Variant**:

```rust
TorrentDht {
    recent_queries: Vec<(String, Instant)>,  // Query type + timestamp
}
```

Note: UDP is connectionless, so each datagram creates a temporary "connection" entry. Connections are short-lived (
single query-response).

## Limitations

1. **No Routing Table**: LLM-based DHT doesn't maintain a persistent Kademlia routing table. Responses may be random or
   empty.

2. **No Peer Storage**: Peers announced via announce_peer are not persisted (unless LLM explicitly tracks them).

3. **No Bootstrap**: No automatic bootstrapping to join the global DHT network. Node exists in isolation unless manually
   connected.

4. **IPv4 Only**: Compact encoding only supports IPv4. No IPv6 support (BEP 32).

5. **No Security Extensions**: BEP 42 (DHT Security Extension) not implemented. Node ID spoofing is possible.

6. **Stateless**: Each query is independent. No concept of "good" vs "bad" nodes, timeouts, or routing table
   maintenance.

7. **Limited Query Types**: Only ping, find_node, get_peers implemented. announce_peer parsing exists but no action
   defined yet.

## DHT Concepts

**Node ID**: 160-bit (20-byte) identifier. Randomly chosen at startup. Should be persistent across restarts for real DHT
participation.

**XOR Distance Metric**: Distance between two node IDs is XOR of their binary representations. Closer IDs = smaller XOR
result.

**K-Buckets**: Routing table divided into 160 buckets (one per bit prefix). Each bucket stores up to K nodes (K=8
typical).

**Iterative Lookup**: To find a node/peer:

1. Query closest known nodes
2. They return even closer nodes
3. Repeat until target found or no closer nodes exist

**Token Mechanism**: get_peers returns a token. Client must echo this token in announce_peer to prove they recently
queried. Prevents announce spam.

## Security Considerations

- **No rate limiting**: Vulnerable to query floods (LLM could implement via scheduled tasks)
- **No ID verification**: Node IDs can be spoofed (BEP 42 solves this)
- **Token validation**: Tokens are generated by LLM, no cryptographic verification
- **Amplification attacks**: Small query → large response (e.g., 100 nodes = 2600 bytes)

## Testing

See `tests/server/torrent_dht/CLAUDE.md` for comprehensive testing documentation.

## References

- [BEP 5: DHT Protocol](http://www.bittorrent.org/beps/bep_0005.html)
- [BEP 32: IPv6 DHT](http://www.bittorrent.org/beps/bep_0032.html)
- [BEP 42: DHT Security Extension](http://www.bittorrent.org/beps/bep_0042.html)
- [Kademlia Paper](https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf)
- [BitTorrent DHT Specification](https://wiki.theory.org/BitTorrentSpecification#Distributed_Hash_Table)
