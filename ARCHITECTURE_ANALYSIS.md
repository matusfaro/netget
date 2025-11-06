# NetGet Server Architecture - Comprehensive Analysis

## Executive Summary
NetGet is an LLM-controlled network protocol framework with:
- Multi-protocol server support (50+ protocols)
- Action-based LLM integration (structured responses)
- Flexible state management for multi-instance servers
- Event-driven architecture with task scheduling
- Dual logging (file + TUI)

Key insight: The architecture is designed as **per-protocol autonomous servers** controlled by a centralized LLM via structured **actions**. Each protocol is decentralized (implements its own Server trait) but orchestrated by shared AppState.

---

## 1. STATE MANAGEMENT ARCHITECTURE

### 1.1 AppState - Global Application State
**File**: `src/state/app_state.rs` (1378 lines)

AppState is an `Arc<RwLock<AppStateInner>>` providing thread-safe access to:

```rust
pub struct AppState {
    inner: Arc<RwLock<AppStateInner>>
}

struct AppStateInner {
    mode: Mode,                           // Idle/Server/Client
    servers: HashMap<ServerId, ServerInstance>,  // All servers
    next_server_id: u32,                  // Auto-incrementing server IDs
    ollama_model: String,                 // Active LLM model
    scripting_env: ScriptingEnvironment,  // Python/Node.js availability
    selected_scripting_mode: ScriptingMode,      // On/Off/Python/JavaScript/Go
    web_search_mode: WebSearchMode,       // On/Off/Ask
    web_approval_tx: Option<mpsc::UnboundedSender<WebApprovalRequest>>,
    include_disabled_protocols: bool,     // For testing
    ollama_lock_enabled: bool,            // Serialize LLM calls
    instance_id: String,                  // Unique process ID
    tasks: HashMap<TaskId, ScheduledTask>, // All scheduled tasks
    next_task_id: u64,
    task_names: HashMap<String, TaskId>,
    system_capabilities: SystemCapabilities, // Root/CAP_NET_RAW, etc
    conversations: Vec<ConversationInfo>,   // Tracking active LLM conversations
}
```

**Key Methods**:
- `add_server()`, `remove_server()`, `get_server()` - Server lifecycle
- `get_instruction()`, `set_instruction()` - Per-server LLM instructions
- `get_memory()`, `set_memory()` - Per-server LLM memory (context)
- `add_connection_to_server()`, `close_connection_on_server()` - Connection tracking
- Task management: `add_task()`, `get_task()`, `remove_task()`, cleanup methods
- Conversation tracking: `register_conversation()`, `end_conversation()`

**Critical Pattern**: All async methods use RwLock:
- Read operations: `.read().await` (non-blocking)
- Write operations: `.write().await` (exclusive)
- Never hold lock across I/O operations (deadlock risk)

### 1.2 ServerInstance - Individual Server State
**File**: `src/state/server.rs` (370 lines)

```rust
pub struct ServerInstance {
    pub id: ServerId,
    pub port: u16,
    pub protocol_name: String,
    pub instruction: String,              // LLM system prompt override
    pub memory: String,                   // LLM conversation memory
    pub status: ServerStatus,             // Starting/Running/Stopped/Error
    pub connections: HashMap<ConnectionId, ConnectionState>,
    pub handle: Option<JoinHandle<()>>,   // Tokio task handle
    pub created_at: Instant,
    pub status_changed_at: Instant,
    pub local_addr: Option<SocketAddr>,
    pub startup_params: Option<serde_json::Value>, // Protocol-specific params
    pub script_config: Option<ScriptConfig>,       // Lua/Python script config
    pub protocol_data: serde_json::Value,          // Flexible JSON storage
    pub log_files: HashMap<String, PathBuf>,       // Per-protocol log files
}
```

**Flexible Protocol Data Pattern**:
Instead of protocol-specific enum variants, each ServerInstance has:
- `protocol_data: serde_json::Value` - flexible JSON object
- `get_protocol_data::<T>()` and `set_protocol_data::<T>()` - type-safe access
- `get_protocol_field()` and `set_protocol_field()` - direct field access

This avoids centralized enum fights and allows protocols to store arbitrary state.

### 1.3 ConnectionState - Per-Connection Tracking
**File**: `src/state/server.rs` (209 lines)

