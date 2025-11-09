# NetGet - LLM-Controlled Network Protocol Server & Client

Rust CLI where an LLM (via Ollama) controls 40+ network protocols as both servers and clients. The LLM constructs raw protocol datagrams or high-level responses.

## Protocols (50+)

**Beta**: TCP, HTTP, UDP, DataLink, DNS, DoT, DoH, DHCP, NTP, SNMP, SSH, OpenAI
**Experimental**: IRC, Telnet, SMTP, IMAP, mDNS, LDAP, MySQL, PostgreSQL, Redis, Cassandra, DynamoDB, Elasticsearch, IPP, WebDAV, NFS, SMB, HTTP Proxy, SOCKS5, STUN, TURN, Tor Directory, gRPC, MCP, JSON-RPC, XML-RPC, VNC, etcd, Kafka, MQTT, Git, S3, SQS, BOOTP
**Stable**: WireGuard (full VPN), Tor Relay
**Incomplete**: OpenVPN (honeypot), IPSec (honeypot), BGP

See `/docs` command for protocol details and metadata. Use `METADATA_EXAMPLES.md` for classification reference.

See protocol-specific docs: `src/server/<protocol>/CLAUDE.md`, `tests/server/<protocol>/CLAUDE.md`

## Architecture Principles

**Decentralization (CRITICAL)**: Never create centralized protocol registries. Use trait-based patterns where each protocol implements traits independently. Exceptions: Protocol registry (`protocol/registry.rs`), `Cargo.toml` features, `server_startup.rs` match statements.

**Modules**: `cli/` (TUI), `server/<protocol>/` (server implementations), `client/<protocol>/` (client implementations), `protocol/` (registry, metadata), `state/` (app state), `llm/` (Ollama), `events/` (coordination), `llm/actions/` (action system)

**Connection**: TcpStream split with `tokio::io::split()`. Never hold Mutex during I/O (deadlock risk).

**Data Queueing**: Per-connection state machine (Idle → Processing → Accumulating) prevents concurrent LLM calls.

**Actions**: Protocols implement `ProtocolActions` trait with async (user-triggered) and sync (network event) actions. Files: `src/server/<protocol>/actions.rs`

**Actions/Events Design (CRITICAL)**: NEVER use bytes (`Vec<u8>`) or base64-encoded strings in action parameters or event data. LLMs cannot effectively parse or construct binary data. Instead, use structured data (JSON objects, fields, enums) that you construct into bytes. Example: Instead of `{"data": "SGVsbG8="}`, use `{"method": "GET", "path": "/", "headers": {...}}`.

**Protocol Memory (CRITICAL)**: Protocols should NOT use any storage layer. The LLM returns all needed items via actions, scripts, or static responses. Example: MySQL protocol does not have actual data stored - the LLM answers all SQL queries via `answer` action, script mode, or static response. Do not implement databases, file systems, or persistent storage within protocols. Let the LLM's memory and instruction handle all state and data.

## Protocol Documentation (CRITICAL)

Each protocol has TWO CLAUDE.md files:
- `src/server/<protocol>/CLAUDE.md` - Implementation (library choices, architecture, LLM integration, limitations)
- `tests/server/<protocol>/CLAUDE.md` - Testing (strategy, LLM call budget, runtime, known issues)

**Always read both before modifying a protocol.**

## Testing Philosophy

Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients. **Status**: Unit 12/12 passing, E2E infrastructure fixed. See `TEST_INFRASTRUCTURE_FIXES.md`, `TEST_STATUS_REPORT.md`.

### Organization & Feature Gating (CRITICAL)

- All tests in `tests/` (never `src/`), access public APIs only
- Protocol E2E tests: `tests/server/<protocol>/e2e_test.rs`
- **ALL tests MUST be feature-gated**: `#[cfg(all(test, feature = "<protocol>"))]` in mod.rs
- Unit tests (no Ollama): `tests/base_stack_test.rs` (registry parsing), etc.
- E2E tests (Ollama required): Real clients, use `{AVAILABLE_PORT}` placeholder

