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
cargo test --lib                                              # Unit tests
cargo test --features e2e-tests --test server::tcp::e2e_test  # TCP E2E tests
cargo test --features e2e-tests --test server::http::e2e_test # HTTP E2E tests
cargo test --features e2e-tests --test server::ssh::e2e_test  # SSH E2E tests
cargo test --features e2e-tests,proxy --test server::proxy::e2e_test  # Proxy E2E tests
```

**IMPORTANT - Protocol-Specific Testing**:
- **MUST run tests for your protocol using feature gates** - Never run the entire test suite
- E2E tests are extremely slow (each spawns NetGet process + makes LLM API calls)
- Running all tests can take 30+ minutes
- **Always use protocol-specific test command**:
  ```bash
  cargo test --features e2e-tests,<protocol> --test server::<protocol>::e2e_test
  ```
- Example for SSH: `cargo test --features e2e-tests,ssh --test server::ssh::e2e_test`
- Example for HTTP Proxy: `cargo test --features e2e-tests,proxy --test server::proxy::e2e_test`

### E2E Test Performance

**Critical**: E2E tests are slow because each test spawns a NetGet process and makes LLM API calls.

**Test Execution**:
- **Tests can run concurrently** (Ollama lock enabled by default in tests)
- The `--ollama-lock` flag serializes LLM API access, preventing overload
- Each test runs in isolation with dynamic port allocation
- Example: `cargo test --features e2e-tests --test server::http::e2e_test`
- See "Multi-Instance Concurrency Support" section for details

**Critical setup requirement**:
- **MUST build release binary with all features before running tests**:
  ```bash
  cargo build --release --all-features
  ```
- E2E tests spawn the release binary from `target/release/netget`
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

**Bad Example** (3 server setups, 3+ LLM calls):
```rust
#[tokio::test]
async fn test_dns_a_record() {
    let prompt = "Listen on port 0 via DNS. For A record queries, return 93.184.216.34";
    let server = start_server_with_prompt(prompt).await;
    // Test A record...
}

#[tokio::test]
async fn test_dns_mx_record() {
    let prompt = "Listen on port 0 via DNS. For MX record queries, return mail.example.com";
    let server = start_server_with_prompt(prompt).await;
    // Test MX record...
}
```

**Good Example** (1 server setup, 3 LLM calls total):
```rust
#[tokio::test]
async fn test_dns_records() {
    let prompt = r#"Listen on port 0 via DNS.
    - For A record queries on example.com, return 93.184.216.34
    - For MX record queries on example.com, return mail.example.com with priority 10
    - For AAAA record queries on example.com, return 2606:2800:220:1:248:1893:25c8:1946"#;

    let server = start_server_with_prompt(prompt).await;

    // Test A record
    // Test MX record
    // Test AAAA record
    // All against same server instance
}
```

**2. Target < 10 Total LLM Calls Per Protocol Test Suite**
- Count LLM calls carefully: 1 server startup + N requests (unless scripted)
- Prefer scripting mode when protocol supports it (0 LLM calls per request)
- Consolidate test cases to share server instances
- Example budget for a protocol:
  - 2-3 comprehensive server setups
  - 2-3 requests per server (if not scripted)
  - Total: 6-9 LLM calls

**3. Use Scripting Mode When Available**
- For protocols with repetitive request/response patterns, use scripting
- Server startup generates script (1 LLM call), all subsequent requests use script (0 LLM calls)
- Example: DNS, HTTP, DHCP are excellent candidates for scripting
- Allows testing many scenarios with only 1 LLM call per server

**4. Group Related Test Scenarios**
- Bundle all CRUD operations into one test with one server
- Bundle all error cases into one test with comprehensive error instructions
- Bundle all edge cases into one test

**Example Test Structure**:
```rust
#[tokio::test]
async fn test_protocol_basic_operations() {
    // 1 server setup with comprehensive instructions
    // Tests: create, read, update, delete
    // LLM calls: 1 startup + 4 requests = 5 total
}

#[tokio::test]
async fn test_protocol_error_handling() {
    // 1 server setup with error handling instructions
    // Tests: invalid input, missing fields, timeouts
    // LLM calls: 1 startup + 3 requests = 4 total
}

// Total for protocol: 9 LLM calls (within budget)
```

**Anti-Pattern to Avoid**:
- Spinning up 10+ servers for 10+ test cases (10+ startup calls + N request calls = 20+ total LLM calls)
- Testing each protocol feature in isolation (wastes server setup overhead)
- Ignoring scripting mode when protocol supports it

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
cargo test --features e2e-tests --test server::http::e2e_test
```

### E2E Test Concurrency

**Default behavior**: E2E tests enable `--ollama-lock` by default in `ServerConfig::new()`.

