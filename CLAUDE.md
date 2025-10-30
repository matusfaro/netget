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

**Action response**:
```json
{"actions": [{"type": "send_tcp_data", "data": "hello"}, {"type": "show_message", "message": "Sent greeting"}]}
```

All protocols use the action-based response format. The LLM returns a JSON object with an `actions` array containing action objects. Each action has a `type` field and protocol-specific parameters.

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

**CRITICAL: Test Organization**:
- **ALL tests MUST be in the `tests/` directory, NEVER in `src/`**
- **Tests in `tests/` can ONLY access public APIs** - they are compiled as separate crates
- Unit tests in `tests/` should test public functions, types, and modules
- Integration tests in `tests/` should test end-to-end behavior with real clients
- **No `#[cfg(test)]` modules in `src/` files** - keep production code clean
- Tests that need private API access should be refactored to test through public interfaces

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
cargo test --test e2e_proxy_test --features e2e-tests,proxy  # Proxy tests
```

### E2E Test Performance

**Critical**: E2E tests are slow because each test spawns a NetGet process and makes LLM API calls.

**Expected runtimes** (with qwen3-coder:30b and `--test-threads=3`):
- Fast protocols (IPP, MySQL): 15-25 seconds per suite
- Medium protocols (Telnet, HTTP, IRC): 35-50 seconds per suite
- Slow protocols (SMTP, mDNS): 55-85 seconds per suite
- Very slow protocols (TCP/FTP): >5 minutes per suite (complex multi-round-trip protocols)

**Parallelization**:
- **ALWAYS run with `--test-threads=3`** for e2e tests
- Provides significant speedup by utilizing multiple CPU cores
- Each test is isolated (dynamic ports, separate processes)
- Ollama handles concurrent LLM requests internally
- Example: `cargo test --features e2e-tests --test e2e_telnet_test -- --test-threads=3`

**Critical setup requirement**:
- **MUST build release binary with all features before running tests**:
  ```bash
  cargo build --release --all-features
  ```
- E2E tests spawn the release binary from `target/release/netget`
- If the binary wasn't built with all features, protocol tests will fail
- Symptom: Server starts as TCP stack instead of protocol-specific stack

**Known issues**:
- TCP/FTP tests: May show occasional flakiness (1-2 failures) when running with --test-threads=3 due to LLM overload
  - All tests pass reliably when run individually
  - Reduced from >5 minutes to ~20 seconds (15x improvement)

### Privacy and Network Isolation Policy

**CRITICAL SECURITY REQUIREMENT**: NetGet must NOT leak information to external networks during testing or runtime unless explicitly requested by the user.

**Testing Requirements**:
1. **All tests MUST pass without internet access** - Tests must use local servers only
2. **No external endpoints** - Tests must NOT make requests to public services (e.g., httpbin.org, example.com)
3. **Self-contained test infrastructure** - Create local HTTP/HTTPS servers within test code
4. **Localhost only** - All test traffic must be to 127.0.0.1 or ::1

**Runtime Requirements**:
1. **Explicit user consent** - Only make external network requests when user prompt explicitly requests it
2. **No telemetry** - Never send usage data, metrics, or logs to external services
3. **No automatic updates** - Never check for updates or download content without user request
4. **LLM-controlled external access** - External network requests only when LLM receives explicit instructions from user

**Test Server Pattern**:
```rust
// CORRECT: Local HTTPS test server
async fn start_test_https_server() -> (u16, JoinHandle<()>) {
    // Generate self-signed cert
    // Bind to 127.0.0.1:0
    // Return port and handle
}

