# NetGet - Knowledge Document

NetGet is a Rust CLI application where an LLM (via Ollama) controls network protocols.

**Critical Design Principle**: The LLM is in control - either constructing raw protocol datagrams or generating high-level responses based on the chosen stack.

## Base Protocol Stacks

### Core Protocols (Beta)
- **TCP** (`BaseStack::Tcp`) - LLM controls raw TCP data, constructs entire protocols (FTP, HTTP, custom)
- **HTTP** (`BaseStack::Http`) - LLM controls HTTP responses (status, headers, body); hyper handles parsing
- **UDP** (`BaseStack::Udp`) - LLM controls raw UDP datagrams
- **DataLink** (`BaseStack::DataLink`) - LLM controls layer 2 Ethernet frames (ARP, custom frames)
- **DNS** (`BaseStack::Dns`) - LLM generates DNS responses using hickory-dns
- **DoT** (`BaseStack::Dot`) - DNS-over-TLS server using hickory-dns with TLS (port 853)
- **DoH** (`BaseStack::Doh`) - DNS-over-HTTPS server using hickory-dns with HTTP/2 (port 443)
- **DHCP** (`BaseStack::Dhcp`) - LLM handles DHCP requests using dhcproto
- **NTP** (`BaseStack::Ntp`) - LLM handles time sync using ntpd-rs
- **SNMP** (`BaseStack::Snmp`) - LLM handles SNMP get/set using rasn-snmp
- **SSH** (`BaseStack::Ssh`) - LLM handles SSH auth and shell using russh

### Application Protocols (Alpha)
- **IRC** (`BaseStack::Irc`) - IRC chat server
- **Telnet** (`BaseStack::Telnet`) - Telnet terminal server using nectar
- **SMTP** (`BaseStack::Smtp`) - SMTP mail server (port 25)
- **IMAP** (`BaseStack::Imap`) - IMAP mail server (port 143/993 for TLS)
- **mDNS** (`BaseStack::Mdns`) - Multicast DNS service discovery (port 5353)
- **LDAP** (`BaseStack::Ldap`) - LDAP directory server (port 389)

### Database Protocols (Alpha)
- **MySQL** (`BaseStack::Mysql`) - MySQL server using opensrv-mysql (port 3306)
- **PostgreSQL** (`BaseStack::Postgresql`) - PostgreSQL server using pgwire (port 5432)
- **Redis** (`BaseStack::Redis`) - Redis server with RESP protocol (port 6379)
- **Cassandra** (`BaseStack::Cassandra`) - Cassandra/CQL database server (port 9042)
- **DynamoDB** (`BaseStack::Dynamo`) - DynamoDB-compatible server (port 8000)
- **Elasticsearch** (`BaseStack::Elasticsearch`) - Elasticsearch search engine (port 9200)

### Web & File Protocols (Alpha)
- **IPP** (`BaseStack::Ipp`) - Internet Printing Protocol server (port 631)
- **WebDAV** (`BaseStack::WebDav`) - WebDAV file server using dav-server
- **NFS** (`BaseStack::Nfs`) - NFSv3 file server using nfsserve (port 2049)
- **SMB** (`BaseStack::Smb`) - SMB/CIFS file server using smb-msg (port 445)

### Proxy & Network Protocols (Alpha)
- **HTTP Proxy** (`BaseStack::Proxy`) - HTTP/HTTPS proxy using http-mitm-proxy (port 8080/3128)
- **SOCKS5** (`BaseStack::Socks5`) - SOCKS5 proxy server (port 1080)
- **STUN** (`BaseStack::Stun`) - STUN server for NAT traversal (port 3478)
- **TURN** (`BaseStack::Turn`) - TURN relay server for NAT traversal (port 3478)

### VPN Protocols
- **WireGuard** (`BaseStack::Wireguard`) - ✅ **Full VPN Server** with tunnel support using defguard_wireguard_rs (port 51820)
- **OpenVPN** (`BaseStack::Openvpn`) - ⚠️ **Honeypot Only** - Detection and logging (port 1194)
- **IPSec/IKEv2** (`BaseStack::Ipsec`) - ⚠️ **Honeypot Only** - Detection and logging (port 500/4500)
- **BGP** (`BaseStack::Bgp`) - Border Gateway Protocol routing server (port 179)

**VPN Implementation Status**: See `VPN_IMPLEMENTATION_STATUS.md` for detailed information about why WireGuard is the only fully-functional VPN server.

### AI & API Protocols (Alpha)
- **OpenAI** (`BaseStack::OpenAi`) - OpenAI-compatible API server (port 11435)

## Architecture

### Decentralization Principle - **CRITICAL**

**NEVER create centralized protocol registries or lists**. The codebase must remain decentralized with respect to protocol implementations.

