# NetGet - Knowledge Document

NetGet is a Rust CLI application where an LLM (via Ollama) controls network protocols.

**Critical Design Principle**: The LLM is in control - either constructing raw protocol datagrams or generating high-level responses based on the chosen stack.

## Base Protocol Stacks

- **TCP** (`BaseStack::Tcp`) - LLM controls raw TCP data, constructs entire protocols (FTP, HTTP, custom)
- **HTTP** (`BaseStack::Http`) - LLM controls HTTP responses (status, headers, body); hyper handles parsing
- **UDP** (`BaseStack::Udp`) - LLM controls raw UDP datagrams
- **DataLink** (`BaseStack::DataLink`) - LLM controls layer 2 Ethernet frames (ARP, custom frames)
- **DNS** (`BaseStack::Dns`) - LLM generates DNS responses using hickory-dns
- **DHCP** (`BaseStack::Dhcp`) - LLM handles DHCP requests using dhcproto
- **NTP** (`BaseStack::Ntp`) - LLM handles time sync using ntpd-rs
- **SNMP** (`BaseStack::Snmp`) - LLM handles SNMP get/set using rasn-snmp
- **SSH** (`BaseStack::Ssh`) - LLM handles SSH auth and shell using russh
- **IRC** (`BaseStack::Irc`) - LLM handles IRC chat protocol

## Architecture

### Key Modules

- **`ui/`** - Ratatui TUI with 4 panels (input, LLM responses, connections, status). Midnight Commander blue theme.
- **`network/`** - Protocol implementations (`tcp.rs`, `http.rs`, `udp.rs`, etc.) with `*_actions.rs` for action handlers
- **`protocol/`** - Base stack definitions (`base_stack.rs`)
- **`state/`** - App state (mode, stack, connections, user instructions, memory)
- **`llm/`** - Ollama integration (`client.rs`, `prompt.rs`)
- **`events/`** - Event coordination (`types.rs`, `handler.rs`)
- **`llm/actions/`** - Action system (definitions, execution, protocol traits)

### Connection Management

TcpStream split with `tokio::io::split()` → `(ReadHalf, WriteHalf)`. Write halves stored in `Arc<Mutex<WriteHalf>>` HashMap. Read tasks spawn separately and send DataReceived events.

**Critical**: Never hold Mutex lock during blocking I/O (causes deadlock).

### Data Queueing System

Per-connection state machine prevents concurrent LLM calls:

**States**: Idle → Processing → Accumulating (if `wait_for_more`) → Idle

**Flow**:
1. Data arrives → spawn async task
2. Check state: if Processing → queue data; if Idle/Accumulating → process
3. Merge queued data, call LLM
4. Execute response actions (send data, close, wait for more)
5. Process queue or return to Idle

**Status channel**: Spawned tasks send UI updates via unbounded mpsc channel.

### Structured LLM Responses

**Legacy TCP response** (fallback):
```json
{"output": "220 Welcome\r\n", "close_connection": false, "wait_for_more": false, "shutdown_server": false, "log_message": "..."}
```

**HTTP response**:
```json
{"status": 200, "headers": {"Content-Type": "text/html"}, "body": "...", "log_message": "..."}
```

**Action response** (new):
```json
{"actions": [{"type": "send_tcp_data", "data": "hello"}, {"type": "show_message", "message": "Sent greeting"}]}
```

### Action-Based Prompt System

LLM returns `{actions: [...]}` instead of nested structures. Each action is self-describing.

**Action Categories**:
- **Common Actions**: Available everywhere (show_message, open_server, close_server, update_instruction, change_model, set_memory, append_memory)
- **Protocol Async Actions**: User can trigger anytime, no network context (TCP: send_to_connection, UDP: send_to_address, SNMP: send_trap)
- **Protocol Sync Actions**: Require network context (TCP: send_tcp_data, HTTP: send_http_response, UDP: send_udp_response, DNS/DHCP/NTP/SSH/IRC: protocol-specific responses)

**Implementation**: Each protocol implements `ProtocolActions` trait with:
- `get_async_actions(&self, state) -> Vec<ActionDefinition>`
- `get_sync_actions(&self, context) -> Vec<ActionDefinition>`
- `execute_action(&self, action, context) -> Result<ActionResult>`

