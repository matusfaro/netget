# NetGet - LLM-Controlled Network Application

A Rust CLI application where an LLM (via Ollama) controls network protocols and acts as a server (or client) for various protocols like FTP, HTTP, or custom protocols. The application provides only the TCP/IP stack - all protocol logic is handled by the LLM.

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────────┐
│                   Interactive TUI                        │
│  ┌──────────────┐  ┌────────────┐  ┌────────────────┐  │
│  │ User Input   │  │ LLM        │  │ Connection     │  │
│  │              │  │ Responses  │  │ Info & Stats   │  │
│  └──────────────┘  └────────────┘  └────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │         Status / Activity Log                     │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                    Event System                          │
│  - User Commands                                         │
│  - Network Events (data received/sent, connections)      │
│  - LLM Coordination                                      │
└─────────────────────────────────────────────────────────┘
                            │
         ┌──────────────────┴──────────────────┐
         ▼                                     ▼
┌──────────────────┐               ┌──────────────────────┐
│   TCP/IP Stack   │               │   Ollama LLM Client  │
│  - TcpServer     │               │  - Prompt Generation │
│  - Connections   │               │  - Response Parsing  │
│  - Packets       │               │                      │
└──────────────────┘               └──────────────────────┘
```

### Module Structure

- **`ui/`** - Full-screen terminal interface with ratatui
  - `app.rs` - Application state and rendering
  - `layout.rs` - 4-panel layout management
  - `events.rs` - Terminal input handling

- **`network/`** - TCP/IP stack implementation
  - `tcp.rs` - Async TCP server
  - `connection.rs` - Connection management
  - `packet.rs` - Packet representation

- **`protocol/`** - Protocol type definitions (NO implementations)
  - Only defines protocol types (FTP, HTTP, Custom)
  - All protocol logic is handled by LLM

- **`state/`** - Application state management
  - `app_state.rs` - Global state (mode, protocol, connections, instructions)
  - `machine.rs` - Generic state machine utilities

- **`llm/`** - Ollama integration
  - `client.rs` - Ollama API client
  - `prompt.rs` - Prompt generation for different scenarios

- **`events/`** - Event coordination
  - `types.rs` - Event and command definitions
  - `handler.rs` - LLM-driven event processing

## How It Works

### 1. User Issues Command

User types a command like:
```
listen on port 21 via FTP protocol and serve a single file data.txt with content 'hello'
```

### 2. State Management

The application:
- Sets mode to Server
- Sets protocol type to FTP
- Stores user instruction in memory

### 3. TCP Connection Established

When a client connects:
- Application asks LLM: "What initial data should we send?" (provides context: FTP protocol, user instructions)
- LLM generates FTP welcome message: `220 NetGet FTP Server Ready\r\n`
- Application sends the exact bytes

### 4. Data Received

When data arrives from client:
- Application asks LLM: "Client sent: `USER anonymous\r\n`. What should we respond?"
- LLM generates: `331 Password required\r\n`
- Application sends response

### 5. Continuous Interaction

This pattern continues for all network events:
- LLM reads current state
- LLM reads user instructions
- LLM reads received data
- LLM decides: send response / close connection / no response

## Prerequisites

- Rust (latest stable)
- [Ollama](https://ollama.ai/) running locally
- An Ollama model (e.g., `llama3.2:latest`)

```bash
# Install Ollama
curl https://ollama.ai/install.sh | sh

# Pull a model
ollama pull llama3.2:latest
```

## Building

```bash
cargo build --release
```

## Testing

NetGet has both **unit tests** and **integration tests**:

### Unit Tests (no Ollama required)
```bash
cargo test --lib
```

### Integration Tests (requires Ollama)

Integration tests verify the full system with real FTP clients and LLM interaction.

```bash
# Start Ollama first
ollama serve
ollama pull deepseek-coder:latest

# In another terminal, run all tests
cargo test

# Or run only integration tests
cargo test --test ftp_integration_test
```

**Note**: Integration tests will fail if Ollama is not running. This is expected behavior.

See [`tests/README.md`](tests/README.md) for detailed testing documentation.

## Running

```bash
# Start Ollama (if not running)
ollama serve