**Anti-Pattern** (DO NOT DO THIS):
```rust
// ❌ WRONG - Centralized list of all protocols
fn get_all_protocol_metadata() -> Vec<ProtocolMetadata> {
    vec![
        ProtocolMetadata { name: "DNS", port: 53, ... },
        ProtocolMetadata { name: "HTTP", port: 80, ... },
        ProtocolMetadata { name: "SMTP", port: 25, ... },
        // ... requires updating this central list for every new protocol
    ]
}
```

**Correct Pattern** (DO THIS):
```rust
// ✅ CORRECT - Trait-based, decentralized
pub trait ProtocolServer {
    fn protocol_name(&self) -> &str;
    fn default_port(&self) -> u16;
    fn description(&self) -> &str;
    fn startup_params(&self) -> Vec<ActionDefinition>;
}

// Each protocol implements the trait independently
impl ProtocolServer for DnsServer {
    fn protocol_name(&self) -> &str { "DNS" }
    fn default_port(&self) -> u16 { 53 }
    // ...
}
```

**Why This Matters**:
- **Extensibility**: New protocols don't require modifying central lists
- **Maintainability**: Protocol metadata lives with the protocol implementation
- **Feature Gates**: Protocols can be compiled conditionally without breaking shared code
- **Plugin Architecture**: External protocols can be added without core changes

**Allowed Exceptions**:
- `BaseStack` enum in `src/protocol/base_stack.rs` - This is the protocol identifier, not metadata
- Feature flags in `Cargo.toml` - Build system requires this
- Match statements in `server_startup.rs` - Dispatching requires knowing which protocol to spawn

**When adding functionality across protocols**:
1. Define a trait in a shared location (e.g., `src/protocol/`)
2. Have each protocol implement the trait independently
3. Use trait bounds to access functionality generically
4. Never maintain a hardcoded list of all protocols

### Key Modules

- **`cli/`** - Rolling terminal TUI (like `tail -f`) with sticky footer showing input, model, scripting mode, and packet stats
  - `rolling_tui.rs` - Main TUI implementation with scrolling output region
  - `sticky_footer.rs` - Sticky footer UI component
  - `input_state.rs` - Multi-line input handling with history
  - `server_startup.rs` - Server spawning logic for all protocols
- **`server/`** - Protocol implementations organized by protocol (e.g., `server/imap/`, `server/ssh/`)
  - Each protocol has `mod.rs` (server implementation) and optionally `actions.rs` (action handlers)
  - Older protocols in root: `tcp.rs`, `http.rs`, `udp.rs`, `datalink.rs`
- **`protocol/`** - Base stack definitions (`base_stack.rs`)
- **`state/`** - App state (mode, stack, connections, user instructions, memory)
  - `app_state.rs` - Global state with RwLock for thread-safe access
  - `server.rs` - Server state and protocol-specific connection info
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

**Protocol action files**:
- New protocols: `src/server/<protocol>/actions.rs`
- Legacy protocols: `src/network/*_actions.rs` (older implementation pattern)

## Protocol-Specific Documentation

**CRITICAL**: When working with a specific protocol, ALWAYS check for protocol-specific CLAUDE.md files first.

### Where to Find Protocol Information

Each protocol has **two CLAUDE.md files** documenting different aspects:

**1. Implementation Documentation** (`src/server/<protocol>/CLAUDE.md`):
- Protocol overview and version/RFC compliance
- Library choices and rationale
- Architecture decisions
- LLM integration approach
- Connection and state management
- Known limitations
- Example prompts and responses

**2. Test Documentation** (`tests/server/<protocol>/CLAUDE.md`):
- Test strategy and structure
- LLM call budget breakdown
- Scripting mode usage
- Client library details
- Expected runtime and failure rate
- Test cases covered
- Known flaky tests or issues

### When to Consult Protocol Documentation

**Before modifying a protocol**:
1. Read `src/server/<protocol>/CLAUDE.md` to understand implementation decisions
2. Read `tests/server/<protocol>/CLAUDE.md` to understand test structure
3. Check if changes might affect LLM call budget or test runtime

**Before adding tests**:
1. Read `tests/server/<protocol>/CLAUDE.md` to understand existing test strategy
2. Ensure new tests follow established patterns (consolidation, scripting, etc.)
3. Update LLM call budget after adding tests

**When investigating test failures**:
1. Check `tests/server/<protocol>/CLAUDE.md` for known flaky tests
2. Verify expected runtime hasn't degraded
3. Check if failure is environmental or protocol-specific

### Example: Understanding DNS Protocol

```bash
# Read implementation details
cat src/server/dns/CLAUDE.md
# Learn: Uses hickory-dns, supports A/MX/AAAA/TXT records, scripting capable

# Read test details
cat tests/server/dns/CLAUDE.md
# Learn: 2 LLM calls total, scripting enabled, ~25s runtime, stable tests
```