// INCORRECT: External service
let response = client.get("https://httpbin.org/get").send().await; // ❌ NEVER
```

**Violation Examples**:
- ❌ Using httpbin.org, example.com, or any public service in tests
- ❌ DNS queries to real DNS servers in tests
- ❌ Downloading dependencies or data at runtime
- ❌ Sending logs/metrics to external services
- ❌ Checking for updates automatically

This policy ensures NetGet respects user privacy and works in isolated/air-gapped environments.

## Logging (CRITICAL)

**IMPORTANT**: Do NOT use `info!()`, `debug!()`, `trace!()`, `error!()`, or `warn!()` macros directly without also sending to status_tx. These macros alone only write to the log file and are not visible in the TUI.

**Dual logging pattern** - ALL logs MUST go to BOTH:
1. Tracing macros (`debug!`, `trace!`, `error!`, `warn!`, `info!`) → `netget.log` file
2. Status channel (`status_tx.send()`) → TUI Status panel

**Required pattern**:
```rust
debug!("TCP sent {} bytes to {}", len, conn_id);
let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}", len, conn_id));
```

**Exception**: In synchronous code without access to `status_tx` (like script executor), use tracing macros only. The tracing framework automatically adds prefixes like `[DEBUG]` and `[TRACE]`.

**Prefixes**: `[DEBUG]`, `[TRACE]`, `[ERROR]`, `[WARN]`, `[INFO]`
**User-facing symbols**: `→` (success), `✗` (failure), `✓` (confirmation)

**Levels**:
- ERROR - Critical failures
- WARN - Non-fatal issues
- INFO - Major lifecycle events (server start/stop, connections)
- DEBUG - Request/response summaries, general info
- TRACE - Full payloads (pretty-printed JSON), script input/output, detailed execution traces

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

## Protocol Implementation Checklist

When creating new protocols in NetGet, ensure ALL of these steps are completed:

### 1. Protocol Stack Definition (`src/protocol/base_stack.rs`)
- Add new variant to `BaseStack` enum with correct stack name
- Update `name()` method with proper stack representation (e.g., "ETH>IP>TCP>SMTP")
- Update `from_str()` to parse protocol keywords
- Update `available_stacks()` to include new protocol
- Add unit tests for parsing the new protocol

### 2. TUI Description (`src/cli/tui.rs`)
- Add protocol description to welcome message list
- Include example usage (e.g., "Start an SMTP mail server on port 25")
- Mark as "(Alpha)" if new/experimental

### 3. Protocol Implementation (`src/network/<protocol>.rs`)
- Create server implementation file
- Implement spawn function with LLM integration
- Add **structured logging to Output tab**:
  - **TRACE** - Packet-level details, full payloads, pretty-printed JSON
  - **DEBUG** - Summaries and formatted view of packets, request/response summaries
  - **INFO** - High-level events: connection open/close, server start/stop
  - **ERROR** - Critical failures
  - **WARN** - Non-fatal issues
- Use **dual logging pattern** (both tracing macros AND status_tx)
- Ensure connections are **properly tracked in the UI**:
  - Add to server's connection HashMap
  - Update connection status (Active/Closed)
  - Track bytes sent/received, packets sent/received
  - Update last_activity timestamp

### 4. Protocol Actions (`src/network/<protocol>_actions.rs`)
- Implement `ProtocolActions` trait
- Define async actions (user-triggered, no network context)
- Define sync actions (network event triggered, with context)
- Create action definitions with parameters and examples
- Implement action execution logic

### 5. Module Registration (`src/network/mod.rs`)
- Add module declarations with feature flags
- Export server and protocol structs

### 6. Server Startup (`src/cli/server_startup.rs`)
- Add match arm for new `BaseStack` variant
- Implement feature-gated server spawning
- Update server status on success/failure
- Send status updates to UI

### 7. Connection Info (`src/state/server.rs`)
- Add variant to `ProtocolConnectionInfo` enum
- Include protocol-specific state (write_half, queued_data, etc.)

### 8. Feature Flag (`Cargo.toml`)
- Add feature flag for protocol
- Add protocol-specific dependencies
- Include in `all-protocols` feature

### 9. E2E Test (`tests/e2e_<protocol>_test.rs`)
- **Must start NetGet in non-interactive mode** with a prompt
- **Must assert server started with correct stack** using helpers
- **Must use real client** or emulated client (only if no library available)
- Test multiple scenarios:
  - Basic functionality (connect, send, receive)
  - Protocol-specific commands
  - Error handling
  - Concurrent connections (if applicable)
- **Before running tests, MUST build release binary**:
  ```bash
  cargo build --release --all-features
  ```
- **Run tests with parallelization**:
  ```bash
  cargo test --features e2e-tests --test e2e_<protocol>_test -- --test-threads=3
  ```
- **Fix any issues before considering protocol complete**

### 10. Test Helpers (`tests/e2e/helpers.rs`)
- Update `extract_stack_from_prompt()` if needed
- Update `wait_for_server_startup()` to detect new protocol
- Handle protocol-specific stack validation

### Validation Checklist
- [ ] Protocol compiles with feature flag
- [ ] Protocol compiles in `all-protocols` mode
- [ ] E2E tests pass
- [ ] No compilation warnings
- [ ] Logging appears in Output panel
- [ ] Connections tracked in UI
- [ ] Stack name displays correctly
- [ ] Protocol responds to LLM actions correctly

### Common Pitfalls
- Forgetting dual logging (tracing + status_tx)
- Not tracking connections in server state
- Missing feature flags in multiple files
- E2E tests using interactive mode instead of prompt mode
- Not validating server stack in tests
- Forgetting to update base_stack.rs parsing

## SSH/SFTP Implementation

**Library**: russh 0.45 + russh-sftp 2.1
**Location**: `src/network/ssh.rs`, `src/network/sftp_handler.rs`

**SSH Shell Buffering**: Line-based with character echo. Backspace erases on screen. Enter triggers LLM. Ctrl-C passed to LLM. First Enter shows banner, subsequent empty Enters skip LLM.

**SFTP**: LLM-controlled virtual filesystem. Operations: opendir, readdir, open, read, close, lstat, fstat, realpath. Handle tracking prevents re-reads.

**Connection Tracking**: SSH connections tracked with `ProtocolConnectionInfo::Ssh {authenticated, username, channels}`.

## Git Commit Instructions

- **ONLY commit when explicitly requested by the user** - Do not automatically commit changes
- **DO NOT** add "🤖 Generated with [Claude Code]" line
- **DO NOT** add "Co-Authored-By: Claude <noreply@anthropic.com>" signature
- Keep messages clean and professional without AI references
- Write concise, descriptive messages explaining what and why

## References

- Ollama API: http://localhost:11434
- Tokio: https://docs.rs/tokio
- Ratatui: https://github.com/ratatui-org/ratatui