# Run NetGet interactively
cargo run

# Run with debug logging enabled
cargo run -- --debug

# Pass a command directly (executes before entering TUI)
cargo run -- "listen on port 21 via ftp"

# Combine flags
cargo run -- --debug "listen on port 21 via ftp"
```

### UI Architecture

NetGet uses a **rolling terminal interface** (like `tail -f`):

- **Output Scrolling**: Server logs and LLM responses scroll naturally into your terminal's scrollback buffer
- **Sticky Footer**: Input field and status bar remain fixed at the bottom
- **Status Bar**: Shows model name, scripting mode (LLM/Python/JavaScript), web search toggle, and packet statistics
- **Log Levels**: ERROR, WARN, INFO, DEBUG, TRACE (toggle with Ctrl+L)
- **Natural Navigation**: Use your terminal's native scrollback (scrollbar, page up/down)

### Shell-like Features

NetGet provides a rich command-line interface with familiar keybindings:

#### Command History
- **Up/Down arrows**: Navigate through previous commands
- **History indicator**: Title bar shows "History N/M" when browsing
- **Smart editing**: Start typing to exit history and edit current input
- **Persistent storage**: History automatically saved to `~/.netget_history` on exit
- **Cross-session**: Previous commands are loaded when you restart NetGet

#### Multi-line Input
- **Shift+Enter**: Insert newline for multi-line commands
- **Enter**: Submit command
- **Smart cursor**: Tracks position across multiple lines

#### Keybindings
- **Ctrl+A**: Move to start of line
- **Ctrl+E**: Move to end of line
- **Ctrl+K**: Delete from cursor to end of line
- **Ctrl+W**: Toggle web search on/off
- **Ctrl+U**: Clear entire input
- **Ctrl+L**: Cycle log level (ERROR → WARN → INFO → DEBUG → TRACE)
- **Home/End**: Jump to line start/end
- **Ctrl+C**: Quit application

#### CLI Arguments

Execute commands immediately on startup:

```bash
# Start server with single command
netget "listen on port 21 via ftp"

# Useful for scripting and automation
```

## Usage Examples

### Example 1: FTP Server

```
> listen on port 21 via FTP protocol
> Also serve a single file data.txt with content 'hello world'
```

The LLM will:
- Send FTP welcome messages
- Handle USER, PASS, PWD, LIST, RETR commands
- Serve the file as instructed

### Example 2: Echo Server

```
> listen on TCP port 1234, and echo back everything that is sent to you
```

### Example 3: Question Answering Server

```
> listen on TCP port 12121, expect text to be sent to you, try to answer the questions sent to you
```

### Example 4: HTTP Server

```
> listen on port 80 via HTTP
> Serve a simple HTML page with "Hello World"
```

### Managing Connections

```
> status                  # Show current state
> close                   # Close all connections
```

## Configuration

### Changing Ollama Model

The default model is `deepseek-coder:latest`.

**Change model at runtime** (recommended):
```
model deepseek-coder:latest
model llama3.2:latest
model codellama:latest
```

**Change default model** (requires rebuild):
```rust
// In src/state/app_state.rs
ollama_model: "llama3:latest".to_string(),
```

The current model is always displayed in the Connection Info panel.

### Ollama URL

Default is `http://localhost:11434`. Change in `src/llm/client.rs`:

```rust
pub fn default() -> Self {
    Self::new("http://your-ollama-host:11434")
}
```

## Prompt Engineering

The LLM receives detailed prompts for each event. See `src/llm/prompt.rs`:

- **`build_data_received_prompt`** - When data arrives
- **`build_connection_established_prompt`** - When connection opens
- **`build_status_prompt`** - For status explanations

Prompts include:
- Current mode (server/client)
- Protocol type (FTP, HTTP, Custom)
- All user instructions
- Connection state
- Received data

## Testing

### Test FTP Server

```bash
# Terminal 1: Start NetGet
cargo run

# Enter in NetGet UI:
listen on port 2121 via ftp

# Terminal 2: Connect with FTP client
ftp localhost 2121
# Try: USER anonymous, PASS test, PWD, SYST, LIST
```

