# TCP Protocol Implementation

## Overview

TCP server implementing raw TCP socket handling where the LLM has full control over the byte stream. This is the most
fundamental protocol in NetGet - the LLM can construct any TCP-based protocol on top of it (FTP, HTTP, SMTP, custom
protocols, etc.).

**Status**: Beta (Core Protocol)
**RFC**: RFC 793 (Transmission Control Protocol)

## Library Choices

- **tokio::net::TcpListener** - Async TCP server from Tokio runtime
- **tokio::io::{AsyncReadExt, AsyncWriteExt}** - Async I/O traits for reading/writing
- **Manual byte-level handling** - LLM receives raw bytes and constructs responses

**Rationale**: No high-level TCP library needed. The LLM directly controls the byte stream, making this the most
flexible protocol implementation.

## Architecture Decisions

### 1. Raw Byte Control

The LLM receives the exact bytes sent by the client and can respond with any byte sequence. This allows the LLM to:

- Implement any text protocol (FTP, SMTP, POP3, custom protocols)
- Handle binary protocols
- Mix text and binary data
- Create custom protocol parsers

### 2. Connection State Machine

Each connection has three states:

- **Idle**: Ready to process new data
- **Processing**: LLM is generating a response
- **Accumulating**: LLM requested `wait_for_more` to accumulate additional data

This prevents concurrent LLM calls for the same connection and ensures ordered processing.

### 3. Data Queueing

When data arrives while the LLM is processing:

1. Data is queued in `ConnectionData.queued_data`
2. After LLM response, queued data is merged and processed
3. Loop continues until all queued data is processed

This ensures no data loss even under high traffic.

### 4. Optional Banner Support

The `send_first` parameter allows servers to send a banner before receiving client data:

- Used for protocols like FTP, SMTP that send greeting on connection
- Triggers `TCP_CONNECTION_OPENED_EVENT` which the LLM handles
- Banner is sent immediately after connection acceptance

### 5. Stream Splitting

TcpStream is split into `(ReadHalf, WriteHalf)` using `tokio::io::split()`:

- **ReadHalf**: Owned by dedicated reader task
- **WriteHalf**: Wrapped in `Arc<Mutex<WriteHalf>>` and stored in connection map
- This allows concurrent reading and writing without cloning the stream

### 6. Dual Logging

All data operations use **dual logging**:

- **DEBUG**: Data summary with 100-char preview (for both text and binary)
- **TRACE**: Full payload (text as string, binary as hex)
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model

The LLM responds to TCP events with actions:

**Events**:

- `tcp_connection_opened` - New connection accepted (only if `send_first=true`)
- `tcp_data_received` - Data received from client

**Available Actions**:

- `send_tcp_data` - Send raw bytes to client (text or hex)
- `close_connection` - Close the connection
- `wait_for_more` - Enter Accumulating state to buffer more data
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Response

```json
{
  "actions": [
    {
      "type": "send_tcp_data",
      "data": "220 Welcome to NetGet FTP Server\r\n"
    },
    {
      "type": "show_message",
      "message": "Sent FTP greeting"
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

1. **Accept**: `TcpListener::accept()` creates new connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Tcp`
3. **Split**: Stream split into ReadHalf and WriteHalf
4. **Track**: WriteHalf stored in `connections` HashMap with `ConnectionData`
5. **Handle**: Separate tasks for reading and LLM processing
6. **Close**: Connection removed from maps when client disconnects or LLM closes

### Connection Data Structure

```rust
struct ConnectionData {
    state: ConnectionState,           // Idle/Processing/Accumulating
    queued_data: Vec<u8>,              // Data queued while processing
    memory: String,                    // Per-connection memory (unused currently)
    write_half: Arc<Mutex<WriteHalf>>, // For sending responses
}
```

### State Updates

- Connection state tracked in `ServerInstance.connections`
- Updates include: bytes sent/received, packets sent/received, last_activity
- UI automatically refreshes on state changes via `__UPDATE_UI__` message

## Known Limitations

### 1. No TLS Support

- Raw TCP only, no built-in TLS/SSL
- For HTTPS/FTPS, use the HTTP protocol with TLS or implement TLS manually

### 2. No Connection Timeouts

- Connections remain open indefinitely until client closes or LLM sends `close_connection`
- No idle timeout mechanism

### 3. No Backpressure Handling

- All received data is processed immediately
- Large bursts of data may overwhelm the LLM processing queue

### 4. Memory Accumulation

- `wait_for_more` accumulates data in memory without limits
- Long-running accumulating connections could exhaust memory

### 5. No Half-Close Support

- Closing a connection closes both read and write directions
- TCP half-close (shutdown write but keep reading) not supported

## Example Prompts

### FTP Server

```
listen on port 21 via ftp
When a client connects, send "220 NetGet FTP Server\r\n"
Handle USER command: respond with "331 Password required\r\n"
Handle PASS command: respond with "230 Login successful\r\n"
Handle PWD command: respond with "257 \"/home/user\"\r\n"
Handle QUIT command: respond with "221 Goodbye\r\n" and close connection
```

### Echo Server

```
listen on port 7 via tcp
When you receive any data, echo it back with "ACK: " prefix
```

### Binary Protocol

```
listen on port 9000 via tcp
When you receive a 4-byte big-endian integer, respond with the integer + 1
Use hex encoding for binary data
```

### Stateful Protocol

```
listen on port 8080 via tcp
Wait for command: HELLO, START, or STOP
After HELLO, send "READY\r\n"
After START, send "RUNNING\r\n"
After STOP, send "STOPPED\r\n" and close connection
```

## Performance Characteristics

### Latency

- One LLM call per received data chunk (unless using `wait_for_more`)
- Typical latency: 2-5 seconds per request with qwen3-coder:30b

### Throughput

- Limited by LLM response time
- Concurrent connections processed in parallel (each on separate tokio task)
- Queue mechanism prevents data loss but doesn't improve throughput

### Concurrency

- Unlimited concurrent connections (bounded by system resources)
- Each connection has independent state and processing
- Ollama lock serializes LLM API calls across all connections

## References

- [RFC 793: Transmission Control Protocol](https://datatracker.ietf.org/doc/html/rfc793)
- [Tokio TcpListener](https://docs.rs/tokio/latest/tokio/net/struct.TcpListener.html)
- [Tokio AsyncReadExt](https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html)
- [Tokio AsyncWriteExt](https://docs.rs/tokio/latest/tokio/io/trait.AsyncWriteExt.html)
