# Tor Client Implementation

## Overview

The Tor client enables NetGet to make anonymous connections through the Tor network using the Arti library (a pure Rust
implementation of Tor).

## Library Choice

**Arti** (`arti-client` v0.36)

- Official Tor Project implementation in Rust
- Pure Rust (no C dependencies)
- Production-ready (v1.0.0 released)
- Full async/await support with Tokio integration
- Supports regular connections and onion services (.onion addresses)

**Why Arti:**

- Mature and well-maintained by Tor Project
- Safer than C Tor due to memory safety
- Native Tokio integration (AsyncRead + AsyncWrite)
- Simpler API than alternative libraries
- Supports all Tor features we need: exit nodes, onion services, circuit isolation

## Architecture

### Connection Flow

1. **Bootstrap**: Create `TorClient` and bootstrap consensus documents from directory authorities
2. **Circuit Building**: Arti automatically builds circuits through 3+ relays
3. **Connection**: Use `TorClient::connect(target)` to establish anonymized stream
4. **Data Exchange**: Read/write through `DataStream` (implements AsyncRead/AsyncWrite)
5. **LLM Integration**: Call LLM on connection and data received events

### Key Components

- `TorClient` (from arti): Main client instance, manages circuits and connections
- `DataStream` (from arti): Individual anonymized connection (like TcpStream)
- `ConnectionState`: State machine (Idle → Processing → Accumulating) prevents concurrent LLM calls
- `ClientData`: Per-client memory for LLM context

### State Machine

```
Idle ────data────> Processing ─────LLM done────> Idle
                        │                          ↑
                        └──more data──> Accumulating
                                             │
                                             └──(queue data)
```

## LLM Integration

### Events

1. **`tor_connected`**: Triggered when connection established through Tor
    - Parameters: `target` (destination address)
    - LLM can send initial data or wait for response

2. **`tor_data_received`**: Triggered when data received from destination
    - Parameters: `data_hex` (hex-encoded data), `data_length`
    - LLM decides how to respond (send data, wait for more, disconnect)

### Actions

**Async Actions** (user-initiated):

- `send_tor_data`: Send hex-encoded data to destination
- `disconnect`: Close the circuit

**Sync Actions** (response to events):

- `send_tor_data`: Send hex-encoded data in response to received data
- `wait_for_more`: Queue current data and wait for more before responding

### LLM Prompt Example

```
You are controlling a Tor client connected to example.com:80.
Your instruction: "Send HTTP GET request for / and analyze response"

Available actions:
- send_tor_data: Send data (hex-encoded)
- disconnect: Close connection
- wait_for_more: Wait for more data

Event: tor_connected
Target: example.com:80

What action do you take?
```

## Directory Query Capabilities (NEW)

The Tor client can now query the Tor network directory (consensus) for relay information using Arti's experimental API.

### Arti Integration

Uses Arti's `experimental-api` feature to access the directory manager:
- `tor_client.dirmgr()` - Access directory manager
- `dirmgr.netdir()` - Get current network directory (NetDir)
- `netdir.relays()` - Iterate over all relays in consensus

### Dependencies

- `tor-dirmgr` v0.36 - Directory manager types
- `tor-netdir` v0.36 - Network directory queries and relay iteration

### Directory Actions

**get_consensus_info** - Get consensus metadata:
```json
{
  "type": "get_consensus_info"
}
```
Returns: relay_count, valid_after, fresh_until, valid_until timestamps

**list_relays** - List all relays (with limit):
```json
{
  "type": "list_relays",
  "limit": 50
}
```
Returns: Array of RelayInfo (nickname, fingerprint, flags, etc.)

**search_relays** - Filter relays by criteria:
```json
{
  "type": "search_relays",
  "flags": ["Guard", "Exit", "Fast"],
  "nickname": "example",
  "limit": 20
}
```
Returns: Matching relays with specified flags and/or nickname pattern

### RelayInfo Structure

Each relay returned contains:
- `nickname`: Relay nickname (string)
- `fingerprint`: RSA identity fingerprint (string)
- `flags`: Array of flags (Guard, Exit, Fast, Stable, Running, Valid)
- `is_guard`: Boolean flag indicators
- `is_exit`, `is_fast`, `is_stable`, `is_running`, `is_valid`

### Events

**`tor_bootstrap_complete`**: Triggered after consensus downloaded
- Parameters: `relay_count`, `valid_after`
- LLM can immediately query directory information

### Use Cases

1. **Network Analysis**: Analyze Tor network topology and relay distribution
2. **Relay Research**: Study relay flags, bandwidth, and availability
3. **Circuit Planning**: Choose specific relays before building circuits (future enhancement)
4. **Testing**: Verify consensus format and relay data from `tor_directory` server
5. **Monitoring**: Track relay count and consensus validity times

### Implementation Details

**State Storage**: `ArtiClient` instances stored in `AppState` after bootstrap
- Enables directory queries without active connection
- Stored by client_id for per-client consensus access

**Query Methods** (in `TorClient`):
- `get_netdir()` - Returns `Arc<NetDir>` from Arti's directory manager
- `query_relays()` - Filters relays using `RelayFilter` criteria
- `get_consensus_info()` - Extracts consensus metadata (relay count, validity)

