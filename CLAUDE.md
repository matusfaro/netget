# NetGet - Knowledge Document

This document captures key learnings, architectural decisions, and implementation details discovered during development and testing of NetGet.

## Project Overview

NetGet is a Rust CLI application where an LLM (via Ollama) controls network protocols. The application supports multiple base protocol stacks with different levels of LLM control.

**Critical Design Principle**: The LLM is in control - either constructing raw protocol datagrams or generating high-level responses based on the chosen stack.

### Base Protocol Stacks

NetGet supports multiple base protocol stacks that determine what the LLM controls:

1. **TCP/IP Raw Stack** (`BaseStack::TcpRaw`)
   - LLM controls raw TCP data
   - LLM constructs entire protocol messages from scratch (FTP, HTTP, custom protocols)
   - Application provides ONLY the TCP/IP stack
   - Protocol types: FTP, HTTP, Custom

2. **HTTP Stack** (`BaseStack::Http`)
   - Uses Rust HTTP library (hyper)
   - LLM controls HTTP responses (status, headers, body) based on requests
   - Application handles HTTP parsing and serving
   - LLM receives structured request data (method, URI, headers, body)
   - LLM returns structured response data (status, headers, body)

**Selection**: Users select the base stack when starting a server:
- `listen on port 21 via ftp` → TCP/IP Raw + FTP protocol
- `listen on port 80 via http` → HTTP Stack
- `listen on port 8080 via http stack` → HTTP Stack (explicit)

## Architecture

### Component Layers

```
┌───────────────────────┐
│   TUI (ratatui)       │  - User input
│   4-panel interface   │  - LLM responses
└───────────┬───────────┘  - Connection info
            │              - Status log
            ▼
┌───────────────────────┐
│   Event System        │  - UserCommand
│   (mpsc channels)     │  - NetworkEvent
└───────────┬───────────┘  - AppEvent
            │
      ┌─────┴────────┐
      ▼              ▼
┌───────────┐ ┌──────────────┐
│ TCP/IP    │ │ EventHandler │
│ Stack     │ │ + LLM Client │
└───────────┘ └──────────────┘
```

### Key Modules

1. **`ui/`** - Full-screen terminal UI
   - Midnight Commander blue theme
   - 4 panels: input, LLM responses, connection info, status

2. **`network/`** - Network stack implementations
   - `tcp.rs` - TcpServer, accept(), listen() (for TcpRaw stack)
   - `http.rs` - HttpServer using hyper (for HTTP stack)
   - `connection.rs` - ConnectionId tracking
   - NO protocol logic (in TCP stack)

3. **`protocol/`** - Protocol definitions
   - `base_stack.rs` - `enum BaseStack { TcpRaw, Http }`
   - `mod.rs` - `enum ProtocolType { Ftp, Http, Custom }` (for TcpRaw stack only)
   - NO protocol implementations

4. **`state/`** - Application state
   - Mode (Server/Client/Idle)
   - Base stack (TcpRaw/Http)
   - Protocol type (for TcpRaw stack)
   - Connection tracking
   - User instructions for LLM context

5. **`llm/`** - Ollama integration
   - `client.rs` - Ollama API calls
   - `prompt.rs` - Prompt builders for each event type

6. **`events/`** - Event coordination
   - `types.rs` - Event enums
   - `handler.rs` - LLM-driven event processing

## Architecture Notes

### Data Queueing System

**New Feature**: LLM request queueing prevents concurrent processing and allows smart data accumulation.

**Connection States**:
- **Idle**: Not processing, no queued data
- **Processing**: LLM is currently generating a response
- **Accumulating**: LLM requested "WAIT_FOR_MORE", data is accumulating

**Data Flow**:
1. Data arrives → **Spawn async task** to handle it (enables concurrent event processing)
2. Task checks connection state:
   - If **Processing**: Add to queue, send status msg to UI, exit task (prevents concurrent LLM calls)
   - If **Idle** or **Accumulating**: Proceed to process
