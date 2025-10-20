# NetGet - Knowledge Document

This document captures key learnings, architectural decisions, and implementation details discovered during development and testing of NetGet.

## Project Overview

NetGet is a Rust CLI application where an LLM (via Ollama) controls network protocols. The application provides ONLY the TCP/IP stack - all protocol logic (FTP, HTTP, etc.) is handled by the LLM.

**Critical Design Principle**: No hardcoded protocol implementations. The LLM constructs every single datagram.

## Architecture

### Component Layers

```
┌─────────────────────────┐
│   TUI (ratatui)         │  - User input
│   4-panel interface     │  - LLM responses
└───────────┬─────────────┘  - Connection info
            │                - Status log
            ▼
┌─────────────────────────┐
│   Event System          │  - UserCommand
│   (mpsc channels)       │  - NetworkEvent
└───────────┬─────────────┘  - AppEvent
            │
      ┌─────┴─────┐
      ▼           ▼
┌──────────┐  ┌────────────┐
│ TCP/IP   │  │ EventHandler│
│ Stack    │  │ + LLM Client│
└──────────┘  └────────────┘
```

### Key Modules

1. **`ui/`** - Full-screen terminal UI
   - Midnight Commander blue theme
   - 4 panels: input, LLM responses, connection info, status

2. **`network/`** - TCP/IP stack ONLY
   - `tcp.rs` - TcpServer, accept(), listen()
   - `connection.rs` - ConnectionId tracking
   - NO protocol logic

3. **`protocol/`** - Protocol TYPE definitions only
   - `enum ProtocolType { Ftp, Http, Custom }`
   - NO implementations

4. **`state/`** - Application state
   - Mode (Server/Client/Idle)
   - Protocol type
   - Connection tracking
   - User instructions for LLM context

5. **`llm/`** - Ollama integration
   - `client.rs` - Ollama API calls
   - `prompt.rs` - Prompt builders for each event type

6. **`events/`** - Event coordination
   - `types.rs` - Event enums
   - `handler.rs` - LLM-driven event processing

## Critical Design Issue Found

### The Bug

**Location**: `src/events/handler.rs`

The `EventHandler` has a method `add_connection(connection_id, stream)` to register TcpStreams (line 289-291), but **this method is NEVER called anywhere in the codebase**.

**Impact**:
- When `Connected` or `DataReceived` events are processed, the handler tries to send data via `self.connections.get_mut(&connection_id)` (lines 85, 141)
- Since `self.connections` is always empty, responses are never sent
- The application cannot actually communicate with clients!

**Why it wasn't noticed**: The main application (`main.rs`) has the same bug - it also never registers connections.

### The Root Cause

The design has a fundamental concurrency issue:

1. `tcp::handle_connection()` receives a TcpStream and spawns a task to read from it
2. `EventHandler` needs the same TcpStream to write responses
3. But Rust's ownership rules prevent sharing mutable TcpStreams between tasks
4. The `add_connection` method was created but never called because there was no way to pass the stream safely

### The Solution

**For Production** (needs implementation in main.rs):

Use `tokio::io::split()` to separate read and write halves:

```rust
let (read_half, write_half) = tokio::io::split(stream);

// Pass read_half to handle_connection for reading
tokio::spawn(async move {
    tcp::handle_connection(read_half, ...);
});

// Store write_half in EventHandler for writing
event_handler.add_connection(connection_id, write_half);
```

**Alternative approach**:
```rust
// Use Arc<Mutex<WriteHalf>> for sharing across async tasks
let write_half_arc = Arc::new(Mutex::new(write_half));
connections.insert(connection_id, write_half_arc);
```

**For Tests** (implemented in `tests/ftp_integration_test.rs`):

```rust
// Split stream
let (read_half, write_half) = tokio::io::split(stream);

// Store write half in shared HashMap
let write_half_arc = Arc::new(Mutex::new(write_half));
connections.lock().await.insert(connection_id, write_half_arc.clone());

// Spawn read task
tokio::spawn(async move {
    let mut read_half = read_half;
    loop {
        match read_half.read(&mut buffer).await {
            // Read and send DataReceived events
        }
    }
});

// Event handler uses shared HashMap to write
let stream_arc_opt = connections.lock().await.get(connection_id).cloned();
if let Some(stream_arc) = stream_arc_opt {
    let mut stream = stream_arc.lock().await;
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
}
```

## Key Technical Learnings

### 1. TcpStream Sharing Problem

**Problem**: Can't clone or share TcpStream between read and write tasks

**Solutions**:
- ✅ `tokio::io::split(stream)` → `(ReadHalf, WriteHalf)`
- ✅ `Arc<Mutex<WriteHalf>>` for shared write access
- ❌ Trying to clone TcpStream (not implemented)
- ❌ Unsafe transmute hacks (undefined behavior)

### 2. Deadlock with Mutex + Blocking I/O

**Problem**: Read task holds lock while calling `stream.read().await` which blocks