```rust
pub struct ConnectionState {
    pub id: ConnectionId,
    pub remote_addr: SocketAddr,
    pub local_addr: SocketAddr,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub last_activity: Instant,
    pub status: ConnectionStatus,         // Active/Closed
    pub status_changed_at: Instant,
    pub protocol_info: ProtocolConnectionInfo, // Flexible JSON
}
```

**ProtocolConnectionInfo**: Flexible JSON storage for protocol-specific connection data:
```rust
pub struct ProtocolConnectionInfo {
    pub data: serde_json::Value,
}
```

Examples:
- HTTP: `{"recent_requests": [...]}`
- IMAP: `{"session_state": "Authenticated", "authenticated_user": "bob", ...}`
- TCP: `{"state": "Idle"}`

---

## 2. SERVER LIFECYCLE

### 2.1 Server Creation Flow

```
User Input ("open http server on 8080")
    ↓
Parser (recognize "http" keyword)
    ↓
CommonAction::OpenServer {
    protocol: "HTTP",
    port: 8080,
    instruction: "serve cooking recipes",
    startup_params: {...}
}
    ↓
Action Executor (rolling_tui.rs)
    ↓
1. Create ServerInstance(id=1, port=8080, protocol="HTTP")
2. Add to AppState: state.add_server(server)
3. Call: start_server_by_id(state, server_id, llm_client, status_tx)
    ↓
Server Startup (cli/server_startup.rs)
    ↓
1. Get protocol from registry: registry().get("HTTP")
2. Check privilege requirements (privileged port? raw sockets?)
3. Build SpawnContext with listen_addr, llm_client, app_state, status_tx
4. Call protocol.spawn(ctx) - returns bound SocketAddr
5. Update ServerInstance: local_addr, status=Running
6. Send status messages to TUI
```

### 2.2 SpawnContext - Protocol Spawn Parameters
**File**: `src/protocol/spawn_context.rs`

```rust
pub struct SpawnContext {
    pub listen_addr: SocketAddr,
    pub llm_client: OllamaClient,
    pub state: Arc<AppState>,              // Shared mutable state
    pub status_tx: mpsc::UnboundedSender<String>, // Status messages to TUI
    pub server_id: ServerId,
    pub startup_params: Option<StartupParams>,
}
```

Passed to every protocol's `spawn()` method. Gives protocols everything they need to:
- Bind to a socket
- Call the LLM
- Update AppState
- Send status messages

### 2.3 Protocol Registry
**File**: `src/protocol/registry.rs` (400+ lines)

Single source of truth for protocol implementations:
```rust
pub struct ProtocolRegistry {
    protocols: HashMap<String, Arc<dyn Server>>,
    keyword_map: HashMap<String, String>,  // Case-insensitive lookup
}

// Features control compilation
#[cfg(feature = "tcp")]
registry.register(Arc::new(TcpProtocol::new()));

#[cfg(feature = "http")]
registry.register(Arc::new(HttpProtocol::new()));
```

**Access**: `crate::protocol::registry::registry()` provides global instance.

---

## 3. ACTION SYSTEM - LLM INTEGRATION

### 3.1 Server Trait - Protocol Implementation Interface
**File**: `src/llm/actions/protocol_trait.rs` (232 lines)

Every protocol implements this trait:

```rust
pub trait Server: Send + Sync {
    fn spawn(
        &self,
        ctx: SpawnContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>>;

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition>;
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition>;
    fn get_sync_actions(&self) -> Vec<ActionDefinition>;
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult>;

    fn protocol_name(&self) -> &'static str;
    fn get_event_types(&self) -> Vec<EventType>;
    fn stack_name(&self) -> &'static str;
    fn keywords(&self) -> Vec<&'static str>;
    fn metadata(&self) -> ProtocolMetadataV2;
    fn description(&self) -> &'static str;
    fn example_prompt(&self) -> &'static str;
    fn group_name(&self) -> &'static str;
}
```

**Key Insight**: Each protocol is **autonomous**. It defines its own:
- Actions available to the LLM
- How to execute those actions
- Metadata for documentation and privilege checking

### 3.2 Action Types

#### Sync Actions (Network Events)
Executed in response to network events (data received, connection opened):
```rust
fn get_sync_actions(&self) -> Vec<ActionDefinition> {
    vec![
        send_tcp_data_action(),
        wait_for_more_action(),
        close_this_connection_action(),
    ]
}
```