3. Merge any queued data with new data, mark as Processing
4. Call LLM with all accumulated data (may take several seconds)
5. When LLM responds:
   - If `WAIT_FOR_MORE`: Enter Accumulating state, send status msg to UI
   - If `CLOSE_CONNECTION`: Close connection, send status msg to UI, remove from state
   - If output present: Send over connection, **send "→ Sent to..." msg to UI**
   - Otherwise: No action
6. Check queue:
   - If queue has data (arrived during LLM processing): Process immediately, send status msg to UI (loop back to step 4)
   - If queue empty: Go to Idle state, exit task

**Status Message Channel**:
Spawned tasks send status messages back to the main UI loop via an unbounded channel. The main loop drains this channel every iteration and displays messages. This allows async tasks to update the UI without shared state.

**Why spawn tasks?**
Without spawning tasks, the event loop processes events sequentially - each event is fully handled (including LLM wait) before the next is pulled from the channel. This means rapid data arrivals would never queue because event 2 isn't pulled until event 1 finishes.

By spawning tasks, multiple DataReceived events can be in-flight simultaneously. The first spawns a task and starts calling the LLM. While the LLM generates, subsequent events spawn their own tasks, check the status (now Processing), and queue their data instead of making concurrent LLM calls.

**Benefits**:
- No concurrent LLM calls for the same connection (state machine enforced)
- LLM can request more data for incomplete commands (e.g., partial HTTP headers)
- Queued data is batched and processed together
- Prevents loss of data that arrives during LLM processing
- Multiple connections can be processed concurrently (each has its own state)

**Location**:
- Structs: `src/main.rs:26-52`
- Status message channel: `src/main.rs:227-228` (creation), `src/main.rs:500-503` (processing)
- DataReceived handler: `src/main.rs:553-731` (spawns async task, sends status messages)
- OllamaClient made Clone: `src/llm/client.rs:8`

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

### Structured LLM Responses

**New Feature**: LLM responses are now structured JSON instead of plain text with magic strings.

**Response Format**:
```json
{
  "output": "220 Welcome\r\n",
  "close_connection": false,
  "wait_for_more": false,
  "shutdown_server": false,
  "log_message": "Sent FTP greeting"
}
```

**Fields**:
- `output` (string|null): Data to send over the wire. Use actual `\n`, `\r`, etc. Null/omitted = no output
- `close_connection` (bool): Close this specific connection after sending output
- `wait_for_more` (bool): Wait for more data before responding (triggers Accumulating state)
- `shutdown_server` (bool): Shut down the entire server (not yet implemented)
- `log_message` (string|null): Optional debug message logged with `info!()`

**Benefits**:
- **Extensible**: Easy to add new flags/fields without breaking existing code
- **Type-safe**: JSON parsing ensures correct types
- **Clear semantics**: No ambiguity like "is empty string = no response?"
- **Backwards compatible**: Legacy text responses still work via fallback parser

**Fallback Handling**:
If JSON parsing fails, the parser handles legacy magic strings:
- `"NO_RESPONSE"` → `{}`
- `"CLOSE_CONNECTION"` → `{"close_connection": true}`
- `"WAIT_FOR_MORE"` → `{"wait_for_more": true}`
- Anything else → `{"output": "..."}`

**Location**:
- Struct definition: `src/llm/client.rs:7-73`
- Parser: `src/llm/client.rs:44-73` (with fallback)
- Prompts updated: `src/llm/prompt.rs:53-80` (data), `src/llm/prompt.rs:162-175` (connection)
- Usage: `src/main.rs` (Connected, DataReceived events)

### HTTP Stack Responses

**New Feature**: For HTTP stack, LLM returns structured HTTP responses instead of raw TCP data.

**Response Format**:
```json
{
  "status": 200,
  "headers": {"Content-Type": "text/html"},
  "body": "<html><body>Hello!</body></html>",
  "log_message": "Generated HTML response"
}
```