**Running tests in parallel**:
```bash
# Run all protocol tests concurrently (uses available CPU cores)
cargo test --features e2e-tests

# Run specific protocol tests concurrently
cargo test --features e2e-tests,http,tcp,ssh

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

**IMPORTANT**: When running multiple Claude instances against the same repository, each instance MUST use a separate build directory to avoid conflicts.

#### Session Setup

At the **start of each session** working on netget, run this once:

```bash
export CARGO_TARGET_DIR="$(pwd)/target-claude/claude-$$"
```

This sets a session-specific build directory that:
- Uses the shell PID (`$$`) - unique per Claude session, not per command
- Persists for all commands in this session
- Prevents Cargo lock contention between concurrent Claude instances

**You only need to run this once per session.** All subsequent `cargo` commands will automatically use this directory.

#### Alternative: Automatic Setup

If you want this to happen automatically, add to `~/.bashrc` or `~/.zshrc`:

```bash
# Auto-detect netget project and set isolated build directory
if [[ "$PWD" == *"/netget"* ]] && [ -f "Cargo.toml" ]; then
  export CARGO_TARGET_DIR="$(pwd)/target-claude/claude-$$"
fi
```

Or use a project-specific `.envrc` file (if you have direnv installed):

```bash
# .envrc in netget root
export CARGO_TARGET_DIR="$(pwd)/target-claude/claude-$$"
```

Then run `direnv allow` once.

#### What This Does

1. **Creates isolated builds**: Each Claude session gets `target-claude/claude-{shell_pid}/`
2. **Prevents lock contention**: No more "Blocking waiting for file lock" errors
3. **Stays in repo**: All build dirs under `target-claude/` (already gitignored)
4. **No per-command overhead**: Set once, applies to entire session
5. **No approval prompts**: Avoids prefixing every cargo command with export

#### Maintenance - Cleaning Old Build Directories

Build directories accumulate over time. Clean them manually when needed:

**Option 1: Unified `cargo clean` (Recommended)**

Create a shell alias that cleans both `target/` and `target-claude/`:

```bash
# Add to ~/.bashrc or ~/.zshrc
alias cargo-clean-all='cargo clean && rm -rf target-claude/'
```

Usage: `cargo-clean-all` (cleans standard target and all Claude instance builds)

**Option 2: Periodic Manual Cleanup**

```bash
# Remove build directories older than 10 days
find target-claude/ -maxdepth 1 -type d -mtime +10 -exec rm -rf {} \;

# Or keep only the 3 most recent builds
ls -t target-claude/ | tail -n +4 | xargs -I {} rm -rf target-claude/{}

# Or nuke everything (safe - will rebuild on next build)
rm -rf target-claude/
```

**Option 3: Weekly Cron Job**

```bash
# Add to crontab for weekly cleanup (every Sunday at 2am)
0 2 * * 0 cd ~/dev/netget && find target-claude/ -maxdepth 1 -type d -mtime +7 -exec rm -rf {} \; 2>/dev/null
```

**DO NOT clean on every `cd`** - This is slow and disruptive. Clean manually when disk space becomes an issue.

#### Disk Space Considerations

- Each build directory: ~2-5 GB (full build with all features)
- After 10 concurrent sessions: ~20-50 GB
- Cleanup recommendation: Use `cargo-clean-all` alias or clean manually when disk space is low
- Monitor disk usage: `du -sh target-claude/` to see total size
- Safe to delete: Old build directories don't affect current sessions (they rebuild on next compile)

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

### 6. Server Startup (`src/cli/server_startup.rs`)
- Add match arm for new `BaseStack` variant
- Implement feature-gated server spawning
- Update server status on success/failure
- Send status updates to UI

### 7. Connection Info (`src/state/server.rs`)
- Add variant to `ProtocolConnectionInfo` enum
- Include protocol-specific state (write_half, queued_data, etc.)

### 8. Feature Flag (`Cargo.toml`) - **MANDATORY**
- **CRITICAL**: Every protocol MUST have a feature flag - no exceptions
- Add feature flag for protocol (use lowercase name matching the protocol)
- Add protocol-specific dependencies with `optional = true`
- Include in `all-protocols` feature
- Example: `myprotocol = ["dep:myprotocol-lib", "async-trait"]`

### 9. E2E Test (`tests/server/<protocol>/e2e_test.rs`) - **CRITICAL: Follow Efficiency Guidelines**
- **Must create protocol directory** `tests/server/<protocol>/`
- **Must create mod.rs** with `pub mod e2e_test;` (add feature flag `#[cfg(feature = "e2e-tests")]`)
- **Must add to `tests/server/mod.rs`** with `pub mod <protocol>;`
- **Must start NetGet in non-interactive mode** with a prompt
- **Must assert server started with correct stack** using helpers
- **Must use real client** or emulated client (only if no library available)
- **CRITICAL: Follow E2E Test Efficiency Guidelines** (see section above):
  - **Minimize server setups** - Reuse servers across multiple test cases
  - **Target < 10 total LLM calls** for entire protocol test suite
  - **Use scripting mode** when protocol supports it
  - **Consolidate test cases** - Don't create separate servers for each scenario
  - Example: One DNS server handles A, MX, AAAA records; don't create 3 servers