**Filter Criteria** (`RelayFilter`):
- `flags`: Required relay flags (e.g., ["Guard", "Exit"])
- `nickname_pattern`: Substring match on nickname
- `limit`: Maximum results to return

### Limitations

- **Requires experimental-api**: Arti's directory access API may change in future versions
- **Read-only**: Cannot modify consensus or inject relay data
- **No signature control**: Arti automatically verifies consensus signatures
- **Consensus staleness**: Directory queries reflect Arti's cached consensus (updated hourly)

### Example Usage

**Query consensus after bootstrap**:
```
open_client tor "example.com:80" "After connecting, list all exit relays in the network"
```

**LLM Flow**:
1. Event: `tor_bootstrap_complete` (relay_count: 7234)
2. LLM Action: `search_relays` with flags: ["Exit", "Fast"]
3. Result: 1523 exit relays
4. LLM analyzes relay distribution, chooses exit node criteria

**Network analysis without connection**:
```
open_client tor "unused:80" "Don't connect anywhere. Just analyze the Tor network: show distribution of guard vs exit relays, and report the top 10 fastest relays"
```

## Tor-Specific Features

### Onion Services

The client automatically supports `.onion` addresses:

```rust
TorClient::connect("exampleonion3sd.onion:80").await?
```

Arti handles:

- Hidden service descriptor lookups
- Introduction point connections
- Rendezvous point circuits

### Circuit Isolation

Arti provides automatic circuit isolation:

- Each `TorClient` instance uses isolated circuits
- For additional isolation, use `TorClient::isolated_client()`
- Prevents traffic correlation between different connections

### Exit Node Selection

Currently uses Arti's default exit node selection:

- Weighted by bandwidth and flags
- Avoids bad exits (via consensus)
- Future enhancement: LLM control via `StreamPrefs`

### DNS Resolution

All DNS resolution happens through Tor:

- Prevents DNS leaks
- `connect()` accepts hostname:port, not IP addresses
- DNS queries sent through exit node

## Limitations

1. **Bootstrap Time**: Initial connection takes 10-30 seconds to fetch consensus
    - Mitigated: Arti caches consensus between runs
    - Future: Show bootstrap progress to user

2. **Performance**: Tor adds latency (3+ hops)
    - Typical latency: 100-500ms
    - Bandwidth: Limited by slowest relay

3. **No Direct Circuit Control**: Arti abstracts circuit management
    - Can't manually select specific relays
    - Can't force circuit rebuild (yet)
    - Exit node selection uses Arti's defaults

4. **Binary Data Only**: LLM works with hex-encoded data
    - Same pattern as TCP client
    - Works well for text protocols (HTTP, IRC, etc.)
    - Less ideal for complex binary protocols

5. **No Bridge Support Yet**: No pluggable transport support in this implementation
    - Arti supports bridges, but not exposed in our API
    - Future enhancement

6. **Connection Identification**: No true local address
    - Returns dummy `127.0.0.1:0` as local_addr
    - Tor connections don't have real local sockets

## Security Considerations

### What Tor Provides

- **Anonymity**: Hides client IP from destination
- **Untraceability**: Difficult to link multiple connections
- **Censorship Resistance**: Can access blocked sites

### What Tor Doesn't Provide

- **End-to-End Encryption**: Use HTTPS/TLS on top of Tor
- **Traffic Content Privacy**: Exit nodes can see unencrypted traffic
- **Malware Protection**: Tor doesn't scan for malware
- **DNS Security**: Exit node handles DNS (use DoH on top of Tor for more privacy)

### LLM Considerations

- LLM has full control over data sent through Tor
- LLM can leak identity through application-layer data (e.g., including real IP in HTTP headers)
- Instruction should emphasize privacy if needed

## Testing Strategy

See `tests/client/tor/CLAUDE.md` for testing details.

### Local Testing

1. **Arti Bootstrap**: First run downloads ~1-3MB consensus
2. **Test Destinations**: Use public test sites or local onion services
3. **LLM Budget**: Keep bootstrap time in mind (doesn't count toward LLM calls)

### Public Test Sites

- `check.torproject.org`: Verifies you're using Tor
- `httpbin.org`: HTTP testing (accessible through Tor)
- DuckDuckGo onion: `duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion`

## Future Enhancements

1. **Bootstrap Progress**: Show directory fetch progress to user
2. **Bridge Support**: Add pluggable transport configuration
3. **Circuit Control**: Expose circuit rebuild, exit node selection via StreamPrefs
4. **Onion Service Hosting**: Add server-side onion service support
5. **Persistent Identity**: Maintain client identity across restarts
6. **Hidden Service Client Auth**: Support client authentication for private onion services

## Dependencies

- `arti-client` v0.36: Main Tor client library
- `tor-rtcompat` v0.36: Runtime compatibility layer (Tokio integration)

Both are official Tor Project crates, actively maintained.

## References

- [Arti Documentation](https://tpo.pages.torproject.net/core/doc/rust/arti_client/)
- [Tor Protocol Specification](https://spec.torproject.org/)
- [Onion Services](https://community.torproject.org/onion-services/)