**Fields**:
- `status` (u16): HTTP status code (e.g., 200, 404, 500)
- `headers` (HashMap<String, String>): Response headers
- `body` (string): Response body as a string
- `log_message` (string|null): Optional debug message logged with `info!()`

**How it works**:
1. HTTP request arrives at the server (via hyper)
2. Request is parsed and converted to HttpRequest event with oneshot response channel
3. LLM receives structured prompt with method, URI, headers, body
4. LLM generates structured HTTP response JSON
5. Response is sent back via oneshot channel to HTTP handler
6. hyper converts structured response to actual HTTP response

**Location**:
- Struct definition: `src/llm/client.rs:78-123` (HttpLlmResponse)
- HTTP server: `src/network/http.rs`
- Prompt builder: `src/llm/prompt.rs:204-292` (build_http_request_prompt)
- Event handling: `src/main.rs` (HttpRequest event)
- Event type: `src/events/types.rs:44-61` (HttpRequest with oneshot channel)

### Action-Based Prompt System (NEW)

**Major Refactoring**: Redesigned the entire prompt and response system to use a unified action-based architecture.

**Core Concept**: Both user input and network events now return arrays of actions `{actions: [...]}` instead of nested structures. Each action is self-describing and can be executed independently.

#### Architecture Components

**1. Action System** (`src/llm/actions/`)

Core structures for defining and executing actions:

- **`ActionDefinition`**: Self-describing action with name, description, parameters, and example
- **`Parameter`**: Type-safe parameter definition (name, type, description, required)
- **`ActionResponse`**: LLM response format: `{actions: [action, action]}`
- **`NetworkContext`**: Context enum for all protocol types (TcpConnection, UdpDatagram, HttpRequest, SnmpRequest, etc.)
- **`ProtocolActions` trait**: Interface for protocol-specific action systems
- **`CommonAction` enum**: 9 common actions available across all contexts
- **Action executor**: Parses action array and executes in order

**2. Action Categories**

Actions are categorized by when they can be executed:

- **Common Actions**: Available in both user input and network events
  - `show_message` - Display message to user
  - `open_server` - Start a new server
  - `close_server` - Stop current server
  - `update_instruction` - Update server instruction
  - `change_model` - Switch LLM model
  - `set_memory` / `append_memory` - Memory operations

- **Protocol Async Actions**: Executable anytime from user input (no network context required)
  - TCP: `send_to_connection(connection_id, data)`, `close_connection(connection_id)`, `list_connections()`
  - SNMP: `send_trap(target, variables)`
  - UDP: `send_to_address(address, data)`
  - User can say "send hello to connection X" and it executes immediately

- **Protocol Sync Actions**: Require network context (only available during network events)
  - TCP: `send_tcp_data(data)`, `wait_for_more()`, `close_this_connection()`
  - HTTP: `send_http_response(status, headers, body)`
  - SNMP: `send_snmp_response(variables)`, `send_snmp_error(error_message)`
  - UDP: `send_udp_response(data)`, `ignore_datagram()`
  - DNS, DHCP, NTP: `send_X_response(data)`, `ignore_request()`
  - SSH, IRC: `send_X_data(data)`, `wait_for_more()`, `close_connection()`

**3. Unified Prompt Builder** (`src/llm/prompt.rs`)

Three prompt building functions that share the same underlying structure:

```rust
// Core unified builder
build_action_prompt(state, trigger_reason, instructions, available_actions) -> String

// User input (includes common + protocol async actions)
build_user_input_action_prompt(state, user_input, protocol_async_actions) -> String

// Network events (includes common subset + protocol sync actions)
build_network_event_action_prompt(state, event_description, protocol_sync_actions) -> String
```

**Prompt Structure**:
1. **Current State**: Server status, port, stack, memory
2. **Trigger Reason**: "User input: X" or "Event: TCP data received"
3. **Instructions**: How to handle (user instructions or protocol requirements)
4. **Available Actions**: Auto-generated list with examples for each available action
5. **Response Format**: `{actions: [...]}`