**Note**: Not all protocols may have these files yet (especially legacy protocols). If missing, they should be created following the Protocol Implementation Checklist.

## Testing

**Philosophy**: Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients.

**✅ CURRENT TEST STATUS (2025-10-30):**
- **Unit Tests:** ✅ 12/12 PASSING (100%)
- **E2E Test Infrastructure:** ✅ FIXED - All compilation errors resolved
- **E2E Tests:** 🔄 READY TO RUN - Infrastructure repaired, tests compile successfully
- **See `TEST_INFRASTRUCTURE_FIXES.md` for fix details**
- **See `TEST_STATUS_REPORT.md` for comprehensive audit findings**

**Recent Fixes:**
- ✅ Added missing helper functions (`wait_for_server_startup`, `assert_stack_name`, `get_server_output`)
- ✅ Fixed module imports (imap/test.rs)
- ✅ Fixed async/await errors (ipsec/e2e_test.rs)
- ✅ All tests now compile successfully with 0 errors
- 🔄 Protocol status (Alpha/Beta) can now be verified through E2E test execution

**CRITICAL: Test Organization**:
- **ALL tests MUST be in the `tests/` directory, NEVER in `src/`**
- **Tests in `tests/` can ONLY access public APIs** - they are compiled as separate crates
- **Protocol E2E tests belong in `tests/server/<protocol>/`** - Each protocol gets its own directory
- **Shared helpers live in `tests/server/helpers.rs`** - Common test utilities
- Unit tests at `tests/` root test public functions, types, and modules (e.g., `base_stack_test.rs`, `logging_unit_test.rs`)
- **No `#[cfg(test)]` modules in `src/` files** - keep production code clean
- Tests that need private API access should be refactored to test through public interfaces

**CRITICAL: Feature Gating Tests**:
- **ALL tests (both E2E and unit tests) MUST be feature-gated** to match their protocol
- Tests compile the protocol code, so they need the same feature flags
- Failure to feature gate tests will cause compilation errors or slow builds

**Implementation**:
```rust
// tests/server/<protocol>/mod.rs - Module declaration
#[cfg(all(test, feature = "<protocol>"))]
pub mod e2e_test;

// tests/server/<protocol>/e2e_test.rs - Individual E2E tests (no additional gate)
#[tokio::test]
async fn test_protocol_feature() {
    // Test implementation
}

// tests/base_stack_test.rs - Protocol-specific unit tests
#[test]
#[cfg(feature = "<protocol>")]
fn test_parse_protocol_stack() {
    // Test implementation
}
```

**Examples**:
```rust
// tests/server/ssh/mod.rs - Module declaration
#[cfg(all(test, feature = "ssh"))]
pub mod e2e_test;

// tests/server/ssh/e2e_test.rs - E2E test (no additional feature gate)
#[tokio::test]
async fn test_ssh_authentication() {
    // Test implementation
}

// tests/base_stack_test.rs - Protocol parsing test
#[test]
#[cfg(feature = "ssh")]
fn test_parse_ssh_stack() {
    assert_eq!(registry().parse_from_str("ssh"), Some("SSH".to_string()));
}
```

**Why this matters**:
- Without feature gates, tests compile even when protocol is disabled → compilation errors
- Running `cargo test` without feature flags would try to compile all protocol tests
- Feature gates ensure tests only compile when their protocol is enabled

**Test Directory Structure**:
```
tests/
├── server/                           # Protocol E2E tests
│   ├── helpers.rs                    # Shared test helpers
│   ├── mod.rs                        # Module declarations
│   ├── <protocol>/                   # One directory per protocol
│   │   ├── mod.rs                    # Protocol test module
│   │   ├── e2e_test.rs               # Main E2E test
│   │   └── e2e_<variant>_test.rs     # Additional test variants
│   ├── tcp/
│   ├── http/
│   ├── ssh/
│   ├── smb/
│   └── ...
├── base_stack_test.rs                # Unit tests
├── logging_unit_test.rs              # Unit tests
├── e2e_footer_test.rs                # UI tests
└── ...
```

**Unit tests**: No Ollama required. Test parsing and detection logic.

**E2E tests** (`tests/server/<protocol>/`): Require Ollama + model. Use real clients (suppaftp, reqwest, ssh2, raw sockets).

**Test helper**: `start_server_with_prompt(prompt)` infers configuration and returns (state, port, handle).

**Dynamic ports**: Use port 0 in prompts for auto-assignment.

**Running tests**:
```bash
./cargo-isolated.sh test --lib                                        # Unit tests
./cargo-isolated.sh test --features tcp --test server::tcp::e2e_test # TCP E2E tests
./cargo-isolated.sh test --features http --test server::http::e2e_test # HTTP E2E tests
./cargo-isolated.sh test --features ssh --test server::ssh::e2e_test # SSH E2E tests
./cargo-isolated.sh test --features proxy --test server::proxy::e2e_test # Proxy E2E tests
```