### Running Tests

```bash
# Unit tests
./cargo-isolated.sh test --lib

# Protocol-specific E2E (ALWAYS use --features, never run all tests)
./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test
```

### E2E Test Efficiency (CRITICAL)

**Minimize LLM calls** (< 10 per suite): Reuse servers, use scripting mode, bundle scenarios. **Setup**: `./cargo-isolated.sh build --release --all-features`. **Privacy**: Localhost only (127.0.0.1/::1), no external endpoints.

## Multi-Instance Concurrency

**Ollama Lock**: `--ollama-lock` serializes LLM API (default in tests). **Safe**: Multiple E2E tests/instances with lock. **Unsafe**: Same `target/` (use `cargo-isolated.sh`), concurrent git (use worktrees).

### Build Isolation (CRITICAL)

**Use `./cargo-isolated.sh`** (session-specific `target-claude/claude-$$`). **Kill**: `./cargo-isolated-kill.sh` (NEVER `pkill cargo`). **Speed**: Fast with `--no-default-features --features <protocol>` (10-30s), slow with `--all-features` (1-2min). **Cleanup**: `rm -rf target-claude/`.

### Build Performance & Feature Flags (CRITICAL)

**ALWAYS use minimal features unless you explicitly need all protocols.** Default to feature-specific builds:

```bash
# FAST: Single protocol testing (10-30s)
./cargo-isolated.sh test --no-default-features --features tcp --test server::tcp::e2e_test

# FAST: Multiple related protocols (20-40s)
./cargo-isolated.sh build --no-default-features --features tcp,http,dns

# SLOW: Only use when absolutely needed (1-2min+, uses 3GB+ RAM)
./cargo-isolated.sh build --all-features
```

**Why this matters**:
- `--all-features` compiles 50+ protocols with all their dependencies (2GB+ code)
- sccache helps but cannot eliminate compilation of all crates
- Multiple concurrent cargo processes without feature limiting = resource thrashing
- Default: Use protocol-specific features for development, `--all-features` only for CI/release builds

**Before building**, ask: "Do I need ALL protocols for this task?" If not, use `--no-default-features --features <protocol>`.

**Common workflows**:
- Testing a protocol: `--no-default-features --features <protocol>`
- Modifying shared code: `--no-default-features --features tcp,http,dns` (representative subset)
- Full validation: `--all-features` (use sparingly)

### Claude Code for Web Environment (CRITICAL)

**Detection**: Claude Code for Web can be detected via environment variables:
- Primary: `CLAUDE_CODE_REMOTE=true` (most reliable)
- Secondary: `CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE=cloud_default`
- Tertiary: `CLAUDE_CODE_ENTRYPOINT=remote` or `IS_SANDBOX=yes`

**Bluetooth-BLE Restriction**: The `bluetooth-ble` feature MUST be skipped in Claude Code for Web:
- Depends on system library `libbluetooth-dev` which is not available in the web environment
- Attempting to build with `bluetooth-ble` feature will fail with missing library errors
- Always use `--no-default-features` with explicit feature selection in Claude Code for Web
- Avoid `--all-features` in Claude Code for Web as it includes `bluetooth-ble`

**Example safe builds for Claude Code for Web**:
```bash
# SAFE: Explicit features without bluetooth-ble
./cargo-isolated.sh build --no-default-features --features tcp,http,dns

# SAFE: Single protocol
./cargo-isolated.sh build --no-default-features --features tcp

# UNSAFE: Will try to build bluetooth-ble
./cargo-isolated.sh build --all-features  # DON'T USE IN WEB
```

**Detection**: Use the provided script to check your environment:
```bash
./am_i_claude_code_for_web.sh
```