#### Protocol Implementation Pattern

Each protocol implements `ProtocolActions` trait:

```rust
// Example: UDP Protocol
impl ProtocolActions for UdpProtocol {
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition> {
        // Actions user can trigger anytime
        vec![send_to_address_action()]
    }

    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition> {
        // Actions only available when handling network events
        match context {
            NetworkContext::UdpDatagram { .. } => vec![
                send_udp_response_action(),
                ignore_datagram_action()
            ],
            _ => Vec::new()
        }
    }

    fn execute_action(&self, action: serde_json::Value, context: Option<&NetworkContext>) -> Result<ActionResult> {
        // Parse and execute the action
        let action_type = action.get("type").and_then(|v| v.as_str())?;
        match action_type {
            "send_udp_response" => self.execute_send_udp_response(action, context),
            "ignore_datagram" => Ok(ActionResult::NoAction),
            _ => Err(anyhow!("Unknown UDP action: {}", action_type))
        }
    }

    fn protocol_name(&self) -> &'static str { "UDP" }
}
```

#### Protocol Action Files

Each protocol has dedicated action handler:
- `src/network/udp_actions.rs` - UdpProtocol
- `src/network/tcp_actions.rs` - TcpProtocol
- `src/network/http_actions.rs` - HttpProtocol
- `src/network/snmp_actions.rs` - SnmpProtocol
- `src/network/dns_actions.rs` - DnsProtocol
- `src/network/dhcp_actions.rs` - DhcpProtocol
- `src/network/ntp_actions.rs` - NtpProtocol
- `src/network/ssh_actions.rs` - SshProtocol
- `src/network/irc_actions.rs` - IrcProtocol

#### User Command Integration

**Event Handler** (`src/events/handler.rs`):

```rust
// New action-based handler
handle_interpret_with_actions(input, status_tx, protocol) -> Result<()>
```

Flow:
1. User types command
2. Get protocol async actions if server is running
3. Build prompt with `build_user_input_action_prompt()`
4. LLM returns `{actions: [...]}`
5. Execute actions via `execute_actions()`
6. Handle server management actions separately (open/close server)
7. Falls back to old system if parsing fails

#### Benefits

**Context-Aware Actions**:
- Async actions only shown when they make sense (e.g., TCP `send_to_connection` only if connections exist)
- Sync actions only available during network events (e.g., UDP `send_udp_response` needs peer address)

**Protocol-Specific**:
- Each protocol defines exactly the actions it needs
- No hardcoded protocol logic in prompts
- Easy to add new protocols

**Type-Safe**:
- Structured action definitions with parameter validation
- Compile-time checks for action types

**Unified**:
- Same prompt structure for all protocols
- Same action execution flow
- Consistent user experience

**Extensible**:
- Add new actions without changing core system
- Protocols can add arbitrary actions based on runtime state
- Actions execute in defined order

**User-Friendly**:
- Users can interact with protocols naturally: "send hello to connection X"
- Protocol async actions bridge the gap between user commands and network operations

#### Example: UDP with New System

**User Input**:
```
User: "send 'ping' to 192.168.1.100:8080"
```

**LLM Sees** (via `build_user_input_action_prompt`):
- Common actions (show_message, open_server, etc.)
- UDP async actions (send_to_address)
- No UDP sync actions (no network context)

**LLM Returns**:
```json
{
  "actions": [
    {"type": "send_to_address", "address": "192.168.1.100:8080", "data": "ping"}
  ]
}
```

**Network Event**:
```
UDP datagram received from 192.168.1.100:8080
```

**LLM Sees** (via `build_network_event_action_prompt`):
- Common actions (show_message, memory operations)
- UDP sync actions (send_udp_response, ignore_datagram)
- NetworkContext with peer address

