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
