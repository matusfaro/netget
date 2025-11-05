# HTTP/3 Protocol Implementation

## Overview
HTTP/3 server implementing multiplexed stream handling where the LLM has full control over individual streams. HTTP/3 is a UDP-based, encrypted transport protocol with built-in TLS 1.3, providing stream multiplexing without head-of-line blocking.

**Status**: Experimental
**RFC**: RFC 9000 (HTTP/3: A UDP-Based Multiplexed and Secure Transport)

## Library Choices
- **quinn v0.11** - Pure Rust async HTTP/3 implementation
- **rustls** - TLS 1.3 implementation (required by HTTP/3)
- **rcgen** - Self-signed certificate generation for TLS
- **tokio** - Async runtime for HTTP/3 endpoint and stream handling

**Rationale**: Quinn is the most mature pure-Rust HTTP/3 implementation with full async support and excellent tokio integration. It provides built-in TLS 1.3 encryption (mandatory in HTTP/3), automatic stream multiplexing, and flow control.

## Architecture Decisions

### 1. Stream-Based Control
The LLM controls individual bidirectional streams within a HTTP/3 connection:
- Each stream is a separate communication channel
- Streams are multiplexed over a single HTTP/3 connection
- No head-of-line blocking between streams
- Independent flow control per stream

### 2. Connection vs Stream Lifecycle
**HTTP/3 Connection**:
- Established with TLS 1.3 handshake
- Notifies LLM via `HTTP/3_CONNECTION_OPENED_EVENT`
- Persists across multiple streams

**HTTP/3 Stream**:
- Created by client opening bidirectional stream
- Notifies LLM via `HTTP/3_STREAM_OPENED_EVENT`
- Independent lifetime from connection
- Closed by client or LLM `close_this_stream` action

### 3. Stream State Machine
Each stream has three states:
- **Idle**: Ready to process new data
- **Processing**: LLM is generating a response
- **Accumulating**: LLM requested `wait_for_more` to accumulate additional data

This prevents concurrent LLM calls for the same stream and ensures ordered processing.

### 4. Data Queueing
When data arrives while the LLM is processing a stream:
1. Data is queued in `StreamData.queued_data`
2. After LLM response, queued data is merged and processed
3. Loop continues until all queued data is processed

This ensures no data loss even under high traffic.

### 5. TLS Certificate Management
HTTP/3 mandates TLS 1.3 encryption:
- Self-signed certificates generated at server startup using `rcgen`
- Certificate valid for "localhost"
- ALPN protocol: `h3`
- No client authentication required

### 6. Transport Configuration
- **Max concurrent bidirectional streams**: 100
- **Max concurrent unidirectional streams**: 100 (not used currently)
- Standard HTTP/3 flow control and congestion control

### 7. Dual Logging
All data operations use **dual logging**:
- **DEBUG**: Data summary with 100-char preview (for both text and binary)
- **TRACE**: Full payload (text as string, binary as hex)
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model
The LLM responds to HTTP/3 events with actions:

**Events**:
- `http3_connection_opened` - New HTTP/3 connection established (with TLS handshake)
- `http3_stream_opened` - Client opened a new bidirectional stream
- `http3_data_received` - Data received from client on a stream

**Available Actions**:
- `send_http3_data` - Send raw bytes to client on current stream (text or hex)
- `close_this_stream` - Close the current stream
- `wait_for_more` - Enter Accumulating state to buffer more data
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Response
```json
{
  "actions": [
    {
      "type": "send_http3_data",
      "data": "Hello from HTTP/3 stream\n"
    },
    {
      "type": "show_message",
      "message": "Sent HTTP/3 greeting"
    }
  ]
}
```

### Data Format
- **Text data**: Sent as-is in the `data` field
- **Binary data**: Sent as hex string (e.g., `"48656c6c6f"` for "Hello")
- **Received data**: Formatted as text if all bytes are ASCII printable, otherwise hex

## Connection Management