**LLM Returns**:
```json
{
  "actions": [
    {"type": "send_udp_response", "data": "pong"}
  ]
}
```

#### Migration Notes

**Current Status**:
- ✅ All protocols have action implementations
- ✅ Unified prompt builder complete
- ✅ Action executor working
- ✅ User command handler integrated
- ✅ UDP proof-of-concept functional
- ⏳ Old system remains as fallback during transition

**Location**:
- Core system: `src/llm/actions/`
- Protocol actions: `src/network/*_actions.rs`
- Prompt builder: `src/llm/prompt.rs:200-337` (new functions)
- User handler: `src/events/handler.rs:132-267` (handle_interpret_with_actions)

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

**Philosophy**: Tests are **black-box** and **prompt-driven**. Each test provides a prompt that the LLM interprets, and validates behavior using real network clients.

**Unit Tests** (`#[test]`):
- No Ollama required
- Test command parsing, protocol type detection
- Example: `test_user_command_parsing()` in tcp_integration_test.rs

**Integration Tests** (`tests/` directory):
- Requires Ollama running with a model
- **Simple setup**: Just provide a prompt, the system infers everything (mode, stack, protocol)
- Uses real network clients (suppaftp for FTP, reqwest for HTTP, raw TCP sockets)
- Tests full system: Prompt → LLM → Protocol behavior

**Test Structure**:
Each test has two clear sections:
1. **PROMPT**: Instructions for the LLM (e.g., "listen on port 0 via ftp. Serve file data.txt")
2. **VALIDATION**: Verify behavior using real clients

Example:
```rust
// PROMPT: Tell the LLM to act as an FTP server
let prompt = "listen on port 0 via ftp. Serve file data.txt with content: hello";
let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

// VALIDATION: Use real FTP client to verify behavior
let mut ftp = FtpStream::connect(format!("127.0.0.1:{}", port))?;
ftp.login("anonymous", "test@example.com")?;
assert!(ftp.pwd().is_ok());
```

**Test Helper** (`tests/common/mod.rs`):
- `start_server_with_prompt(prompt)` - Black-box server setup
- Parses prompt using `UserCommand::parse()` to infer configuration
- Sets up appropriate server (TCP or HTTP) based on prompt
- Returns (state, port, handle) for cleanup

**TCP Integration Tests** (`tcp_integration_test.rs`):
- `test_ftp_server` - FTP protocol via LLM (uses suppaftp client)
- `test_simple_echo` - Simple echo/reply behavior (raw TCP)
- `test_custom_response` - Greeting and PING/PONG (raw TCP)

**HTTP Integration Tests** (`http_integration_test.rs`):
- `test_http_get_html` - GET request with HTML response
- `test_http_post_json` - POST request with JSON response
- `test_http_custom_headers` - Custom headers verification
- `test_http_404` - 404 error response
- `test_http_routing` - Route-based responses

**Dynamic Port Allocation**:
Tests use port 0 in prompts, which auto-assigns an available port. This avoids port conflicts and allows parallel test execution.

## Implementation Notes

### Event Flow

**TCP/IP Raw Stack:**
1. **User types command** → Parse → UserCommand event
2. **TCP connection arrives** → Accept → Connected event
3. **Data received** → Read → DataReceived event
4. **EventHandler processes event** → Calls LLM → Sends response

**HTTP Stack:**
1. **User types command** → Parse → UserCommand event (with BaseStack::Http)
2. **HTTP request arrives** → hyper parses → HttpRequest event with oneshot channel
3. **Event handler spawns task** → Calls LLM → Returns structured HTTP response
4. **Response sent via oneshot** → hyper converts to HTTP response → Sends to client

### Prompt Engineering

**TCP/IP Raw Stack Prompts** include:
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

**HTTP Stack Prompts** include:
- Current mode (Server)
- Base stack (HTTP)
- User instructions history
- HTTP request details (method, URI, headers, body)