**IMPORTANT - Protocol-Specific Testing**:
- **MUST run tests for your protocol using feature gates** - Never run the entire test suite
- E2E tests are extremely slow (each spawns NetGet process + makes LLM API calls)
- Running all tests can take 30+ minutes
- **Always use protocol-specific test command**:
  ```bash
  ./cargo-isolated.sh test --features <protocol> --test server::<protocol>::e2e_test
  ```
- Example for SSH: `./cargo-isolated.sh test --features ssh --test server::ssh::e2e_test`
- Example for HTTP Proxy: `./cargo-isolated.sh test --features proxy --test server::proxy::e2e_test`

### E2E Test Performance

**Critical**: E2E tests are slow because each test spawns a NetGet process and makes LLM API calls.

**Test Execution**:
- **Tests can run concurrently** (Ollama lock enabled by default in tests)
- The `--ollama-lock` flag serializes LLM API access, preventing overload
- Each test runs in isolation with dynamic port allocation
- Example: `./cargo-isolated.sh test --features http --test server::http::e2e_test`
- See "Multi-Instance Concurrency Support" section for details

**Critical setup requirement**:
- **MUST build release binary with all features before running tests**:
  ```bash
  ./cargo-isolated.sh build --release --all-features
  ```
- E2E tests spawn the release binary from session-specific `target-claude/claude-$$/release/netget`
- If the binary wasn't built with all features, protocol tests will fail
- Symptom: Server starts as TCP stack instead of protocol-specific stack

**Script Control**:
- Use `--no-scripts` flag to disable script generation in tests
- Forces LLM to use action-based responses only
- Example: `ServerConfig::new_no_scripts(prompt)` in test code
- Prevents event_type_id mismatches between scripts and protocols

### E2E Test Efficiency - Minimizing LLM Calls

**CRITICAL**: Each LLM call is expensive in time and resources. Design E2E tests to minimize LLM invocations.

**LLM Call Points**:
1. **Server startup** - 1 LLM call to generate initial server logic
2. **Each network request** - 1 LLM call per request (unless scripting is used)
3. **Scripting mode** - 0 additional LLM calls after startup (script handles all requests)

**Best Practices**:

**1. Reuse Server Instances Across Test Cases**
- **NEVER** spin up a new server for each test case
- Provide comprehensive instructions in a single prompt covering all test scenarios
- Test multiple operations against the same server instance

**❌ Bad**: Separate servers for each test case (3+ server setups = 3+ startup LLM calls)
**✅ Good**: One server with comprehensive prompt covering all test cases (1 server setup = 1 startup LLM call)

**2. Target < 10 Total LLM Calls Per Protocol Test Suite**
- Count: 1 server startup + N requests (unless scripted)
- Prefer scripting mode (0 LLM calls per request)
- Example budget: 2-3 comprehensive server setups, 2-3 requests per server = 6-9 total LLM calls

**3. Use Scripting Mode When Available**
- Server startup generates script (1 LLM call), all subsequent requests use script (0 LLM calls)
- Good candidates: DNS, HTTP, DHCP

**4. Group Related Test Scenarios**
- Bundle CRUD operations, error cases, and edge cases into comprehensive tests
- Avoid: 10+ servers for 10+ test cases, testing each feature in isolation, ignoring scripting mode

### Privacy and Network Isolation Policy

**CRITICAL SECURITY REQUIREMENT**: NetGet must NOT leak information to external networks during testing or runtime unless explicitly requested by the user.

**Testing Requirements**:
1. **All tests MUST pass without internet access** - Tests must use local servers only
2. **No external endpoints** - Tests must NOT make requests to public services (e.g., httpbin.org, example.com)
3. **Self-contained test infrastructure** - Create local HTTP/HTTPS servers within test code
4. **Localhost only** - All test traffic must be to 127.0.0.1 or ::1

This policy ensures NetGet respects user privacy and works in isolated/air-gapped environments.

## Multi-Instance Concurrency Support

**OVERVIEW**: NetGet supports running multiple instances and E2E tests concurrently using the `--ollama-lock` flag.

### Ollama Lock Mechanism

**Purpose**: Serialize access to the Ollama API to prevent overload and timeouts when multiple instances run simultaneously.

**How it works**:
- When `--ollama-lock` is enabled, NetGet acquires an exclusive file lock on `./ollama.lock` before each LLM API call
- The lock is automatically released after the API call completes
- Other instances wait their turn, preventing concurrent API requests
- Lock file is created in the current directory (repo root) and persists across runs
- **Stale lock detection**: If a lock is older than 30 seconds, it's assumed stale and forcibly acquired
- **Lock file lifecycle**: Never deleted, only truncated and updated in-place to avoid race conditions
- **Cross-platform**: Uses `fs2` crate (flock on Unix, LockFileEx on Windows)
- **Git**: Lock file is gitignored