### Connection Lifecycle
1. **Accept**: `Endpoint::accept()` receives incoming HTTP/3 connection attempt
2. **TLS Handshake**: Quinn automatically performs TLS 1.3 handshake
3. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Http3`
4. **Notify**: LLM receives `http3_connection_opened` event
5. **Stream Loop**: Accept bidirectional streams from client
6. **Close**: Connection removed when client closes or error occurs

### Stream Lifecycle
1. **Accept**: `Connection::accept_bi()` receives new bidirectional stream
2. **Track**: Stream added to `streams` HashMap with `StreamData`
3. **Notify**: LLM receives `http3_stream_opened` event
4. **Read Loop**: Process data received on `RecvStream`
5. **Write**: Send responses via `SendStream`
6. **Close**: Stream removed when finished or LLM closes

### Stream Data Structure
```rust
struct StreamData {
    state: StreamState,               // Idle/Processing/Accumulating
    queued_data: Vec<u8>,             // Data queued while processing
    memory: String,                   // Per-stream memory (unused currently)
    send_stream: Arc<Mutex<SendStream>>, // For sending responses
}
```

### State Updates
- Connection state tracked in `ServerInstance.connections`
- `ProtocolConnectionInfo::Http3 { stream_count }` tracks active streams
- UI automatically refreshes on state changes via `__UPDATE_UI__` message

## Known Limitations

### 1. Unidirectional Streams Not Supported
- Only bidirectional streams are implemented
- Unidirectional streams would require additional event types and actions

### 2. No 0-RTT Support
- No early data (0-RTT) implementation
- All connections require full handshake

### 3. No Connection Migration
- Connections are bound to initial endpoint
- No support for HTTP/3's connection migration feature

### 4. No Stream Priorities
- All streams treated equally
- No priority/weight mechanism for streams

### 5. Memory Accumulation
- `wait_for_more` accumulates data in memory without limits
- Long-running accumulating streams could exhaust memory

### 6. No DATAGRAM Support
- HTTP/3 DATAGRAM frames not implemented
- Only reliable stream data

### 7. Self-Signed Certificates Only
- No support for custom certificates or ACME
- Clients must disable certificate validation

## Example Prompts

### Echo Server
```
listen on port 4433 via http3
When you receive data on any stream, echo it back
```

### HTTP/3-like Server
```
listen on port 443 via http3
Each stream represents an HTTP request
Read the request, respond with "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, HTTP/3!\n"
Close the stream after responding
```

### Multiplexed Chat Server
```
listen on port 5000 via http3
Stream 0: Control channel - handle "JOIN name" and "LEAVE" commands
Other streams: Message channels - broadcast messages to all connected clients
```

### Binary Protocol
```
listen on port 9000 via http3
When you receive a 4-byte big-endian integer, respond with the integer + 1
Use hex encoding for binary data
Each stream handles independent calculations
```

## Performance Characteristics

### Latency
- One LLM call per received data chunk per stream (unless using `wait_for_more`)
- TLS handshake adds ~1 RTT to connection establishment
- Stream multiplexing allows concurrent processing
- Typical latency: 2-5 seconds per request with qwen3-coder:30b

### Throughput
- Limited by LLM response time per stream
- Multiple streams can be processed concurrently (each on separate tokio task)
- No head-of-line blocking between streams
- Queue mechanism prevents data loss but doesn't improve throughput

### Concurrency
- Unlimited concurrent connections (bounded by system resources)
- Up to 100 concurrent streams per connection
- Each stream has independent state and processing
- Ollama lock serializes LLM API calls across all streams

### Comparison to TCP
**Advantages**:
- Stream multiplexing eliminates head-of-line blocking
- Built-in TLS 1.3 encryption (no separate TLS setup)
- 0-RTT capability (not currently implemented)
- Better congestion control than TCP

**Disadvantages**:
- UDP-based, may be blocked by some firewalls
- More complex implementation
- Higher CPU overhead due to encryption
- Clients need HTTP/3-capable libraries

## Security Considerations

### TLS 1.3 Mandatory
- All HTTP/3 connections are encrypted by default
- Self-signed certificate used (clients must trust it)
- No plaintext data transmission

### Certificate Validation
- Test clients must disable certificate validation
- Production deployments should use proper certificates

### ALPN Protocol
- Uses custom ALPN: `h3`
- Prevents accidental connection from HTTP/3 clients

## References
- [RFC 9000: HTTP/3: A UDP-Based Multiplexed and Secure Transport](https://datatracker.ietf.org/doc/html/rfc9000)
- [Quinn Documentation](https://docs.rs/quinn/)
- [HTTP/3 Transport](https://datatracker.ietf.org/doc/html/rfc9000)
- [HTTP/3 TLS](https://datatracker.ietf.org/doc/html/rfc9001)
- [HTTP/3 Recovery](https://datatracker.ietf.org/doc/html/rfc9002)