These actions have direct access to network context:
- Current connection
- Received data
- Can modify connection state

#### Async Actions (User/Task Triggered)
Executable anytime from user input or scheduled tasks:
```rust
fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
    vec![
        send_to_connection_action(),   // Requires connection ID
        close_connection_action(),      // Requires connection ID
        list_connections_action(),
    ]
}
```

These need to look up connection from AppState because they don't have direct network context.

#### Common Actions (Always Available)
Available everywhere (user commands, network events, tasks):
- `show_message` - Display to user
- `open_server` - Start new server
- `close_server` - Stop server
- `update_instruction` - Change LLM prompt
- `set_memory` / `append_memory` - Persistent context
- `schedule_task` - Create scheduled task

### 3.3 Action Execution Flow

#### Network Event Path (Sync Actions)
```
Connection receives data
    ↓
TCP Server calls action_helper::call_llm()
    ↓
call_llm_with_actions(
    llm_client,
    state,
    server_id,
    Some(connection_id),  // Network context!
    "tcp_data_received",  // Event type
    {...event context...},
    Some(&protocol),      // Protocol sync actions
    vec![],               // No custom actions
    Some(received_data)
)
    ↓
Try Script First:
  - Check if script config handles this event type
  - If yes: execute script, return actions
  - If script fails: fall through to LLM
    ↓
Build LLM Prompt:
  1. System prompt with server instruction
  2. Available actions (common + protocol sync)
  3. Network context (connection, received data)
    ↓
Call LLM with tools (web search, file read, etc)
    ↓
Parse ActionResponse: {"actions": [...]}
    ↓
Execute Actions:
  1. Try CommonAction::from_json(action)
  2. If not common: try protocol.execute_action(action)
  3. Returns ActionResult (Output, WaitForMore, CloseConnection, etc)
    ↓
Process ActionResults:
  - Output: send bytes on connection
  - WaitForMore: enter Accumulating state, queue data
  - CloseConnection: close socket
  - Custom: protocol-specific handling
```

#### User Input Path (Async Actions)
```
User types "send hello to connection 5"
    ↓
EventHandler processes UserCommand::Interpret
    ↓
LLM interprets with:
  - System: "You control network servers"
  - Available: common actions + async actions for all servers
  - Context: list of active servers, connections
    ↓
LLM returns action (e.g., send_to_connection)
    ↓
Action Executor:
  - execute_common_action() for common actions
  - protocol.execute_action() for protocol actions
    ↓
Some async actions require special handling:
  - open_server: LLM can't spawn (needs tokio task)
    → Rolling TUI handles specially
  - close_server: Similar special handling
```

### 3.4 ActionResult Enum
**File**: `src/llm/actions/protocol_trait.rs`

```rust
pub enum ActionResult {
    Output(Vec<u8>),              // Send these bytes
    CloseConnection,              // Close this connection
    WaitForMore,                  // Enter Accumulating state
    NoAction,                      // Nothing to do
    Multiple(Vec<ActionResult>),  // Combine multiple results
    Custom {
        name: String,
        data: serde_json::Value,  // Protocol-specific JSON
    },
}
```

**Custom Result Example** (MySQL):
```json
{
  "type": "custom",
  "name": "mysql_query_result",
  "data": {
    "columns": ["id", "name"],
    "rows": [
      [1, "Alice"],
      [2, "Bob"]
    ]
  }
}
```

---

## 4. EVENT SYSTEM

### 4.1 Event Flow Architecture
**File**: `src/events/` directory

```rust
pub enum AppEvent {
    UserCommand(UserCommand),    // User input
    Tick,                        // 100ms timer for UI
    Shutdown,                    // Signal to exit
}

pub enum UserCommand {
    // Slash commands (parsed by CLI)
    Status,                       // /status
    ChangeModel { model: String }, // /model qwen2
    ShowLogLevel,                 // /log
    ChangeLogLevel { level: String }, // /log debug
    ShowScriptingEnv,             // /script
    ChangeScriptingEnv { env: String }, // /script python
    ShowWebSearch,                // /web
    SetWebSearch { mode: WebSearchMode }, // /web on
    ShowDocs { protocol: Option<String> }, // /docs http
    Quit,                         // /quit
    UnknownSlashCommand { command: String }, // /typo
    
    // Non-slash input (goes to LLM)
    Interpret { input: String },  // "open http server on 8080"
}
```