Example HTTP prompt:
```
You are controlling an HTTP server application.

Mode: Server
Stack: HTTP

User Instructions:
For any POST request, return a JSON response with status 200

Event: HTTP Request
Method: POST
URI: /api/data
Headers:
  Content-Type: application/json
Body:
{"key": "value"}

IMPORTANT: Respond with a JSON object with the following structure:
{
  "status": 200,
  "headers": {"Content-Type": "application/json"},
  "body": "response body content",
  "log_message": "optional debug message"
}
```

### Color Scheme (Midnight Commander Blue)

All panels: Blue background (`Color::Blue`)
- **Input panel**: White on blue
- **LLM panel**: White on blue
- **Connection Info**: Cyan labels, white values on blue
- **Status**: Light cyan on blue
- **Borders**: Bold cyan

User explicitly requested this after initial white-on-black was not visible.

### Logging

NetGet uses structured logging with different levels for different purposes. Logging behavior differs between interactive (TUI) and non-interactive modes.

**Logging Levels:**

1. **ERROR** - Critical errors that prevent functionality
   - Server startup failures
   - Fatal LLM errors
   - Configuration errors

2. **WARN** - Non-fatal issues that may need attention
   - Missing configuration files (falling back to defaults)
   - LLM response parsing failures (with fallback)
   - Connection errors that are recovered

3. **INFO** - Major events in the application lifecycle
   - Server started/stopped (with port, protocol)
   - Connection established/closed (with peer address)
   - Mode changes (Idle → Server)
   - Model changes
   - User instruction updates

4. **DEBUG** - Protocol-level request/response summaries
   - Each network request with summary (protocol-dependent)
   - Each LLM request with summary (model, prompt length)
   - Each protocol response with summary (status code, size)
   - Each LLM response with summary (action count, flags)
   - Action execution results

5. **TRACE** - Full payloads for debugging
   - Complete protocol request payloads
   - Complete protocol response payloads
   - Complete LLM prompts
   - Complete LLM responses (with pretty-printed JSON)
   - Raw network data (hex dumps for binary protocols)

**Default Levels:**
- **Non-interactive mode**: INFO (use `-l debug` or `-l trace` to increase)
- **Interactive mode**: TRACE (logged to `netget.log`)

**Configuration:**
```bash
# Non-interactive with default (INFO)
netget listen on port 80 via http

# Non-interactive with debug logging
netget -l debug listen on port 80 via http

# Non-interactive with trace logging
netget -l trace listen on port 80 via http

# Interactive mode (always logs TRACE to netget.log)
netget
```

**Log Format:**
- **Non-interactive**: Logs to stderr with timestamps and levels
- **Interactive**: Logs to `netget.log` file to avoid garbling TUI

**JSON Pretty-Printing:**
All JSON in trace logs is automatically pretty-printed for readability:
- LLM responses (action arrays)
- HTTP request/response bodies (if JSON)
- SNMP variable bindings
- Any other JSON payloads

**Location**: `src/cli/setup.rs`, `src/cli/args.rs`

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

1. **No streaming LLM**: Each response waits for full LLM generation
2. **No error recovery**: LLM errors just log, don't retry
3. **No TLS/SSL**: Plain TCP only
4. **Protocol integration incomplete**: New action system implemented but not fully integrated into all protocol servers (only UDP has `spawn_with_llm_actions` currently)

## Future Work

### High Priority

- Integrate action system into all protocol servers (TCP, HTTP, SNMP, DNS, DHCP, NTP, SSH, IRC)
- Migrate network event handlers to use `build_network_event_action_prompt()`
- Add streaming LLM support for faster responses
- Add retry logic for LLM failures
- Update integration tests to use new action system

### Medium Priority

- Remove old response structures and prompt functions after migration complete
- Add more protocol async actions (e.g., SSH `send_to_connection`, IRC `broadcast_message`)
- Implement dynamic action availability based on state (e.g., TCP `list_connections` only if connections exist)

### Low Priority

