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

## Architecture Notes

### Connection Management

**Current Implementation** (in `main.rs`):

The main application uses a shared connection map to manage TcpStream write halves:

```rust
// In main.rs, line 87-88
type WriteHalfMap = Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;
let connections: WriteHalfMap = Arc::new(Mutex::new(HashMap::new()));

// When accepting connections (line 154-159)
let (read_half, write_half) = tokio::io::split(stream);
let write_half_arc = Arc::new(Mutex::new(write_half));
connections.lock().await.insert(connection_id, write_half_arc);

// Read task spawned separately (line 169-200)
tokio::spawn(async move {
    let mut buffer = vec![0u8; 8192];
    loop {
        match read_half.read(&mut buffer).await {
            Ok(n) => { /* send DataReceived event */ }
            // ...
        }
    }
});

// Write using shared HashMap (line 287-297)
if let Some(write_half_arc) = connections.lock().await.get(connection_id) {
    let mut write_half = write_half_arc.lock().await;
    write_half.write_all(response.as_bytes()).await?;
    write_half.flush().await?;
}
```

**Note**: The `EventHandler::add_connection()` method exists but is NOT used by main.rs. Instead, main.rs manages its own connection map directly. This is intentional to keep the connection management in the main event loop.

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

## User Interface Features

### Command History

- **Up/Down arrows**: Navigate through previous commands
- **Automatic deduplication**: Doesn't save duplicate consecutive commands
- **Smart browsing**: Typing while in history mode exits to current input
- **Visual indicator**: Title shows "History N/M" when browsing
- **Persistent storage**: History saved to `~/.netget_history`
- **Auto-load**: Previous commands loaded on startup
- **Auto-save**: History written on clean exit (Ctrl+C or quit)

### Multi-line Input

- **Shift+Enter**: Insert newline character for multi-line commands
- **Enter**: Submit command as usual
- **Smart cursor**: Tracks cursor position across multiple lines

### Shell-like Keybindings

- **Ctrl+A**: Move to start of line
- **Ctrl+E**: Move to end of line
- **Ctrl+K**: Delete from cursor to end of line
- **Ctrl+W**: Delete word before cursor
- **Ctrl+U**: Clear entire line
- **Home/End**: Alternative start/end navigation
- **Ctrl+C**: Quit application

### CLI Arguments

You can pass a command as a CLI argument to execute immediately on startup:

```bash
# Start and immediately listen on port 21 via FTP
netget "listen on port 21 via ftp"

# The command is executed before entering the TUI
```

This is useful for scripting and automation.

## Recent Fixes

### 1. LLM Response Formatting (Fixed)

**Problem**: LLM was returning debug-formatted responses like `b"ack\n"` instead of raw text `ack\n`.

**Solution**:
- Updated prompts to explicitly instruct LLM to return raw text, not debug representations
- Added `process_llm_response()` function in both `main.rs` and `events/handler.rs` that:
  - Strips `b"..."` wrapping if present
  - Unescapes `\n`, `\r`, `\t` sequences
  - Logs warnings when fixups are applied

**Location**: `src/main.rs:25-54`, `src/events/handler.rs:37-66`, `src/llm/prompt.rs:51-63,145-152`

### 2. TUI Log Interference (Fixed)

**Problem**: Tracing logs were written to stderr, garbling the TUI display.

**Solution**:
- Logging now only enabled with `--debug` flag
- When enabled, logs go to `netget.log` file instead of stderr
- Disabled ANSI colors in log file output
- Added `netget.log` to `.gitignore`
- No-op subscriber initialized when debug is disabled

**Location**: `src/main.rs:71-108`

### 3. Command History Persistence (Implemented)

**Feature**: Command history persists across sessions in `~/.netget_history`.

**Implementation**:
- `App::load_history()` reads from `~/.netget_history` on startup
- `App::save_history()` writes to `~/.netget_history` on exit
- History loaded in `App::new()` constructor
- History saved in main event loop cleanup (before terminal restore)
- Uses `dirs` crate for cross-platform home directory detection
- Gracefully handles missing files and I/O errors

**Location**: `src/ui/app.rs:79-135`, `src/main.rs:500-503`

## Known Limitations

1. **No concurrent connections**: EventHandler processes events sequentially
2. **No streaming LLM**: Each response waits for full LLM generation
3. **No error recovery**: LLM errors just log, don't retry
4. **No TLS/SSL**: Plain TCP only
5. **No UDP support**: TCP only

## Future Work

### High Priority

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
2. **Connection management**: Split streams with `tokio::io::split()` and use `Arc<Mutex<WriteHalf>>`
3. **Testing**: Separate unit tests (no Ollama) from integration tests (requires Ollama)
4. **Concurrency**: Avoid deadlocks by splitting read/write or using proper locking patterns
5. **Model**: qwen3-coder:30b optimized for protocol implementation
6. **UI**: Midnight Commander blue theme for visibility
7. **Events**: Channel-based architecture with async event processing
8. **UX**: Shell-like features (history, multi-line, keybindings) improve usability
9. **CLI**: Support for initial command argument enables scripting
10. **Logging**: Logs to file (netget.log) to avoid garbling TUI
11. **History persistence**: Commands saved to ~/.netget_history across sessions