This script checks all detection methods and provides build guidance. You can also check manually in code:
```bash
if [ "$CLAUDE_CODE_REMOTE" = "true" ]; then
    echo "Running in Claude Code for Web - skipping bluetooth-ble"
    ./cargo-isolated.sh build --no-default-features --features tcp,http,dns
else
    ./cargo-isolated.sh build --all-features
fi
```

### Efficient Build & Test Iteration (CRITICAL)

**Building and testing takes a long time** (10s-2min depending on features). **NEVER rebuild/retest after each individual fix.** Instead:

**Automatic Logging**: `./cargo-isolated.sh` automatically logs all output to `./tmp/netget-<command>-$$.log` and displays the log path. Use `./cargo-isolated.sh --print-last` to view the last log.

1. **Build/test and view output**:
```bash
# Build and pipe to see last 50 lines (automatically logged to ./tmp/netget-build-$$.log)
./cargo-isolated.sh build --no-default-features --features tcp | tail -50

# Test and pipe to see last 50 lines (automatically logged to ./tmp/netget-test-$$.log)
./cargo-isolated.sh test --no-default-features --features tcp | tail -50
```

2. **Analyze saved log for ALL errors**:
```bash
# View more of the log (last 100 lines)
./cargo-isolated.sh --print-last | tail -100

# Find all compilation errors
./cargo-isolated.sh --print-last | grep "error\[E"

# Get error summary (count by type)
./cargo-isolated.sh --print-last | grep "^error\[E" | sed 's/:.*$//' | sort | uniq -c | sort -rn

# Find specific error types
./cargo-isolated.sh --print-last | grep "error\[E0425\]"  # Unresolved names
./cargo-isolated.sh --print-last | grep "error\[E0599\]"  # Method not found

# Find test failures
./cargo-isolated.sh --print-last | grep "FAILED"
./cargo-isolated.sh --print-last | grep "assertion"
```

3. **Fix ALL issues before rebuilding**:
   - Analyze the complete log using `./cargo-isolated.sh --print-last`
   - Identify ALL problems (compilation errors, test failures, warnings)
   - Fix everything in a single batch
   - Only rebuild/retest once after all fixes are applied

**Anti-pattern** (wasteful):
```bash
# DON'T do this - rebuilds after every single fix
./cargo-isolated.sh build | tail -50  # Error 1 found
# Fix error 1
./cargo-isolated.sh build | tail -50  # Error 2 found (wasted 30s)
# Fix error 2
./cargo-isolated.sh build | tail -50  # Error 3 found (wasted another 30s)
# ... (wastes hours)
```

**Correct approach**:
```bash
# Build once and view last 50 lines (full log saved automatically)
./cargo-isolated.sh build --no-default-features --features tcp | tail -50

# Analyze ALL errors in the saved log
./cargo-isolated.sh --print-last | grep "error\[E"  # Shows all 15 errors

# Fix all 15 errors in code

# Rebuild once
./cargo-isolated.sh build --no-default-features --features tcp | tail -50
```

**Time savings**: Fixing 10 errors one-by-one = 10-20 minutes. Fixing all at once = 30 seconds + one build.

**Log files**: Located in `./tmp/netget-<command>-<pid>.log`. Use `./cargo-isolated.sh --print-last` to view the most recent log.

## Logging (CRITICAL)

**Dual logging**: ALL logs to tracing macros (`debug!`, `trace!`, etc.) → `netget.log` AND `status_tx.send()` → TUI. **Levels**: ERROR (critical), WARN (non-fatal), INFO (lifecycle), DEBUG (summaries), TRACE (payloads).

## UI & Technical Details

**TUI**: Rolling terminal, sticky footer, Ctrl+L (log levels), Ctrl+W (web search), multi-line (Shift+Enter). **Tech**: TcpStream via `tokio::io::split()` (never clone), never hold Mutex during I/O, default model `qwen3-coder:30b`, flow: UserCommand → Parse → EventHandler → LLM → Protocol action.