**Scenario**:
1. Read task: `let mut stream = arc.lock().await; stream.read().await;` ← blocks here with lock held
2. Write task: `let mut stream = arc.lock().await;` ← waits forever for lock
3. Deadlock!

**Solution**: Split into separate read and write halves that don't share locks

### 3. LLM Integration Pattern

**When data arrives**:
```rust
let prompt = PromptBuilder::build_data_received_prompt(&state, connection_id, &data).await;
let response = llm.generate(&model, &prompt).await?;

if response.trim() != "NO_RESPONSE" && !response.is_empty() {
    stream.write_all(response.as_bytes()).await?;
}
```

**When connection established**:
```rust
let prompt = PromptBuilder::build_connection_established_prompt(&state, connection_id).await;
let response = llm.generate(&model, &prompt).await?;
// Send initial greeting (e.g., "220 FTP Server Ready")
```

### 4. Model Configuration

**Default model**: `qwen3-coder:30b` (configured in `src/state/app_state.rs:81`)

**Runtime switching**:
```
> model llama3.2:latest
> model deepseek-coder:latest
```

**Why qwen3-coder**: Optimized for protocol implementation and code generation

### 5. Testing Strategy

**Unit Tests** (`#[test]`):
- No Ollama required
- Test command parsing, protocol type detection
- Example: `test_user_command_parsing()`

**Integration Tests** (`tests/` directory):
- Requires Ollama running
- Uses real network clients (suppaftp for FTP)
- Tests full system: TCP → LLM → Protocol responses
- Example: `test_raw_tcp_connection()`, `test_ftp_server_basic_commands()`

**Dynamic Port Allocation**:
```rust
async fn get_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}
```
This avoids port conflicts and allows parallel test execution.

## Implementation Notes

### Event Flow

1. **User types command** → Parse → UserCommand event
2. **TCP connection arrives** → Accept → Connected event
3. **Data received** → Read → DataReceived event
4. **EventHandler processes event** → Calls LLM → Sends response

### Prompt Engineering

All prompts include:
- Current mode (Server/Client)
- Protocol type (FTP/HTTP/Custom)
- User instructions history
- Connection state
- Received data (for DataReceived events)

Example FTP prompt:
```
You are acting as an FTP server.
Protocol: FTP
Mode: Server
Instructions: Serve file data.txt with content: hello
Connection established. Generate the initial FTP greeting.
Respond with the exact bytes to send, or NO_RESPONSE if nothing should be sent.
```

### Color Scheme (Midnight Commander Blue)

All panels: Blue background (`Color::Blue`)
- **Input panel**: White on blue
- **LLM panel**: White on blue
- **Connection Info**: Cyan labels, white values on blue
- **Status**: Light cyan on blue
- **Borders**: Bold cyan

User explicitly requested this after initial white-on-black was not visible.

## Known Limitations

1. **Main application broken**: Same bug as found in EventHandler - never registers connections
2. **No concurrent connections**: EventHandler processes events sequentially
3. **No streaming LLM**: Each response waits for full LLM generation
4. **No error recovery**: LLM errors just log, don't retry
5. **No TLS/SSL**: Plain TCP only
6. **No UDP support**: TCP only

## Future Work

### High Priority - Fix Main Application

The main.rs needs the same fix as the tests:
1. Split TcpStream into read/write halves
2. Pass write half to EventHandler via add_connection()
3. Keep read half in connection handler task

### Medium Priority

- Implement connection pooling with Arc<Mutex<HashMap<>>>
- Add streaming LLM support for faster responses
- Add retry logic for LLM failures
- Support multiple concurrent connections properly

### Low Priority

- TLS/SSL support
- UDP protocol support
- WebSocket support
- Client mode implementation

## Testing Checklist

Before committing changes:

- [ ] Run unit tests: `cargo test --lib`
- [ ] Start Ollama: `ollama serve`
- [ ] Pull model: `ollama pull qwen3-coder:30b`
- [ ] Run integration tests: `cargo test --test ftp_integration_test test_raw_tcp_connection`
- [ ] Verify FTP test: `cargo test --test ftp_integration_test test_ftp_server_basic_commands`
- [ ] Check no hardcoded protocols: `grep -r "220 FTP" src/` should return nothing

## References

- Ollama API: http://localhost:11434
- Tokio docs: https://docs.rs/tokio
- Ratatui: https://github.com/ratatui-org/ratatui
- Test FTP client: https://docs.rs/suppaftp

## Learnings Summary

1. **Architecture**: LLM-only protocol handling, no hardcoded logic
2. **Bug found**: EventHandler never registers TcpStreams → responses never sent
3. **Solution**: Split streams with `tokio::io::split()` and use `Arc<Mutex<WriteHalf>>`
4. **Testing**: Separate unit tests (no Ollama) from integration tests (requires Ollama)
5. **Concurrency**: Avoid deadlocks by splitting read/write or using proper locking patterns
6. **Model**: qwen3-coder:30b optimized for protocol implementation
7. **UI**: Midnight Commander blue theme for visibility
8. **Events**: Channel-based architecture with async event processing