- Test multiple scenarios:
  - Basic functionality (connect, send, receive)
  - Protocol-specific commands
  - Error handling
  - Concurrent connections (if applicable)
- **Before running tests, MUST build release binary**:
  ```bash
  cargo build --release --all-features
  ```
- **Run tests** (concurrent execution supported with Ollama lock):
  ```bash
  cargo test --features e2e-tests --test server::<protocol>::e2e_test
  ```
- **Fix any issues before considering protocol complete**

### 10. Test Documentation (`tests/server/<protocol>/CLAUDE.md`) - **MANDATORY**
- **CRITICAL**: Every protocol MUST have a test CLAUDE.md file documenting test details
- **Location**: `tests/server/<protocol>/CLAUDE.md`
- **Required Content**:
  - **Test Overview**: What aspects of the protocol are tested
  - **Test Strategy**: How tests are structured (consolidated vs isolated)
  - **LLM Call Budget**: Total LLM calls for entire test suite (target: < 10)
    - Count server startups + network requests (unless scripted)
    - Show breakdown per test function
  - **Scripting Usage**: Whether tests use scripting mode or action-based
  - **Client Library**: Actual client library used (e.g., hickory-client for DNS) or manual/emulated client
  - **Expected Runtime**: Typical runtime for full test suite (with specified model)
  - **Failure Rate**: Historical failure/flakiness rate (if any)
  - **Test Cases**: List of test scenarios covered
  - **Known Issues**: Flaky tests, timing issues, or environment dependencies
- **Example Structure**:
  ```markdown
  # DNS Protocol E2E Tests

  ## Test Overview
  Tests DNS server with A, MX, AAAA, TXT, NXDOMAIN record types

  ## Test Strategy
  - Single consolidated test with one server handling all record types
  - Reuses server instance across multiple queries
  - Uses scripting mode for fast, deterministic responses

  ## LLM Call Budget
  - `test_dns_records_comprehensive()`: 1 startup call (scripted mode)
  - `test_dns_error_handling()`: 1 startup call (scripted mode)
  - **Total: 2 LLM calls** (well under 10 limit)

  ## Scripting Usage
  ✅ **Scripting Enabled** - All responses generated by script, no LLM calls per request

  ## Client Library
  - `hickory-client` v0.24 with UDP transport
  - Uses real DNS client for protocol correctness

  ## Expected Runtime
  - Model: qwen3-coder:30b
  - Runtime: ~25 seconds for full test suite
  - Fast due to scripting (no LLM calls per query)

  ## Failure Rate
  - **Low** (<1%) - Occasional timeout if Ollama is slow on startup
  - No flakiness - scripted responses are deterministic

  ## Test Cases
  1. A record resolution (IPv4)
  2. MX record with priority
  3. AAAA record (IPv6)
  4. TXT record
  5. NXDOMAIN for non-existent domains

  ## Known Issues
  - None - tests are stable and deterministic
  ```

### 11. Test Helpers (`tests/server/helpers.rs`)
- Update `extract_stack_from_prompt()` if needed
- Update `wait_for_server_startup()` to detect new protocol
- Handle protocol-specific stack validation

### Validation Checklist
- [ ] Protocol compiles with feature flag
- [ ] Protocol compiles in `all-protocols` mode
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
- **Missing CLAUDE.md files** - MUST create both `src/server/<protocol>/CLAUDE.md` (implementation) and `tests/server/<protocol>/CLAUDE.md` (testing)
- **Inefficient E2E tests** - Spinning up multiple servers instead of reusing one comprehensive server, exceeding 10 LLM calls
- **Ignoring scripting mode** - Not using scripting when protocol supports it, wasting LLM calls on repetitive requests
- Forgetting dual logging (tracing + status_tx)
- Not tracking connections in server state
- Missing feature flags in multiple files (server/mod.rs, cli/server_startup.rs, Cargo.toml)
- E2E tests using interactive mode instead of prompt mode
- Not validating server stack in tests
- Forgetting to update base_stack.rs parsing
- Not creating tests/server/<protocol>/ directory structure
- Not adding protocol to tests/server/mod.rs

## Git Commit Instructions

- **ONLY commit when explicitly requested by the user** - Do not automatically commit changes
- **DO NOT** add "🤖 Generated with [Claude Code]" line
- **DO NOT** add "Co-Authored-By: Claude <noreply@anthropic.com>" signature
- Keep messages clean and professional without AI references
- Write concise, descriptive messages explaining what and why

## References

- Ollama API: http://localhost:11434
- Tokio: https://docs.rs/tokio