## Scheduled Tasks

Tasks execute at intervals/delays. Three scopes: **Global** (any server, all actions), **Server** (specific server, auto-cleaned on close), **Connection** (specific connection, auto-cleaned on close).

**Connection tasks** for long-lived connections (SSH, WebSocket): idle timeouts, session cleanup, rate limiting, monitoring. Short-lived (HTTP GET) use server-level instead.

**Creation**: Via `open_server` action (`scheduled_tasks` array) or `schedule_task` action. Add `connection_id` for connection-scoped tasks. Parameters: `task_id`, `recurring`, `interval_secs`/`delay_secs`, `instruction`.

## Protocol Planning (Before Implementation)

Research: **Server library** (crate eval: compliance, maturity, LLM control), **Client library** (E2E testing), **LLM control points** (async vs sync actions), **Logging strategy**, **Example prompts** (comprehensive, basis for E2E).

## Protocol Implementation Checklist (CRITICAL: ALL protocols MUST be feature gated)

**IMPORTANT**: Protocols should NOT implement storage. The LLM returns all data via actions/scripts/static responses (e.g., MySQL protocol has no actual database - LLM answers all queries).

**12-Step Implementation**:
1. **protocol/registry.rs**: Register protocol implementation (feature-gated)
2. **rolling_tui.rs**: Add welcome message (state will be Experimental by default)
3. **src/server/<protocol>/mod.rs**: Implement server with dual logging, track connections
4. **src/server/<protocol>/actions.rs**: Implement `ProtocolActions` trait (async/sync actions)
5. **src/server/<protocol>/CLAUDE.md** (MANDATORY): Document implementation, libraries, LLM integration, limitations
6. **src/server/mod.rs**: Add feature-gated module declaration
7. **cli/server_startup.rs**: Add feature-gated match arm
8. **state/server.rs**: Add `ProtocolConnectionInfo` variant
9. **Cargo.toml** (MANDATORY): Add feature flag, optional deps, include in all-protocols
10. **tests/server/<protocol>/e2e_test.rs**: Create feature-gated E2E test (< 10 LLM calls)
11. **tests/server/<protocol>/CLAUDE.md** (MANDATORY): Document test strategy, LLM budget, runtime
12. **tests/server/helpers.rs**: Update if needed

**Validation**: Compiles with feature, tests pass, both CLAUDE.md files exist, < 10 LLM calls

**Common Pitfalls**: Missing feature flags/gates, missing CLAUDE.md files, inefficient E2E tests, forgetting dual logging, using `--all-features` for single protocol

## Client Capability (NEW)

NetGet now supports LLM-controlled network **clients** in addition to servers. Clients connect to remote servers and allow the LLM to control sending data, interpreting responses, and making decisions based on server behavior.

### Client Architecture

**Client Trait System**: Mirrors server patterns with `Client` trait in `llm/actions/client_trait.rs`
- `connect()`: Establish connection and spawn LLM integration loop
- `get_async_actions()`: User-triggered actions (modify instruction, reconnect)
- `get_sync_actions()`: Response actions (send_data, disconnect, wait_for_more)
- `execute_action()`: Execute actions returning `ClientActionResult`

**Client State Management** (`state/client.rs`):
- `ClientInstance`: Client metadata (id, remote_addr, protocol, instruction, memory, status)
- `ClientId`: Unique identifier (u32)
- `ClientStatus`: Connecting, Connected, Disconnected, Error
- `ClientConnectionState`: Per-client LLM state (Idle/Processing/Accumulating)

**Client Registry** (`protocol/client_registry.rs`):
- `CLIENT_REGISTRY`: LazyLock registry of all client protocols
- Feature-gated registration (same as servers)
- Protocol lookup by name for `open_client` action