### 4.2 Protocol Events (Event Types)
**File**: `src/protocol/event_type.rs`

Protocols define events they emit:
```rust
pub struct EventType {
    pub id: String,              // "http_request"
    pub description: String,     // When this event occurs
    pub actions: Vec<ActionDefinition>, // Actions available
}

// Example: TCP protocol
pub fn get_tcp_event_types() -> Vec<EventType> {
    vec![
        EventType {
            id: "tcp_connection_opened".to_string(),
            description: "New TCP connection accepted (if send_first=true)".to_string(),
            actions: vec![...sync actions...],
        },
        EventType {
            id: "tcp_data_received".to_string(),
            description: "Data received from client".to_string(),
            actions: vec![...sync actions...],
        },
    ]
}
```

### 4.3 Connection State Machine
**File**: `src/state/machine.rs` and per-protocol implementations

TCP connection states (Idle/Processing/Accumulating):
```rust
enum ConnectionState {
    Idle,           // Ready for new data
    Processing,     // LLM is generating response
    Accumulating,   // LLM said wait_for_more, buffering data
}
```

**State Machine Flow** (TCP):
```
Connection accepts
    ↓ (state = Idle)
Data received
    ↓ (state = Processing)
LLM call initiated
    ↓
While more data arrives:
  - Queue in ConnectionData.queued_data
    ↓
LLM responds with action
    ↓
If action == WaitForMore:
  - state = Accumulating
  - Wait for more data
  - When data arrives: merge with queued, restart LLM
    ↓
If action == Output or CloseConnection:
  - state = Idle (or close)
  - Process queued data if any
  - Loop back to data handling
```

---

## 5. ROLLING TUI - USER INTERFACE

### 5.1 TUI Architecture
**File**: `src/cli/rolling_tui.rs` (900+ lines)

```
Terminal
├─ Scrolling Region (lines 1-N)  ← Output scrolls here
│  └─ Status messages from servers
├─ Sticky Footer (last 3-4 lines) ← Always visible
│  ├─ Status bar
│  ├─ Footer with connection info
│  └─ Input line
```

**Key Components**:
- `EventStream` - Crossterm keyboard events
- `Footer` - Sticky footer management
- `InputState` - Multi-line input buffer
- `StickyFooter` - Connection info display

### 5.2 Main Event Loop
```rust
loop {
    tokio::select! {
        // Handle keyboard input
        Some(Ok(event)) = event_stream.next() => {
            handle_event(event, &mut app, &state, &mut event_handler, ...)
        }

        // Web search approval requests
        Some(request) = web_approval_rx.recv() => {
            footer.pending_approval = Some(request)
        }

        // Periodic UI updates (100ms)
        _ = tick_interval.tick() => {}

        // Execute scheduled tasks (1s)
        _ = task_execution_interval.tick() => {
            execute_due_tasks(&state, &llm_client, &status_tx)
        }

        // Cleanup old servers/connections (5s)
        _ = cleanup_interval.tick() => {
            state.cleanup_old_servers(...)
            state.cleanup_closed_connections(...)
        }

        // Server status messages (from spawned tasks)
        Ok(msg) = status_rx.try_recv() => {
            if msg == "__UPDATE_UI__" {
                update_ui_from_state(&mut app, &state, &mut footer)
            } else {
                print_output_line(&msg, &mut footer, &palette)
            }
        }
    }
}
```

### 5.3 Status Channel Communication
**Pattern**: Channels for async status updates

Every spawned server task has `status_tx: mpsc::UnboundedSender<String>`:

```rust
// In TCP server handler
let _ = status_tx.send(format!("[DEBUG] TCP received {} bytes", n));
let _ = status_tx.send("__UPDATE_UI__".to_string());

// In TUI event loop
while let Ok(msg) = status_rx.try_recv() {
    if msg == "__UPDATE_UI__" {
        // Update UI from current state
    } else {
        // Display message
    }
}
```

This allows background tasks to:
- Log messages
- Update server state
- Trigger UI refresh
- Without blocking on TUI locks

---

## 6. LOGGING STRATEGY

### 6.1 Dual Logging Pattern (CRITICAL)
**Pattern**: All significant operations log BOTH ways:

```rust
// 1. Tracing macro (logs to netget.log file)
debug!("TCP received {} bytes: {}", n, preview);

// 2. Status channel (displays in TUI)
let _ = status_tx.send(format!("[DEBUG] TCP received {} bytes: {}", n, preview));
```

**Log Levels** (used by TUI filtering):
- `[ERROR]` - Critical failures (always shown)
- `[WARN]` - Non-fatal issues (Warn level+)
- `[INFO]` - Lifecycle events (Info level+)
- `[DEBUG]` - Operation summaries (Debug level+)
- `[TRACE]` - Full payloads (Trace level+)

**Example** (TCP data):
```rust
if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
    // Text data
    let data_str = String::from_utf8_lossy(&data);
    let preview = if data_str.len() > 100 {
        format!("{}...", &data_str[..100])
    } else {
        data_str.to_string()
    };
    debug!("TCP received {} bytes: {}", n, preview);           // File
    status_tx.send(format!("[DEBUG] TCP received {} bytes: {}", n, preview)); // TUI
    trace!("TCP data (text): {:?}", data_str);                // File only
} else {
    // Binary data
    debug!("TCP received {} bytes (binary data)", n);          // File
    status_tx.send(format!("[DEBUG] TCP received {} bytes (binary data)", n)); // TUI
    let hex_str = hex::encode(&data);
    trace!("TCP data (hex): {}", hex_str);                    // File only
}
```

### 6.2 Log File Output
**Per-Protocol**: Each server can create log files:
```rust
let log_path = server.get_or_create_log_path(&output_name);
// Returns: netget_output_name_YYYY_MM_DD_HH_MM_SS.log
```

Protocols can append to logs via action:
```json
{
  "type": "append_to_log",
  "output_name": "dns_queries",
  "content": "Resolved example.com to 1.2.3.4"
}
```

---

## 7. TASK SCHEDULING SYSTEM

### 7.1 Scheduled Tasks
**File**: `src/state/task.rs`

```rust
pub struct ScheduledTask {
    pub id: TaskId,
    pub name: String,
    pub scope: TaskScope,           // Global/Server/Connection
    pub task_type: TaskType,        // OneShot/Recurring
    pub instruction: String,         // LLM instruction
    pub context: Option<serde_json::Value>,
    pub status: TaskStatus,         // Scheduled/Executing/Completed/Failed
    pub next_execution: Instant,
    pub last_error: Option<String>,
    pub failure_count: u64,
}

pub enum TaskScope {
    Global,                         // Any LLM action
    Server(ServerId),              // Server's protocol actions
    Connection(ServerId, ConnectionId), // Specific connection actions
}

pub enum TaskType {
    OneShot { delay_secs: u64 },
    Recurring {
        interval_secs: u64,
        max_executions: Option<u64>,
        executions_count: u64,
    },
}
```

### 7.2 Task Creation (via Action)
```json
{
  "type": "schedule_task",
  "name": "cleanup_logs",
  "scope": { "type": "server", "server_id": 1 },
  "task_type": {
    "type": "recurring",
    "interval_secs": 300,
    "max_executions": null
  },
  "instruction": "Clean up old log files"
}
```

### 7.3 Task Execution
Every 1 second, TUI checks for due tasks and LLM executes them:
```rust
// rolling_tui.rs event loop
_ = task_execution_interval.tick() => {
    execute_due_tasks(&state, &llm_client, &status_tx).await;
}
```

---

## 8. CONVERSATION TRACKING

### 8.1 Conversation Info
**File**: `src/state/app_state.rs` (lines 42-54)

```rust
pub struct ConversationInfo {
    pub id: String,
    pub source: ConversationSource,
    pub details: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
}

pub enum ConversationSource {
    User,                               // Direct user input
    Network { server_id, connection_id }, // Network event
    Task { task_name },                 // Scheduled task
    Scripting,                          // Script mode
}
```

### 8.2 Conversation Lifecycle
```
1. Network event arrives (data received on TCP)
   ↓
2. state.register_conversation(id, source, details)
   ↓
3. LLM processes event
   ↓
4. state.end_conversation(id)
   ↓
5. TUI displays active conversations for last 1 second
```

UI shows "Inputs" column with active/recent conversations.

---

## 9. EXISTING PATTERNS FOR CLIENT IMPLEMENTATION

### 9.1 What Already Exists (Server-side)