**Usage**:
```bash
# Enable Ollama locking for concurrent execution
netget --ollama-lock "listen on port 80 via http"

# Run E2E tests concurrently (locking enabled by default in tests)
./cargo-isolated.sh test --features http --test server::http::e2e_test
```

### E2E Test Concurrency

**Default behavior**: E2E tests enable `--ollama-lock` by default in `ServerConfig::new()`.

**Running tests in parallel**:
```bash
# Run all protocol tests concurrently (uses available CPU cores)
cargo test

# Run specific protocol tests concurrently
./cargo-isolated.sh test --features http,tcp,ssh

# Disable locking for serial execution (legacy behavior)
# Not recommended - modify ServerConfig::new() to set ollama_lock: false
```

**Performance**:
- Without locking: Tests may timeout or fail due to Ollama overload
- With locking: Tests run reliably in parallel, utilizing available CPU cores
- Lock acquisition is logged at DEBUG level: "Acquiring Ollama lock" / "Ollama lock acquired" / "Ollama lock released"

### Automatic Instance ID Generation

Each NetGet process automatically generates a unique instance ID:
- Format: `claude-{pid}-{timestamp}-{random4}`
- Used for potential future instance-specific features (logging, metrics, etc.)
- Stored in AppState, accessible via `get_instance_id()`

### Concurrency Safety

**What's safe**:
- ✅ Running multiple E2E test files concurrently
- ✅ Running multiple NetGet instances with `--ollama-lock`
- ✅ Mixing interactive and non-interactive instances

**What's NOT safe without additional setup**:
- ❌ Running multiple instances building to the same `target/` directory
  - **Solution**: Set `CARGO_TARGET_DIR` to use session-specific build directories
  - **At session start**, run once:
    ```bash
    export CARGO_TARGET_DIR="$(pwd)/target-claude/claude-$$"
    ```
  - This creates: `target-claude/claude-{shell_pid}/` (one per session)
  - Already gitignored via `/target-claude/` pattern
  - See "Build Directory Management" section below for details and automation options
- ❌ Modifying shared git state (commits, branch switches) simultaneously
  - Solution: Use git worktrees for true isolation
  - Example: `git worktree add /tmp/netget-claude-2`

### Temporary File Isolation

Test infrastructure automatically uses process-specific temp file names:
- HTTPS certificates: `test_https_cert_{pid}.pem`, `test_https_key_{pid}.pem`
- gRPC proto files: `test_grpc_{pid}.proto`, `test_grpc_descriptor_{pid}.pb`
- No manual cleanup needed - OS temp directory handles lifecycle

### Build Directory Management for Multiple Claude Instances

**CRITICAL**: When running multiple Claude instances, each MUST use a separate build directory to avoid build conflicts.

**Solution**: Use the `cargo-isolated.sh` wrapper script for ALL cargo commands.

**Usage**:
```bash
# Instead of: ./cargo-isolated.sh build --release --all-features
./cargo-isolated.sh build --release --all-features

# Instead of: cargo test
cargo test

# Instead of: ./cargo-isolated.sh check
./cargo-isolated.sh check
```

**How it works**:
- Script automatically sets `CARGO_TARGET_DIR` to `target-claude/claude-$$` (uses shell PID)
- **Session-level isolation**: All cargo commands in the same terminal session share one build directory
- **Cross-session isolation**: Different terminal sessions (different Claude instances) get separate directories
- No manual environment variable setup required
- Example: Terminal session with PID 12345 uses `target-claude/claude-12345/`

**IMPORTANT**: Always use `./cargo-isolated.sh` instead of direct `cargo` commands in this codebase.

**CRITICAL - Feature Flags for Fast Compilation**:
- **ALWAYS use specific feature flags when working on a single protocol** to avoid compiling all protocols
- Compiling with `--all-features` takes 1-2 minutes and compiles 40+ protocols
- Compiling with specific features takes 10-30 seconds and only compiles what you need
- Other protocols may have compilation errors that will block your work

**Examples**:
```bash
# ❌ SLOW - Compiles all 40+ protocols (~1-2 minutes)
./cargo-isolated.sh build --all-features
cargo test  # Uses default = all-protocols

# ✅ FAST - Compiles only what you need (~10-30 seconds)
./cargo-isolated.sh build --no-default-features --features http
./cargo-isolated.sh test --no-default-features --features grpc --test server::grpc::e2e_test
./cargo-isolated.sh check --no-default-features --features ssh,tcp

# Testing specific protocol (recommended pattern)
./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test

# Examples:
./cargo-isolated.sh test --no-default-features --features ssh --test server::ssh::e2e_test
./cargo-isolated.sh test --no-default-features --features http --test server::http::e2e_test
./cargo-isolated.sh test --no-default-features --features kafka --test server::kafka::e2e_test
```

