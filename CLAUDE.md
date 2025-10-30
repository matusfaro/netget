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

## Testing

**Philosophy**: Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients.

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

### E2E Test Performance

**Critical**: E2E tests are slow because each test spawns a NetGet process and makes LLM API calls.

**Expected runtimes** (with qwen3-coder:30b, sequential execution):
- Fast protocols (IPP, MySQL): 30-60 seconds per suite
- Medium protocols (Telnet, HTTP, IRC): 60-120 seconds per suite
- Slow protocols (SMTP, mDNS, SMB): 120-300 seconds per suite
- Very slow protocols (TCP/FTP): >5 minutes per suite (complex multi-round-trip protocols)

**Test Execution**:
- **Run tests sequentially** (do NOT use `--test-threads` > 1)
- LLM processing becomes overloaded with concurrent tests
- Each test runs in isolation with dynamic port allocation
- Example: `cargo test --features e2e-tests --test e2e_<protocol>_test`

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

### Scripting Modes
- **LLM mode** (default): All decisions made by LLM
- **Python mode**: LLM generates Python scripts for protocol logic
- **JavaScript mode**: LLM generates JavaScript scripts for protocol logic

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

### 5. Module Registration (`src/server/mod.rs`)
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

### 8. Feature Flag (`Cargo.toml`)
- Add feature flag for protocol
- Add protocol-specific dependencies
- Include in `all-protocols` feature

### 9. E2E Test (`tests/server/<protocol>/e2e_test.rs`)
- **Must create protocol directory** `tests/server/<protocol>/`
- **Must create mod.rs** with `pub mod e2e_test;` (add feature flag `#[cfg(feature = "e2e-tests")]`)
- **Must add to `tests/server/mod.rs`** with `pub mod <protocol>;`
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
- **Run tests sequentially** (no parallel execution):
  ```bash
  cargo test --features e2e-tests --test server::<protocol>::e2e_test
  ```
- **Fix any issues before considering protocol complete**

### 10. Test Helpers (`tests/server/helpers.rs`)
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
- Not creating tests/server/<protocol>/ directory structure
- Not adding protocol to tests/server/mod.rs

## SSH/SFTP Implementation

**Library**: russh 0.45 + russh-sftp 2.1
**Location**: `src/network/ssh.rs`, `src/network/sftp_handler.rs`

**SSH Shell Buffering**: Line-based with character echo. Backspace erases on screen. Enter triggers LLM. Ctrl-C passed to LLM. First Enter shows banner, subsequent empty Enters skip LLM.

**SFTP**: LLM-controlled virtual filesystem. Operations: opendir, readdir, open, read, close, lstat, fstat, realpath. Handle tracking prevents re-reads.

**Connection Tracking**: SSH connections tracked with `ProtocolConnectionInfo::Ssh {authenticated, username, channels}`.

## VPN Protocol Implementations

### WireGuard - Full VPN Server ✅

**Status**: Production-ready, fully functional VPN server with actual tunnel support
**Library**: defguard_wireguard_rs 0.7 (multi-platform WireGuard library)
**Location**: `src/server/wireguard/mod.rs`, `src/server/wireguard/actions.rs`

**Key Features**:
- **Actual VPN Tunnels**: Clients can connect and route traffic through NetGet
- **TUN Interface**: Creates `netget_wg0` (Linux/Windows) or `utun10` (macOS)
- **Secure Keys**: Curve25519 keypair generation using `defguard_wireguard_rs::key::Key`
- **VPN Subnet**: Configures 10.20.30.0/24 network by default
- **Peer Monitoring**: Tracks connections every 5 seconds
- **Stats Tracking**: Bytes sent/received, last handshake time, endpoints
- **Cross-Platform**: Linux kernel, macOS userspace, Windows kernel, FreeBSD

**LLM Control Points**:
- `authorize_peer`: Allow peer to connect with specific allowed IPs (e.g., ["10.20.30.2/32"])
- `reject_peer`: Deny peer connection request
- `set_peer_traffic_limit`: Configure bandwidth/data limits
- `disconnect_peer`: Immediately disconnect a peer
- `list_peers`: View all connected peers
- `remove_peer`: Permanently remove peer from configuration
- `get_server_info`: View server public key and config

**Connection Tracking**: WireGuard peers tracked with `ProtocolConnectionInfo::Wireguard {public_key, endpoint, allowed_ips, last_handshake}`.

**Requirements**:
- Linux/FreeBSD: root or CAP_NET_ADMIN
- macOS: wireguard-go userspace
- Windows: administrator privileges

### OpenVPN - Honeypot Only ⚠️

**Status**: Detection-only honeypot (no actual VPN tunnels)
**Reason**: No viable Rust OpenVPN server library exists
**Location**: `src/server/openvpn/mod.rs`, `src/server/openvpn/actions.rs`