**Fully Implemented**:
- AppState management (thread-safe, multi-server)
- Event system (user commands, network events)
- Action execution (common + protocol-specific)
- Server registry (feature-gated protocols)
- Logging system (dual logging pattern)
- TUI (rolling terminal with input/output)
- Task scheduling
- Conversation tracking

**Not Implemented**:
- Mode::Client (defined in AppState but never used)
- Client protocol implementations
- Client-side connection handling
- Client event types (different from server events)
- Client action system

### 9.2 Architecture Decisions for Client Mirrors

**Pattern 1: Reuse AppState for Client Servers**
- AppState already supports Mode::Client
- Could add "ClientConnection" to ServerInstance for tracking
- Same memory/instruction/protocol_data pattern works

**Pattern 2: New Client Module Structure**
```
src/client/
├─ connection.rs         // Client-side connection lifecycle
├─ protocols/
│  ├─ tcp/
│  │  ├─ mod.rs
│  │  └─ actions.rs
│  ├─ http/
│  │  ├─ mod.rs
│  │  └─ actions.rs
│  └─ ...
├─ action_helper.rs      // Like llm/action_helper.rs but client-side
└─ event_types.rs        // Client event types (different from server)
```

**Pattern 3: Client Actions vs Server Actions**
```
Server Actions (sync - network events):
- send_tcp_data
- close_this_connection
- wait_for_more

Client Actions (async - user/task triggered):
- connect_to_server
- send_data_on_connection
- disconnect
- close_all_connections
```

**Pattern 4: Client Event Types**
```
Different from server events:
- tcp_connected: established connection to remote
- tcp_data_received: received response from server
- tcp_disconnected: lost connection
- http_response_received: HTTP response parsed
- dns_response_received: DNS response
```

**Pattern 5: Client Connection vs Server Connection**
```
ServerInstance tracks:
- Outbound connections to remote servers (TCP, HTTP)
- Connection state (connected, data received, error)
- Response data (buffered)

Per-Client-Connection (like ServerConnectionState):
- Remote address
- Local address
- Bytes sent/received
- Status
- Protocol-specific info (HTTP headers, DNS response, etc)
```

---

## 10. KEY FILES AND THEIR ROLES

### State Management
- `src/state/app_state.rs` (1378 lines) - Global state with RwLock
- `src/state/server.rs` (370 lines) - ServerInstance, ConnectionState
- `src/state/machine.rs` (59 lines) - Generic state machine
- `src/state/task.rs` (150+ lines) - Scheduled tasks

### Action System
- `src/llm/actions/protocol_trait.rs` (232 lines) - Server trait
- `src/llm/actions/executor.rs` (300+ lines) - Action execution
- `src/llm/actions/common.rs` (400+ lines) - Common actions
- `src/llm/actions/tools.rs` (400+ lines) - Tool actions (web search, files)

### Server Startup & Management
- `src/cli/server_startup.rs` (178 lines) - Server spawning logic
- `src/protocol/registry.rs` (400+ lines) - Protocol registry
- `src/protocol/spawn_context.rs` - SpawnContext passed to protocols

### Event Handling
- `src/events/mod.rs` - Event module exports
- `src/events/types.rs` (205 lines) - UserCommand, AppEvent
- `src/events/handler.rs` (150+ lines) - Event handling

### UI
- `src/cli/rolling_tui.rs` (900+ lines) - Main TUI loop
- `src/cli/sticky_footer.rs` - Footer rendering
- `src/cli/input_state.rs` - Multi-line input handling

### Protocol Examples
- `src/server/tcp/mod.rs` - TCP server (300+ lines)
- `src/server/tcp/actions.rs` - TCP actions
- `src/server/http/mod.rs` - HTTP server
- `src/server/http/actions.rs` - HTTP actions
- `src/server/http/CLAUDE.md` - Documentation

### LLM Integration
- `src/llm/action_helper.rs` (300+ lines) - Call LLM with actions
- `src/llm/conversation.rs` - Multi-turn conversation handling
- `src/llm/ollama_client.rs` - Ollama API client
- `src/llm/prompt.rs` - Prompt building

---

## 11. CRITICAL PATTERNS & ANTIPATTERNS

### CRITICAL Patterns

