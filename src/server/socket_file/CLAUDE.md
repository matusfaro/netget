# Socket File Protocol Implementation

## Overview

Unix domain socket server implementing raw socket file handling where the LLM has full control over the byte stream.
This protocol enables inter-process communication (IPC) using filesystem socket files instead of TCP IP:port addresses.

**Status**: Experimental (Core Protocol)
**Platform**: Linux/Unix/macOS only (uses Unix domain sockets, not available on Windows)
**Compilation**: Gated with `#[cfg(unix)]` - will not compile on non-Unix platforms

## Library Choices

- **tokio::net::UnixListener** - Async Unix domain socket server from Tokio runtime
- **tokio::net::UnixStream** - Async Unix stream socket
- **tokio::io::{AsyncReadExt, AsyncWriteExt}** - Async I/O traits for reading/writing
- **Manual byte-level handling** - LLM receives raw bytes and constructs responses

**Rationale**: Unix domain sockets are a standard IPC mechanism on Unix-like systems, offering lower overhead than TCP
for local communication. The implementation mirrors TCP but uses filesystem paths for addressing.

## Architecture Decisions

### 1. Raw Byte Control

Identical to TCP - the LLM receives the exact bytes sent by the client and can respond with any byte sequence. This
allows the LLM to:

- Implement any IPC protocol
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

- Used for protocols that send greeting on connection
- Triggers `SOCKET_FILE_CONNECTION_OPENED_EVENT` which the LLM handles
- Banner is sent immediately after connection acceptance

### 5. Stream Splitting

UnixStream is split into `(ReadHalf, WriteHalf)` using `tokio::io::split()`:

- **ReadHalf**: Owned by dedicated reader task
- **WriteHalf**: Wrapped in `Arc<Mutex<WriteHalf>>` and stored in connection map
- This allows concurrent reading and writing without cloning the stream

### 6. Socket File Management

- **Creation**: Socket file is created at the specified path when server starts
- **Cleanup**: Existing socket file is removed before binding (if present)
- **Permissions**: Socket file inherits directory permissions
- **Platform**: Unix/Linux only - not supported on Windows

### 7. Dual Logging

All data operations use **dual logging**:

- **DEBUG**: Data summary with 100-char preview (for both text and binary)
- **TRACE**: Full payload (text as string, binary as hex)
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model

The LLM responds to socket file events with actions:

**Events**:

- `socket_file_connection_opened` - New connection accepted (only if `send_first=true`)
- `socket_file_data_received` - Data received from client

**Available Actions**:

- `send_socket_data` - Send raw bytes to client (text or hex)
- `close_connection` - Close the connection
- `wait_for_more` - Enter Accumulating state to buffer more data
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Response

```json
{
  "actions": [
    {
      "type": "send_socket_data",
      "data": "ACK: Message received\n"
    },
    {
      "type": "show_message",
      "message": "Sent acknowledgment via socket file"
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

1. **Accept**: `UnixListener::accept()` creates new connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo`
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
- Socket path stored in `protocol_info` field

## Startup Parameters

### Required Parameters

- **socket_path** (string): Filesystem path for the Unix domain socket file
    - Example: `./netget.sock`
    - Must be a valid path with write permissions
    - Existing socket files are removed automatically

### Optional Parameters

- **send_first** (boolean, default: false): Send banner on connection
    - If true, LLM generates initial message via `socket_file_connection_opened` event

## Known Limitations

### 1. Platform-Specific

- Unix/Linux only - not supported on Windows
- Requires Unix domain socket support in the OS

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
- Socket half-close not supported

### 6. No Credential Passing

- Unix domain sockets can pass credentials (PID, UID, GID)
- This feature is not currently exposed to the LLM

### 7. Dummy SocketAddr

- Internal APIs expect `SocketAddr` but Unix sockets use paths
- Uses dummy address `127.0.0.1:0` internally
- Actual socket path stored in protocol_info

## Example Prompts

### Echo Server

```
Create socket file at ./echo.sock
When you receive any data, echo it back exactly
```

### Line-Based Protocol

```
Listen on socket file ./myapp.sock
Wait for complete lines (ending with \n)
Respond with "OK: <line>\n" for each line received
```

### Binary Protocol

```
Create socket at ./binary.sock
Receive 4-byte big-endian integers
Respond with the integer doubled, also as 4-byte big-endian
Use hex encoding for binary data
```

### Greeting Banner

```
Listen on ./greeter.sock with send_first=true
Send "READY\n" when client connects
Then wait for commands: HELLO or QUIT
HELLO -> respond "GREETINGS\n"
QUIT -> respond "BYE\n" and close connection
```

### IPC Command Server

```
Socket file: ./commands.sock
Wait for JSON commands: {"action": "ping"} or {"action": "status"}
Respond with JSON: {"result": "pong"} or {"result": "ok"}
```

## Performance Characteristics

### Latency

- One LLM call per received data chunk (unless using `wait_for_more`)
- Typical latency: 2-5 seconds per request with qwen3-coder:30b
- Lower overhead than TCP (no network stack)

### Throughput

- Limited by LLM response time
- Concurrent connections processed in parallel (each on separate tokio task)
- Queue mechanism prevents data loss but doesn't improve throughput

### Concurrency

- Unlimited concurrent connections (bounded by system resources)
- Each connection has independent state and processing
- Ollama lock serializes LLM API calls across all connections

## Comparison to TCP

### Similarities

- Same connection state machine
- Same data queueing mechanism
- Same LLM integration model
- Same action/event system

### Differences

- **Addressing**: Socket file paths instead of IP:port
- **Platform**: Unix/Linux only (TCP is cross-platform)
- **Performance**: Lower overhead for local IPC
- **Security**: Filesystem permissions instead of firewall rules
- **Discovery**: File-based (no DNS/service discovery)

## Use Cases

### 1. Inter-Process Communication

- Local services communicating without network overhead
- Process coordination and control

### 2. Development/Testing

- Testing protocol implementations locally
- Debugging without network exposure

### 3. Container Communication

- Communication between containers on same host
- Lower latency than TCP localhost

### 4. Unix Tool Integration

- Integration with traditional Unix tools (nc -U, socat, etc.)
- Shell scripts and command-line clients

## References

- [Unix Domain Sockets (man 7 unix)](https://man7.org/linux/man-pages/man7/unix.7.html)
- [Tokio UnixListener](https://docs.rs/tokio/latest/tokio/net/struct.UnixListener.html)
- [Tokio UnixStream](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html)