**Capabilities**:
- Detects OpenVPN handshake packets (V1 and V2)
- Recognizes opcodes: HARD_RESET, CONTROL, ACK
- Logs reconnaissance attempts with packet details
- LLM receives handshake detection events but cannot establish tunnels

**Why Not Full Implementation**: OpenVPN protocol is extremely complex (500K+ lines of C++), no mature Rust server library, Rust ecosystem focused on WireGuard.

**Recommendation**: Use WireGuard for production VPN. OpenVPN honeypot is sufficient for security research.

### IPSec/IKEv2 - Honeypot Only ⚠️

**Status**: Detection-only honeypot (no actual VPN tunnels)
**Reason**: No viable Rust IPSec server library exists (ipsec-parser is parse-only)
**Location**: `src/server/ipsec/mod.rs`, `src/server/ipsec/actions.rs`

**Capabilities**:
- Detects IKEv1 and IKEv2 handshakes
- Recognizes exchange types: IKE_SA_INIT, IKE_AUTH, CREATE_CHILD_SA, INFORMATIONAL
- Extracts SPIs (Security Parameter Indexes)
- Logs reconnaissance attempts with packet details
- LLM receives handshake detection events but cannot establish tunnels

**Why Not Full Implementation**: IPSec/IKEv2 is extremely complex (hundreds of thousands of lines in strongSwan/libreswan), requires deep OS integration (XFRM policy), no mature Rust server library.

**Recommendation**: Use WireGuard for production VPN. IPSec honeypot is sufficient for IKE protocol analysis.

**VPN Implementation Details**: See `VPN_IMPLEMENTATION_STATUS.md` for comprehensive comparison and future roadmap.

## SMB/CIFS Implementation

**Protocol Version**: SMB2 (dialect 0x0210)
**Library**: smb-msg 0.10 (for message structures, not used for parsing in current implementation)
**Location**: `src/server/smb/mod.rs`, `src/server/smb/actions.rs`

**Authentication**: LLM-controlled. Supports both guest and user authentication:
- **Guest mode**: If no username detected or username is "guest", uses guest authentication
- **User mode**: Extracts username from SESSION_SETUP request, consults LLM for approval
- LLM responds with `smb_auth_success` to allow or `smb_auth_deny` to reject
- Failed authentication returns SMB2 STATUS_ACCESS_DENIED (0xC0000016)

**Protocol Flow**:
1. **NEGOTIATE** - Server offers SMB 2.1 dialect, responds with server GUID and capabilities
2. **SESSION_SETUP** - Consults LLM with username, creates session if LLM approves
3. **TREE_CONNECT** - Grants access to virtual share (accepts all share names)
4. **File Operations** - All operations consult LLM for virtual filesystem

**SMB2 Operations** (all LLM-integrated):
- **CREATE** - Opens/creates files, generates 16-byte file handles (GUIDs), stores path mappings
- **CLOSE** - Releases file handles, cleans up state
- **READ** - Reads file content from LLM, supports offset/length parameters
- **WRITE** - Sends file content to LLM for storage (text files)
- **QUERY_INFO** - Returns file metadata (size, attributes, timestamps) from LLM
- **QUERY_DIRECTORY** - Lists directory contents from LLM, returns UTF-16LE encoded names

**File Handle Management**: Per-connection HashMap tracks `file_id → {path, is_directory}`. Handles generated using timestamp-based GUIDs. Proper cleanup on CLOSE.

**LLM Actions** (defined in `actions.rs`):
- Sync: `smb_auth_success`, `smb_auth_deny`, `smb_list_directory`, `smb_read_file`, `smb_write_file`, `smb_get_file_info`, `smb_create_file`, `smb_delete_file`, `smb_create_directory`, `smb_delete_directory`
- Async: `disconnect_client`

**UTF-16LE Handling**: All file paths parsed from UTF-16LE (Windows encoding). Directory listings returned in UTF-16LE format for client compatibility.

**Response Construction**: Manual SMB2 response building (not using smb-msg parsing). Each response includes:
- 64-byte SMB2 header (signature `\xFESMB`, command code, message ID, tree/session IDs)
- Variable-length body (operation-specific structures)
- Proper Windows FILETIME timestamps and file attributes

**Default Port**: 8445 (non-privileged alternative to standard 445)

**Connection Tracking**: SMB connections tracked with `ProtocolConnectionInfo::Smb {authenticated, username, session_id, open_files}`.

**Limitations**:
- Simplified username extraction (no full NTLM/Kerberos parsing - uses heuristic ASCII extraction)
- Text file support for WRITE operations (binary treated as UTF-8 lossy)
- Simplified timestamps (zeros for creation/modification times)
- Single share model (all tree connects succeed)

**Compatible Clients**: Windows File Explorer, Linux smbclient, macOS Finder, any SMB2+ compatible client.

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