**Prompt builders**:
- `build_user_input_action_prompt()` - Common + protocol async actions
- `build_network_event_action_prompt()` - Common subset + protocol sync actions

**Protocol action files**: `src/network/*_actions.rs`

## Testing

**Philosophy**: Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients.

**Unit tests**: No Ollama required. Test parsing and detection logic.

**Integration tests** (`tests/`): Require Ollama + model. Use real clients (suppaftp, reqwest, ssh2, raw sockets).

**Test helper**: `start_server_with_prompt(prompt)` infers configuration and returns (state, port, handle).

**Dynamic ports**: Use port 0 in prompts for auto-assignment.

**Running tests**:
```bash
cargo test --lib  # Unit tests
cargo test --test tcp_integration_test  # TCP/FTP tests
cargo test --test http_integration_test  # HTTP tests
cargo test --test e2e_ssh_test --features e2e-tests  # SSH/SFTP tests
```

## Logging (CRITICAL)

**Dual logging pattern** - ALL logs MUST go to BOTH:
1. Tracing macros (`debug!`, `trace!`, `error!`, `warn!`, `info!`) → `netget.log` file
2. Status channel (`status_tx.send()`) → TUI Status panel

**Required pattern**:
```rust
debug!("TCP sent {} bytes to {}", len, conn_id);
let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}", len, conn_id));
```

**Prefixes**: `[DEBUG]`, `[TRACE]`, `[ERROR]`, `[WARN]`, `[INFO]`
**User-facing symbols**: `→` (success), `✗` (failure), `✓` (confirmation)

**Levels**:
- ERROR - Critical failures
- WARN - Non-fatal issues
- INFO - Major lifecycle events (server start/stop, connections)
- DEBUG - Request/response summaries
- TRACE - Full payloads (pretty-printed JSON)

**Default**: INFO for non-interactive, TRACE for interactive (logged to `netget.log`)

## UI Features

- **Command history**: Up/Down arrows, saved to `~/.netget_history`, auto-deduplicated
- **Multi-line input**: Shift+Enter inserts newline, Enter submits
- **Keybindings**: Ctrl+A (start), Ctrl+E (end), Ctrl+K (delete to end), Ctrl+W (delete word), Ctrl+U (clear), Ctrl+C (quit)
- **CLI arguments**: `netget "listen on port 21 via ftp"` executes before TUI

## Key Technical Details

1. **TcpStream sharing**: Use `tokio::io::split()`, never clone
2. **Mutex deadlock**: Never hold lock during blocking I/O
3. **Default model**: `qwen3-coder:30b` (optimized for protocols)
4. **Model switching**: User can change via `model <name>` command
5. **Event flow**: UserCommand → Parse → EventHandler → LLM → Protocol action
6. **HTTP events**: Use oneshot channels for request-response pattern
7. **Status messages**: Async tasks update UI via unbounded channel
8. **Concurrent connections**: Each has own state, multiple can process simultaneously
9. **Action execution**: Sequential, in-order from action array

## SSH/SFTP Implementation

**Library**: russh 0.45 + russh-sftp 2.1
**Location**: `src/network/ssh.rs`, `src/network/sftp_handler.rs`

**SSH Shell Buffering**: Line-based with character echo. Backspace erases on screen. Enter triggers LLM. Ctrl-C passed to LLM. First Enter shows banner, subsequent empty Enters skip LLM.

**SFTP**: LLM-controlled virtual filesystem. Operations: opendir, readdir, open, read, close, lstat, fstat, realpath. Handle tracking prevents re-reads.

**Connection Tracking**: SSH connections tracked with `ProtocolConnectionInfo::Ssh {authenticated, username, channels}`.

## Git Commit Instructions

- **DO NOT** add "🤖 Generated with [Claude Code]" line
- **DO NOT** add "Co-Authored-By: Claude <noreply@anthropic.com>" signature
- Keep messages clean and professional without AI references
- Write concise, descriptive messages explaining what and why

## References

- Ollama API: http://localhost:11434
- Tokio: https://docs.rs/tokio
- Ratatui: https://github.com/ratatui-org/ratatui