**When to use `--all-features`**:
- Only when explicitly testing or building all protocols
- When preparing release binaries
- When running full integration test suite

**When to use `--no-default-features`**:
- When working on a specific protocol (99% of the time)
- When running protocol-specific E2E tests
- When you want fast iteration cycles

**Cleanup** (builds accumulate ~2-5 GB each):
```bash
# Remove all isolated build directories
rm -rf target-claude/

# Remove old builds (10+ days)
find target-claude/ -maxdepth 1 -type d -mtime +10 -exec rm -rf {} \;
```

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

### Rolling Terminal TUI
- **Natural scrolling**: Output scrolls into terminal's scrollback buffer like `tail -f`
- **Sticky footer**: Input field and status bar remain fixed at bottom
- **Log level control**: Cycle through ERROR/WARN/INFO/DEBUG/TRACE with Ctrl+L
- **Web search toggle**: Ctrl+W toggles web search on/off (also `/web on|off` command)
- **Status bar**: Shows model, scripting mode (LLM/Python/JavaScript), web search status, packet stats

### Input & Navigation
- **Command history**: Up/Down arrows, saved to `~/.netget_history`, auto-deduplicated
- **Multi-line input**: Shift+Enter inserts newline, Enter submits
- **Keybindings**:
  - Ctrl+A (start), Ctrl+E (end), Ctrl+K (delete to end), Ctrl+W (delete word), Ctrl+U (clear)
  - Ctrl+C (quit), Ctrl+L (cycle log level)
- **CLI arguments**: `netget "listen on port 21 via ftp"` executes before TUI starts

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

## Protocol Planning Phase

**CRITICAL**: Before implementing a new protocol, complete this planning phase to ensure a smooth implementation.

### Planning Checklist

When planning to create a new protocol server, you MUST research and document the following:

#### 1. Server Library Research
- Research suitable Rust server crates for the protocol
- Evaluate: protocol compliance, maturity, API flexibility (LLM control?), documentation, dependencies
- Consider manual implementation if no suitable library exists
- Document chosen library with rationale (see protocol CLAUDE.md files for examples)

#### 2. Client Library Research (for E2E Testing)
- Research Rust client crates for E2E tests
- Prefer well-maintained libraries with good documentation
- Ensure library can test the specific protocol features you plan to implement
- Consider cross-language clients if no good Rust option exists
- Document chosen client with rationale

#### 3. LLM Control Points (Actions)
- List all points where the LLM will make decisions
- Distinguish between:
  - **Async Actions**: User-triggered, no network context
  - **Sync Actions**: Network event triggered, with context
- Define action names, parameters, and examples
- Consider startup parameters (ports, paths, behavior flags) and runtime reconfiguration actions
- See protocol CLAUDE.md files for detailed examples

#### 4. Logging Strategy
- Define what will be logged at each level (ERROR/WARN/INFO/DEBUG/TRACE)
- Follow the dual logging pattern (tracing macros + status_tx)
- See "Logging (CRITICAL)" section below and protocol CLAUDE.md files for examples

#### 5. Example Prompt
- Write a comprehensive prompt that would be used to start the server
- Include all key behaviors and scenarios (will be used as basis for E2E tests)
- Cover the main protocol features you plan to implement
- See protocol CLAUDE.md files for examples

### Planning Deliverables

Before starting implementation, you should have:
- [x] Chosen server library with clear rationale
- [x] Chosen client library for E2E testing
- [x] Documented all LLM control points (actions) with parameters and examples
- [x] Defined logging strategy for ERROR/WARN/INFO/DEBUG/TRACE levels
- [x] Written example prompts covering main protocol features

**Why This Matters**: This planning phase prevents mid-implementation surprises, ensures the library choices support LLM control, and provides clear documentation for future maintainers.

## Protocol Implementation Checklist

**CRITICAL REQUIREMENT**: ALL protocols MUST be feature gated. No protocol implementation should be compiled unconditionally. This keeps build times fast and allows users to compile only the protocols they need.

When creating new protocols in NetGet, ensure ALL of these steps are completed:

### 1. Protocol Stack Definition (`src/protocol/base_stack.rs`)
- Add new variant to `BaseStack` enum with correct stack name
- Update `name()` method with proper stack representation (e.g., "ETH>IP>TCP>SMTP")
- Update `from_str()` to parse protocol keywords
- Update `available_stacks()` to include new protocol
- Add unit tests for parsing the new protocol