**EventType Constants**: Each client defines static `LazyLock<EventType>` constants:
- Example: `TCP_CLIENT_CONNECTED_EVENT`, `TCP_CLIENT_DATA_RECEIVED_EVENT`
- Used with `Event::new(&CONSTANT, json!(...))`
- Avoids string-based event type IDs

**LLM Integration**: Clients use `call_llm_for_client()` helper:
- Builds simple prompt with client instruction and available actions
- Uses `ConversationHandler` for action generation
- Returns `ClientLlmResult` with actions and optional memory updates
- No web search or complex tool calling (simplified for clients)

**State Machine**: Same as servers (Idle → Processing → Accumulating)
- Prevents concurrent LLM calls on same client
- Queues data during Processing state

### Client vs Server Differences

| Aspect | Server | Client |
|--------|--------|--------|
| **Initiates Connection** | No (listens) | Yes (connects) |
| **LLM Integration** | `call_llm()` with scripting support | `call_llm_for_client()` simplified |
| **Actions Result** | `ActionResult` enum | `ClientActionResult` enum |
| **State Location** | `state/server.rs` | `state/client.rs` |
| **Registry** | `PROTOCOL_REGISTRY` | `CLIENT_REGISTRY` |
| **Startup** | `cli/server_startup.rs` | `cli/client_startup.rs` |
| **TUI Display** | Left column | Middle column (3-column layout) |

### Implemented Client Protocols

**TCP Client** (`client/tcp/`):
- Direct socket I/O with hex-encoded data
- Actions: send_tcp_data (hex), disconnect, wait_for_more
- Events: tcp_connected, tcp_data_received

**HTTP Client** (`client/http/`):
- Uses reqwest library with TLS support
- Actions: send_http_request (method, path, headers, body)
- Events: http_connected, http_response_received
- Startup params: default_headers

**Redis Client** (`client/redis/`):
- Line-based RESP protocol parsing
- Actions: execute_redis_command (command string)
- Events: redis_connected, redis_response_received
- Simple synchronous request-response model

## Client Protocol Implementation Checklist (CRITICAL)

**IMPORTANT**: Client protocols should NOT implement storage. The LLM returns all data via actions/scripts/static responses (e.g., Redis client has no cache - LLM decides when to send commands).

**Before implementing a new client protocol:**
1. **Consult `CLIENT_PROTOCOL_FEASIBILITY.md`** - Review the feasibility assessment for your protocol
2. Check for existing Rust client libraries and complexity rating
3. Understand LLM control points and implementation strategy
4. Review similar protocol implementations for patterns

**12-Step Client Implementation**:
1. **protocol/client_registry.rs**: Register client protocol (feature-gated)
2. **src/client/<protocol>/mod.rs**: Implement connection with LLM integration
   - Define connection state machine (Idle/Processing/Accumulating)
   - Spawn read loop that calls `call_llm_for_client()`
   - Handle `ClientActionResult` enum (SendData, Disconnect, WaitForMore, Custom)
   - Use dual logging (tracing macros + status_tx)
3. **src/client/<protocol>/actions.rs**: Implement `Client` trait
   - Define static `LazyLock<EventType>` constants for events
   - Implement `connect()` spawning connection task
   - Implement `get_async_actions()` (user actions)
   - Implement `get_sync_actions()` (response actions)
   - Implement `execute_action()` parsing action JSON
   - Implement `get_event_types()` returning event type list
   - Implement `protocol_name()`, `stack_name()`, `get_startup_params()`
4. **src/client/<protocol>/CLAUDE.md** (MANDATORY): Document implementation
   - Library choices (crates used)
   - Architecture (connection model, state management)
   - LLM integration (action flow, event triggers)
   - Limitations and known issues
5. **src/client/mod.rs**: Add feature-gated module declaration
   - `#[cfg(feature = "<protocol>")] pub mod <protocol>;`
6. **cli/client_startup.rs**: Add feature-gated match arm
   - Match on protocol name, call protocol's connect method