**1. Never hold Mutex during I/O**
```rust
// WRONG - deadlock risk
let server = state.get_server(id).await; // Holds lock
send_data(server).await;                  // I/O under lock

// RIGHT - release lock before I/O
let server = state.get_server(id).await;  // Gets copy, releases lock
drop(server);                              // Explicit drop
send_data().await;                         // I/O without lock
```

**2. Use RwLock.read() for read-only, .write() for mutation**
```rust
// Correct pattern
let mode = state.get_mode().await;        // Uses .read()
state.set_mode(mode).await;               // Uses .write()
```

**3. Per-connection state machine prevents concurrent LLM calls**
```rust
// Ensures no data loss, ordered processing
if conn_state == Idle {
    conn_state = Processing
    await llm_call
    conn_state = Idle
    process_queued_data()
}
```

**4. Protocol-independent via trait**
```rust
// No central protocol enum
// Each protocol implements Server trait
// Registry stores Arc<dyn Server>
// Protocol-specific data in JSON, not enum
```

**5. Dual logging (tracing + status_tx)**
```rust
// All important operations log both:
debug!(...);                     // File
status_tx.send(...);            // TUI
```

### ANTIPATTERNS to Avoid

**1. Don't use feature flags for per-protocol config**
```rust
// WRONG
#[cfg(feature = "tcp")]
pub struct ServerInstance {
    tcp_data: TcpData,
}

// RIGHT
pub struct ServerInstance {
    protocol_data: serde_json::Value,  // Works for all protocols
}
```

**2. Don't clone TcpStream or write_half carelessly**
```rust
// WRONG
let stream = stream.clone();   // Can't clone TcpStream

// RIGHT
let (read, write) = tokio::io::split(stream);
let write_arc = Arc::new(Mutex::new(write));
// Now can share write_half
```

**3. Don't return raw bytes from actions**
```rust
// WRONG - LLM can't parse binary
{
  "data": "SGVsbG8="  // Base64
}

// RIGHT - use structured data
{
  "method": "GET",
  "path": "/",
  "headers": {...}
}
```

**4. Don't forget feature gates**
```rust
// All tests MUST be feature gated
#[cfg(all(test, feature = "tcp"))]
mod tcp_tests {
    ...
}
```

**5. Don't block on `.lock()` synchronously**
```rust
// WRONG - Tokio doesn't handle this well
let guard = mutex.lock().unwrap();

// RIGHT - use async lock
let guard = mutex.lock().await;
```

---

## 12. MULTI-INSTANCE CONCURRENCY PATTERNS

### Concurrent Instances
- `cargo-isolated.sh` - Uses session-specific `target-claude-$$`
- Different instances use different build directories
- Safe to run multiple NetGet instances simultaneously

### Ollama Lock
- `--ollama-lock` flag serializes LLM API calls
- Default in tests to avoid rate limiting
- Uses distributed lock file approach

### Task Safety
- Each instance has unique `instance_id: String`
- Uses PID + timestamp + random bytes
- Embedded in log messages for correlation

---

## 13. PRIVILEGE HANDLING

### System Capabilities Detection
```rust
pub struct SystemCapabilities {
    is_root: bool,
    has_cap_net_raw: bool,          // For raw sockets
    has_cap_net_bind_service: bool, // For privileged ports
}
```

### Privilege Requirements
```rust
pub enum PrivilegeRequirement {
    None,
    PrivilegedPort(u16),       // Port < 1024
    RawSockets,                 // For ARP, raw IP
    Root,                       // Everything
}
```

Checked before server spawn in `cli/server_startup.rs`.

---

## SUMMARY TABLE: SERVER vs CLIENT ARCHITECTURE

| Aspect | Server | Client (To Implement) |
|--------|--------|----------------------|
| **State** | ServerInstance | ClientInstance |
| **Connections** | Inbound from clients | Outbound to servers |
| **Actions (Sync)** | Response to network events | Similar: response to received data |
| **Actions (Async)** | Control from user/tasks | Similar: initiate connections |
| **Event Types** | tcp_data_received, http_request | tcp_connected, response_received |
| **Connection State** | Idle/Processing/Accumulating | Connected/SendingRequest/ReceivingResponse |
| **Protocol Base** | Listen socket | Connect socket |
| **Status Channel** | Server → TUI | Client → TUI |
| **Registry** | Feature-gated protocols | Similar feature-gated |
| **AppState** | Existing | Reuse/extend |