### Test with netcat

```bash
# Terminal 1: Start NetGet, enter:
listen on port 5000

# Terminal 2:
nc localhost 5000
# Type messages and see LLM responses
```

## Project Structure

```
netget/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs              # Event loop orchestration
│   ├── lib.rs
│   ├── ui/                  # Full-screen TUI
│   │   ├── mod.rs
│   │   ├── app.rs
│   │   ├── layout.rs
│   │   └── events.rs
│   ├── network/             # TCP/IP stack
│   │   ├── mod.rs
│   │   ├── tcp.rs
│   │   ├── connection.rs
│   │   └── packet.rs
│   ├── protocol/            # Protocol types only
│   │   └── mod.rs
│   ├── state/               # State management
│   │   ├── mod.rs
│   │   ├── app_state.rs
│   │   └── machine.rs
│   ├── llm/                 # Ollama integration
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   └── prompt.rs
│   └── events/              # Event coordination
│       ├── mod.rs
│       ├── types.rs
│       └── handler.rs
```

## Key Design Decisions

### Why LLM-Only Protocol Handling?

1. **Flexibility**: Support any protocol without writing code
2. **Learning**: LLM can adapt to custom or undocumented protocols
3. **Natural Language Control**: Users describe behavior, not implementation
4. **Experimentation**: Test protocol variations easily

### Why Not Hardcode Protocols?

Traditional approach would implement FTP/HTTP/etc in Rust. Downsides:
- Rigid behavior
- Requires code changes for new protocols
- Can't easily modify behavior
- Users must understand implementation

### LLM Approach Benefits:

- User says: "Act as FTP server with file X"
- LLM generates exact FTP protocol responses
- Behavior changes with user instructions
- No code modifications needed

## Performance Considerations

- Each network event triggers an LLM call (can be slow)
- Suitable for experimentation, testing, learning
- Not for production high-throughput servers
- Future: Add caching for common protocol patterns

## Limitations (MVP)

- [ ] No client mode yet (only server)
- [ ] No streaming LLM responses
- [ ] No persistent file storage
- [ ] One connection at a time handled sequentially
- [ ] No TLS/SSL support
- [ ] No UDP support

## Future Enhancements

- **Client Mode**: LLM as protocol client
- **Multi-Protocol**: Layer protocols (TCP→HTTP→WebSocket)
- **Learning Mode**: Train on protocol traces
- **Replay Mode**: Record and replay sessions
- **UI Improvements**: Hex view, packet inspector
- **Performance**: Cache common responses

## Files Created

NetGet creates the following files:

- **`netget.log`** - Application logs (only created with `--debug` flag)
- **`~/.netget_history`** - Command history (always created)

Both files are safe to delete if you want to start fresh.

## Troubleshooting

### Debugging / Logs

By default, NetGet runs without logging to keep things clean. To enable debug logging:

```bash
# Run with debug logging
netget --debug

# Or with cargo
cargo run -- --debug
```

Logs are written to `netget.log` in the current directory. This prevents log messages from garbling the TUI.

```bash
# View logs in real-time
tail -f netget.log

# Search for errors
grep ERROR netget.log
```

### Ollama Connection Failed

```
Error: Failed to connect to Ollama
```

**Fix**: Ensure Ollama is running: `ollama serve`

### Model Not Found

```
Error: Model not found
```

**Fix**: Pull the model: `ollama pull llama3.2:latest`

### Port Already in Use

```
Error: Address already in use
```

**Fix**: Choose a different port or kill the process using that port.

## Contributing

This is an experimental project. Ideas for improvement:

1. Better prompt engineering
2. Protocol-specific prompt templates
3. Multi-connection handling improvements
4. Client mode implementation
5. WebSocket support

## License

MIT

## Acknowledgments

- Built with [tokio](https://tokio.rs/) for async runtime
- UI powered by [ratatui](https://github.com/ratatui-org/ratatui)
- LLM integration via [Ollama](https://ollama.ai/)