- TLS/SSL support
- WebSocket support
- Client mode implementation
- Action composition (actions that trigger other actions)

## Testing Checklist

Before committing changes:

- [ ] Run unit tests: `cargo test --lib`
- [ ] Start Ollama: `ollama serve`
- [ ] Pull model: `ollama pull qwen3-coder:30b`

**TCP Integration Tests** (prompt-based, black-box):
- [ ] Run FTP test: `cargo test --test tcp_integration_test test_ftp_server`
- [ ] Run echo test: `cargo test --test tcp_integration_test test_simple_echo`
- [ ] Run custom response test: `cargo test --test tcp_integration_test test_custom_response`
- [ ] Run all TCP tests: `cargo test --test tcp_integration_test`
- [ ] Check no hardcoded protocols: `grep -r "220 FTP" src/` should return nothing

**HTTP Integration Tests** (prompt-based, black-box):
- [ ] Run GET HTML test: `cargo test --test http_integration_test test_http_get_html`
- [ ] Run POST JSON test: `cargo test --test http_integration_test test_http_post_json`
- [ ] Run custom headers test: `cargo test --test http_integration_test test_http_custom_headers`
- [ ] Run 404 test: `cargo test --test http_integration_test test_http_404`
- [ ] Run routing test: `cargo test --test http_integration_test test_http_routing`
- [ ] Run all HTTP tests: `cargo test --test http_integration_test`

## References

- Ollama API: http://localhost:11434
- Tokio docs: https://docs.rs/tokio
- Ratatui: https://github.com/ratatui-org/ratatui
- Test FTP client: https://docs.rs/suppaftp

## Learnings Summary

1. **Architecture**: Multi-stack design - LLM controls either raw protocols (TcpRaw) or high-level responses (HTTP)
2. **Base stacks**: TCP/IP Raw (full protocol control) and HTTP (response-only control)
3. **Connection management**: Split streams with `tokio::io::split()` and use `Arc<Mutex<WriteHalf>>`
4. **Testing**: Separate unit tests (no Ollama) from integration tests (requires Ollama)
5. **Concurrency**: Avoid deadlocks by splitting read/write or using proper locking patterns
6. **Model**: qwen3-coder:30b optimized for protocol implementation
7. **UI**: Midnight Commander blue theme for visibility
8. **Events**: Channel-based architecture with async event processing
9. **UX**: Shell-like features (history, multi-line, keybindings) improve usability
10. **CLI**: Support for initial command argument enables scripting
11. **Logging**: Logs to file (netget.log) to avoid garbling TUI
12. **History persistence**: Commands saved to ~/.netget_history across sessions
13. **Data queueing**: Per-connection state machine prevents concurrent LLM calls and enables smart data accumulation with WAIT_FOR_MORE
14. **Structured responses**: LLM returns JSON with flexible fields instead of magic strings
15. **HTTP stack**: Uses hyper library + oneshot channels for request-response pattern with LLM
16. **Response channels**: Tokio oneshot channels enable synchronous HTTP responses from async LLM calls
17. **Action-based prompts**: Unified action system where LLM returns `{actions: [...]}` for both user input and network events
18. **Async vs Sync actions**: Protocol actions categorized by context requirement - async (no context) vs sync (requires network context)
19. **Protocol-specific actions**: Each protocol defines its own actions via `ProtocolActions` trait with `get_async_actions()` and `get_sync_actions()`
20. **Context-aware prompts**: Actions auto-included in prompts based on availability (e.g., TCP `send_to_connection` only if server running)
21. **User protocol control**: TCP async actions enable users to send data to specific connections directly from CLI

## Git Commit Instructions

When creating commits in this repository:
- **DO NOT** add the "🤖 Generated with [Claude Code]" line
- **DO NOT** add the "Co-Authored-By: Claude <noreply@anthropic.com>" signature
- Keep commit messages clean and professional without any AI/Claude references
- Write concise, descriptive commit messages that explain what changed and why