### 2. TUI Description (`src/cli/rolling_tui.rs`)
- Add protocol description to welcome message in `print_welcome_messages()`
- Include example usage (e.g., "Start an SMTP mail server on port 25")
- Mark as "(Alpha)" if new/experimental, "(Beta)" if stable

### 3. Protocol Implementation (`src/server/<protocol>/mod.rs`)
- Create server implementation file (new protocols go in `src/server/<protocol>/`)
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

### 4. Protocol Actions (`src/server/<protocol>/actions.rs`)
- Implement `ProtocolActions` trait
- Define async actions (user-triggered, no network context)
- Define sync actions (network event triggered, with context)
- Create action definitions with parameters and examples
- Implement action execution logic
- **Server Configuration Parameters**:
  - When possible, allow configuration via `startup_params` in `open_server` action (for initial setup)
  - Also provide separate action(s) to reconfigure parameters during runtime (allows LLM to adjust behavior dynamically)
  - Example: DNS server could accept `default_ttl` in startup_params AND have a `set_default_ttl` action for runtime changes

### 5. Protocol Implementation Documentation (`src/server/<protocol>/CLAUDE.md`) - **MANDATORY**
- **CRITICAL**: Every protocol MUST have a CLAUDE.md file documenting implementation details
- **Location**: `src/server/<protocol>/CLAUDE.md`
- **Required Content**:
  - **Protocol Overview**: What protocol is implemented, version/RFC compliance
  - **Library Choices**: Which Rust crates are used and why (or why manual implementation)
  - **Architecture Decisions**: Key design choices made during implementation
  - **LLM Integration**: How the LLM controls the protocol (action-based, scripted, hybrid)
  - **Connection Management**: How connections are tracked and managed
  - **State Management**: Protocol-specific state structures and lifecycle
  - **Limitations**: Known limitations, unsupported features, edge cases
  - **Examples**: Example LLM prompts and responses for this protocol
  - **References**: Links to protocol specs, library docs, relevant RFCs
- **Example Structure**:
  ```markdown
  # SMTP Protocol Implementation

  ## Overview
  SMTP server implementing RFC 5321 (Simple Mail Transfer Protocol)

  ## Library Choices
  - `rust-smtp-server` (v0.1) - Chosen for parsing SMTP commands, handles protocol state machine
  - Manual response construction - Provides flexibility for LLM-controlled responses

  ## Architecture Decisions
  - **Connection State Machine**: Tracks HELO, MAIL FROM, RCPT TO, DATA states per connection
  - **LLM Control Points**: LLM decides whether to accept/reject recipients and message content
  - **No Authentication**: SMTP AUTH not implemented (can be added later)

  ## Limitations
  - No STARTTLS support (port 25 only, not 587)
  - No authentication (accepts all senders)
  - No message persistence (messages logged but not stored)
  ```

### 6. Module Registration (`src/server/mod.rs`)
- Add module declarations with feature flags
- Export server and protocol structs
- Example: `#[cfg(feature = "imap")] pub mod imap;`

### 7. Server Startup (`src/cli/server_startup.rs`)
- Add match arm for new `BaseStack` variant
- Implement feature-gated server spawning
- Update server status on success/failure
- Send status updates to UI

### 8. Connection Info (`src/state/server.rs`)
- Add variant to `ProtocolConnectionInfo` enum
- Include protocol-specific state (write_half, queued_data, etc.)

### 9. Feature Flag (`Cargo.toml`) - **MANDATORY**
- **CRITICAL**: Every protocol MUST have a feature flag - no exceptions
- Add feature flag for protocol (use lowercase name matching the protocol)
- Add protocol-specific dependencies with `optional = true`
- Include in `all-protocols` feature
- Example: `myprotocol = ["dep:myprotocol-lib", "async-trait"]`

### 10. E2E Test (`tests/server/<protocol>/e2e_test.rs`) - **CRITICAL: Follow Efficiency Guidelines**
- **Must create**: `tests/server/<protocol>/` directory, `mod.rs` with `pub mod e2e_test;`, add to `tests/server/mod.rs`
- **Must**: Start NetGet non-interactively, assert correct stack, use real client
- **CRITICAL - Feature Gate Tests**: Add `#[cfg(all(test, feature = "<protocol>"))]` to test module
  - Example: `#[cfg(all(test, feature = "ssh"))]`
  - Without feature gates, tests will fail to compile when protocol is disabled
  - See "CRITICAL: Feature Gating Tests" section for full examples
- **CRITICAL**: Follow efficiency guidelines (see section above):
  - Minimize server setups - reuse across test cases
  - Target < 10 total LLM calls for entire suite
  - Use scripting mode when protocol supports it
  - Consolidate test cases
- **Before running**: `./cargo-isolated.sh build --no-default-features --release --features <protocol>`
- **Run**: `./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test`