7. **Cargo.toml** (MANDATORY): Add feature flag
   - `<protocol> = ["<dependencies>"]`
   - Mark dependencies as `optional = true`
   - Include in `all-protocols` feature
8. **tests/client/<protocol>/e2e_test.rs**: Create feature-gated test (< 10 LLM calls)
   - Test basic connectivity
   - Test LLM-controlled actions
   - Use `#[cfg(all(test, feature = "<protocol>"))]`
9. **tests/client/<protocol>/CLAUDE.md** (MANDATORY): Document test strategy
   - Test approach (unit vs E2E)
   - LLM call budget and rationale
   - Expected runtime
   - Known issues or flaky tests
10. **Export protocol**: Re-export from `client/<protocol>/mod.rs`
    - `pub use actions::XyzClientProtocol;`
    - Export only protocol struct, NOT event constants (to avoid duplicate imports)

**Validation**:
- Compiles with `--no-default-features --features <protocol>`
- Tests pass with `--features <protocol>`
- Both CLAUDE.md files exist
- < 10 LLM calls in test suite

**Common Pitfalls**:
- Exporting EventType constants from mod.rs (causes E0252 duplicate name errors)
- Missing Client trait import when calling execute_action()
- Using string event type IDs instead of static EventType constants
- Forgetting to call `protocol.as_ref()` when passing Arc<ClientProtocol> to trait methods
- Missing `parameters` field in EventType construction
- Not using dual logging (tracing + status_tx)

### Example: Adding a New Client Protocol (SSH)

```rust
// 1. src/client/ssh/actions.rs
use std::sync::LazyLock;

pub static SSH_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ssh_connected", "SSH client authenticated")
        .with_parameters(vec![...])
});

pub struct SshClientProtocol;

impl Client for SshClientProtocol {
    fn connect(&self, ctx: ConnectContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::ssh::SshClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action["type"].as_str()?;
        match action_type {
            "execute_command" => {
                let command = action["command"].as_str()?;
                Ok(ClientActionResult::Custom {
                    name: "ssh_command".to_string(),
                    data: json!({ "command": command }),
                })
            }
            // ... other actions
        }
    }
    // ... other trait methods
}

// 2. src/client/ssh/mod.rs
pub mod actions;
pub use actions::SshClientProtocol;

use crate::client::ssh::actions::{SSH_CLIENT_CONNECTED_EVENT, SSH_CLIENT_DATA_RECEIVED_EVENT};

impl SshClient {
    pub async fn connect_with_llm_actions(...) -> Result<SocketAddr> {
        // 1. Connect to SSH server
        // 2. Authenticate
        // 3. Call LLM with connected event
        let event = Event::new(&SSH_CLIENT_CONNECTED_EVENT, json!({...}));
        call_llm_for_client(..., Some(&event), ...).await?;
        // 4. Spawn read loop with state machine
        // 5. On data received, call LLM again
        // 6. Execute actions from LLM response
    }
}

// 3. protocol/client_registry.rs (add to register_protocols)
#[cfg(feature = "ssh")]
self.register(Arc::new(crate::client::ssh::SshClientProtocol::new()));

// 4. Cargo.toml
ssh = ["russh", "russh-keys"]
all-protocols = [..., "ssh"]

[dependencies]
russh = { version = "0.40", optional = true }
russh-keys = { version = "0.40", optional = true }
```

## Multi-Instance Collaboration (CRITICAL)

**Errors**: PAUSE if error in unmodified code. **Shared files** (`Cargo.toml`, `protocol/registry.rs`, `server/mod.rs`, `server_startup.rs`, `state/server.rs`): NEVER overwrite, use Edit tool, add incrementally. **Kill**: `./cargo-isolated-kill.sh` (NEVER `pkill cargo`).

## Git Commits

Only commit when user requests. DO NOT add AI references ("Generated with Claude Code", "Co-Authored-By"). Keep messages professional and concise.