### 11. Test Documentation (`tests/server/<protocol>/CLAUDE.md`) - **MANDATORY**
- **Location**: `tests/server/<protocol>/CLAUDE.md`
- **Required Content**: Test overview, strategy, LLM call budget (target < 10), scripting usage, client library, expected runtime, failure rate, test cases, known issues
- See existing protocol test CLAUDE.md files for examples

### 12. Test Helpers (`tests/server/helpers.rs`)
- Update `extract_stack_from_prompt()` if needed
- Update `wait_for_server_startup()` to detect new protocol
- Handle protocol-specific stack validation

### Validation Checklist
- [ ] Protocol compiles with feature flag: `./cargo-isolated.sh build --no-default-features --features <protocol>`
- [ ] Protocol compiles in `all-protocols` mode: `./cargo-isolated.sh build --all-features`
- [ ] **Tests are feature-gated**: E2E tests use `#[cfg(all(test, feature = "<protocol>"))]`
- [ ] **Tests compile with protocol feature**: `./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test`
- [ ] **Implementation CLAUDE.md exists** at `src/server/<protocol>/CLAUDE.md` with complete documentation
- [ ] **Test CLAUDE.md exists** at `tests/server/<protocol>/CLAUDE.md` with LLM call budget and runtime
- [ ] E2E tests pass
- [ ] No compilation warnings
- [ ] Logging appears in Output panel
- [ ] Connections tracked in UI
- [ ] Stack name displays correctly
- [ ] Protocol responds to LLM actions correctly
- [ ] Total LLM calls < 10 for entire test suite

### Common Pitfalls
- **Forgetting to add feature flag** - EVERY protocol must be feature gated
- **Forgetting to feature gate tests** - EVERY test (E2E and unit) must use `#[cfg(all(test, feature = "<protocol>"))]`
- **Missing CLAUDE.md files** - MUST create both `src/server/<protocol>/CLAUDE.md` (implementation) and `tests/server/<protocol>/CLAUDE.md` (testing)
- **Inefficient E2E tests** - Spinning up multiple servers instead of reusing one comprehensive server, exceeding 10 LLM calls
- **Ignoring scripting mode** - Not using scripting when protocol supports it, wasting LLM calls on repetitive requests
- **Using `--all-features` for single protocol work** - Wastes 1-2 minutes compiling all protocols, use `--no-default-features --features <protocol>` instead
- Forgetting dual logging (tracing + status_tx)
- Not tracking connections in server state
- Missing feature flags in multiple files (server/mod.rs, cli/server_startup.rs, Cargo.toml)
- E2E tests using interactive mode instead of prompt mode
- Not validating server stack in tests
- Forgetting to update base_stack.rs parsing
- Not creating tests/server/<protocol>/ directory structure
- Not adding protocol to tests/server/mod.rs

## Multi-Instance Collaboration (CRITICAL)

**IMPORTANT**: When multiple Claude instances are working on the codebase simultaneously, follow these rules to avoid conflicts:

### Compilation Error Protocol
- **PAUSE and notify the user** if you encounter a compilation error in code you did not modify
- **DO NOT attempt to fix** compilation errors in other parts of the codebase - another Claude instance may be working on it
- **Exception**: Only fix errors in code sections you directly edited in the current session
- Let the user coordinate between instances if conflicts arise

### Shared File Editing (Cargo.toml, etc.)
- **NEVER overwrite entire shared files** like `Cargo.toml`, `base_stack.rs`, `server/mod.rs`
- **ALWAYS use Edit tool to patch changes** - insert/modify only your specific sections
- **Check for concurrent edits** - if you see multiple recent changes in a shared file, use extra caution
- **Examples of shared files requiring patching**:
  - `Cargo.toml` - Multiple instances adding different protocol features/dependencies
  - `src/protocol/base_stack.rs` - Multiple instances adding protocol variants
  - `src/server/mod.rs` - Multiple instances adding module declarations
  - `src/cli/server_startup.rs` - Multiple instances adding match arms
  - `src/state/server.rs` - Multiple instances adding connection info variants

### Collaboration Best Practices
- Use `Edit` tool for surgical changes to shared files
- Add your changes incrementally without removing others' work
- If you see unfamiliar recent changes in a file, assume another instance made them
- Focus on your assigned protocol/feature, avoid touching unrelated code

## Git Commit Instructions

- **ONLY commit when explicitly requested by the user** - Do not automatically commit changes
- **DO NOT** add "🤖 Generated with [Claude Code]" line
- **DO NOT** add "Co-Authored-By: Claude <noreply@anthropic.com>" signature
- Keep messages clean and professional without AI references
- Write concise, descriptive messages explaining what and why

## References

- Ollama API: http://localhost:11434
- Tokio: https://docs.rs/tokio
