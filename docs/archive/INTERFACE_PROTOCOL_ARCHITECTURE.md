# Flexible Protocol Binding Architecture (Backwards Compatible)

## Problem Statement

Network protocols operate at different layers and require different binding parameters:

- **Layer 2 (DataLink)**: MAC address, interface
- **Layer 3 (ICMP, ARP)**: Interface, IP address (optional)
- **Layer 4 (TCP, UDP)**: IP address, port
- **Layer 7 (HTTP, DNS)**: IP address, port

The current action handler only supports **port-based binding** (IP:port), making it impossible to start interface-based protocols like ICMP, ARP, and DataLink.

### Current Blocker

When the ICMP E2E test tries to start a server:
```json
{
  "type": "open_server",
  "interface": "lo",
  "base_stack": "ICMP",
  "instruction": "..."
}
```

**Error**: "Unknown action type, skipping: {...}"

**Root Cause**: `OpenServer` requires `port: u16` field (not optional), and `SpawnContext` only has `listen_addr: SocketAddr`.

## Design Philosophy: Flexible Binding with Backwards Compatibility

Support **all binding parameters as optional** while maintaining full backwards compatibility:

- **MAC address**: For layer 2 protocols (e.g., spoofing source MAC in ARP)
- **Interface**: For raw socket protocols (ICMP, ARP, DataLink)
- **Host (hostname or IP)**: For specific IP binding (IPv4, IPv6, or hostname like "localhost", "example.com")
- **Port**: For transport layer protocols (TCP, UDP, etc.)

**Key Principles**:
1. **Zero breaking changes**: Old code continues to work
2. **Gradual migration**: Migrate protocols one at a time
3. **Protocol defaults**: Each protocol defines sensible defaults
4. **Flexible addresses**: Support hostnames, not just IP addresses

### Examples

**TCP server on specific interface**:
```json
{"interface": "eth0", "host": "192.168.1.100", "port": 8080, "base_stack": "TCP"}
```

**HTTP server with defaults**:
```json
{"port": 8080, "base_stack": "HTTP"}
// Defaults: host = "127.0.0.1"
```

**HTTP server with hostname**:
```json
{"host": "localhost", "port": 8080, "base_stack": "HTTP"}
```

**ICMP server on loopback**:
```json
{"interface": "lo", "base_stack": "ICMP"}
// Or with default: {"base_stack": "ICMP"}
```

**ARP server with custom MAC**:
```json
{"interface": "eth0", "mac_address": "00:11:22:33:44:55", "base_stack": "ARP"}
```

## Protocol Requirements Matrix

| Protocol | MAC | Interface | IP | Port | Notes |
|----------|-----|-----------|----|----|-------|
| **TCP** | - | Optional | Optional | **Required** | Default host: "127.0.0.1", Default port: 0 |
| **HTTP** | - | Optional | Optional | **Required** | Default host: "127.0.0.1", Default port: 0 |
| **UDP** | - | Optional | Optional | **Required** | Default host: "127.0.0.1", Default port: 0 |
| **DNS** | - | Optional | Optional | **Required** | Default host: "127.0.0.1", Default port: 0 |
| **ICMP** | - | **Required** | Optional | - | Default interface: "lo" |
| **ARP** | Optional | **Required** | Optional | - | Default interface: "lo" |
| **DataLink** | Optional | **Required** | - | - | Default interface: "lo" |
| **WireGuard** | - | **Required** | Optional | Optional | Needs interface for tunnel |
| **DHCP** | - | Optional | Optional | **Required** | UDP-based but may need interface |

## Backwards Compatibility Strategy

### During Migration Period

**Old fields remain** and work exactly as before:
- `OpenServer.port: u16` (required for old protocols)
- `SpawnContext.listen_addr: SocketAddr`

**New fields added** alongside:
- `OpenServer.{mac_address, interface, host, port}` (all optional)
- `SpawnContext.{mac_address, interface, host, port}` (all optional)

**Migration detection**:
```rust
// If protocol implements default_binding(), use new system
// Otherwise, use old system (listen_addr)
if protocol.has_custom_default_binding() {
    // New flexible binding path
} else {
    // Old backwards-compatible path
}
```

### Migration Path

1. **Phase 1**: Add new fields (no changes to existing protocols)
2. **Phase 2-N**: Migrate protocols one by one
3. **Phase N+1**: Remove deprecated fields (breaking change, requires major version bump)

## LLM Migration Flow (CRITICAL)

This section explains what the LLM sees and uses at each phase of migration. This is crucial for ensuring the LLM generates correct actions.

### Before Migration (Current State)

**Action Schema** (what LLM sees in prompt):
```json
{
  "type": "open_server",
  "port": 8080,              // REQUIRED (u16)
  "base_stack": "TCP",
  "instruction": "..."
}
```

**Protocol Prompt** (TCP example):
```
To start a TCP server, use the open_server action with:
- port: Port number to listen on (required)
- base_stack: "TCP"
- instruction: What the server should do

Example: {"type": "open_server", "port": 8080, "base_stack": "TCP", "instruction": "Echo server"}
```

**LLM Response**:
```json
[{
  "type": "open_server",
  "port": 8080,
  "base_stack": "TCP",
  "instruction": "Echo server that repeats back everything"
}]
```

**Backend Behavior**:
- Extracts `port: 8080`
- Builds `listen_addr: 127.0.0.1:8080`
- Passes to protocol's `spawn()`
- Protocol uses `ctx.listen_addr`

### Phase 1: Infrastructure Added (Action Schema Changes)

**Action Schema** (what LLM sees in prompt):
```json
{
  "type": "open_server",
  "mac_address": "...",      // OPTIONAL (for layer 2 protocols)
  "interface": "...",        // OPTIONAL (for raw socket protocols)
  "host": "...",             // OPTIONAL (IPv4, IPv6, or hostname)
  "port": 8080,              // OPTIONAL (for socket protocols)
  "base_stack": "TCP",
  "instruction": "..."
}
```

**Unmigrated Protocol Prompt** (TCP before migration):
```
To start a TCP server, use the open_server action with:
- port: Port number to listen on (defaults to 0 for dynamic assignment)
- host: Host address to bind (defaults to "127.0.0.1")
- base_stack: "TCP"
- instruction: What the server should do

Example: {"type": "open_server", "port": 8080, "base_stack": "TCP", "instruction": "Echo server"}
```

**LLM Response** (unmigrated TCP):
```json
[{
  "type": "open_server",
  "port": 8080,
  "base_stack": "TCP",
  "instruction": "Echo server that repeats back everything"
}]
```

**Backend Behavior** (unmigrated protocol):
1. Check `protocol.default_binding()` → returns `None` (not migrated)
2. **OLD PATH**: Use backwards-compatible behavior
3. Extract `port: Some(8080)` (or default to 0 if None)
4. Force `host: "127.0.0.1"` (ignore user's host value)
5. Build `listen_addr: 127.0.0.1:8080`
6. Pass to protocol's `spawn()` with deprecated `ctx.listen_addr` set
7. Protocol uses `ctx.listen_addr` (old way)

**Migrated Protocol Prompt** (ICMP after migration):
```
To start an ICMP server, use the open_server action with:
- interface: Network interface to bind (defaults to "lo" for loopback)
- base_stack: "ICMP"
- instruction: What the server should do

Example: {"type": "open_server", "interface": "eth0", "base_stack": "ICMP", "instruction": "Respond to ping"}
```

**LLM Response** (migrated ICMP):
```json
[{
  "type": "open_server",
  "interface": "eth0",
  "base_stack": "ICMP",
  "instruction": "Respond to ICMP echo requests with echo replies"
}]
```

**Backend Behavior** (migrated protocol):
1. Check `protocol.default_binding()` → returns `Some(BindingDefaults { interface: Some("lo"), ... })`
2. **NEW PATH**: Use flexible binding system
3. Apply defaults: `interface: Some("eth0")` (user provided), `host: None`, `port: None`, `mac_address: None`
4. Build `SpawnContext` with new optional fields set
5. Pass to protocol's `spawn()` with `ctx.interface = Some("eth0")`
6. Protocol uses `ctx.interface()` → Ok("eth0") (new way)

### Phase 2-N: Protocol Migration

As each protocol migrates, its prompt changes to guide the LLM appropriately.

#### Example: TCP Migration

**Before Migration**:
```
Protocol prompt tells LLM: "Use port field (required)"
LLM generates: {"port": 8080}
Backend: Uses old path (listen_addr)
Protocol code: Uses ctx.listen_addr
```

**After Migration**:
```
Protocol prompt tells LLM: "Use port field (defaults to 0), optionally host field (defaults to 127.0.0.1)"
LLM generates: {"port": 8080} or {"host": "0.0.0.0", "port": 8080}
Backend: Uses new path (applies defaults)
Protocol code: Uses ctx.socket_addr()
```

#### Example: ICMP (New Protocol)

**After Migration** (uses new system from start):
```
Protocol prompt tells LLM: "Use interface field (defaults to 'lo')"
LLM generates: {"interface": "eth0"} or {} (uses default)
Backend: Uses new path (applies defaults)
Protocol code: Uses ctx.interface()
```

### Phase N+1: After Full Migration

**Action Schema** (same as Phase 1):
```json
{
  "type": "open_server",
  "mac_address": "...",      // OPTIONAL
  "interface": "...",        // OPTIONAL
  "host": "...",             // OPTIONAL
  "port": 8080,              // OPTIONAL
  "base_stack": "TCP",
  "instruction": "..."
}
```

**All Protocol Prompts**: Use new optional fields

**Backend Behavior**:
- All protocols return `Some(...)` from `default_binding()`
- Old path removed entirely
- `SpawnContext.listen_addr` removed (deprecated field gone)

### Key Insights for LLM Instructions

1. **Protocol-specific prompts**: Each protocol's prompt tells the LLM which fields to use
2. **Action schema is global**: Same schema shown to all protocols (after Phase 1)
3. **Defaults are automatic**: LLM doesn't need to specify fields that have good defaults
4. **Migration is transparent**: LLM behavior changes only when protocol prompt changes

### Example LLM Interactions

#### TCP Server (Unmigrated)

**LLM Prompt**:
```
Available actions:
  - open_server: Start a new server
    Fields:
      - mac_address (optional): MAC address for layer 2 protocols
      - interface (optional): Network interface for raw protocols
      - host (optional): Host address to bind (IPv4, IPv6, or hostname)
      - port (optional): Port number to bind
      - base_stack (required): Protocol name
      - instruction (required): Server instruction

For TCP protocol:
  Use open_server with port field. Port defaults to 0 (dynamic assignment), host defaults to "127.0.0.1".
```

**LLM Generates**:
```json
[{"type": "open_server", "port": 8080, "base_stack": "TCP", "instruction": "Echo server"}]
```

**Backend Processes**: Old path → uses port, ignores new fields

#### ICMP Server (Migrated)

**LLM Prompt**:
```
Available actions:
  - open_server: Start a new server
    Fields:
      - mac_address (optional): MAC address for layer 2 protocols
      - interface (optional): Network interface for raw protocols
      - host (optional): Host address to bind (IPv4, IPv6, or hostname)
      - port (optional): Port number to bind
      - base_stack (required): Protocol name
      - instruction (required): Server instruction

For ICMP protocol:
  Use open_server with interface field. Interface defaults to "lo" (loopback).
  Do not specify host or port fields for ICMP.
```

**LLM Generates**:
```json
[{"type": "open_server", "interface": "eth0", "base_stack": "ICMP", "instruction": "Respond to pings"}]
```

**Backend Processes**: New path → applies defaults, uses interface

#### HTTP Server (After Migration)

**LLM Prompt**:
```
For HTTP protocol:
  Use open_server with port field (required).
  Optionally specify:
    - host: Bind address (defaults to "127.0.0.1")
    - interface: Specific network interface
```

**LLM Generates** (simple):
```json
[{"type": "open_server", "port": 80, "base_stack": "HTTP", "instruction": "Web server"}]
```

**LLM Generates** (advanced):
```json
[{"type": "open_server", "host": "0.0.0.0", "port": 80, "base_stack": "HTTP", "instruction": "Public web server"}]
```

**LLM Generates** (combined):
```json
[{"type": "open_server", "interface": "eth0", "host": "192.168.1.100", "port": 80, "base_stack": "HTTP", "instruction": "Web server on eth0"}]
```

**Backend Processes**: New path → applies defaults, builds socket address

### Protocol Prompt Update Strategy

When migrating a protocol, update its prompt context:

**Before** (TCP unmigrated):
```rust
// In TCP protocol's get_server_context() or similar
"Use open_server with port=<number>"
```

**After** (TCP migrated):
```rust
// In TCP protocol's get_server_context() or similar
"Use open_server with port=<number> (defaults to 0).
Optionally specify host=<address> (defaults to 127.0.0.1) or interface=<name> for specific binding."
```

### Summary

- **Action schema changes once** in Phase 1 (all fields become optional)
- **Protocol prompts guide LLM** on which fields to use for that specific protocol
- **Backend detects migration status** (`default_binding()` returns `Some` or `None`)
- **Unmigrated protocols** continue working with old path (forced defaults)
- **Migrated protocols** use new path (applies protocol defaults)
- **LLM sees consistent schema** but gets protocol-specific usage guidance

## Proposed Solution

### 1. Action Schema Changes (Backwards Compatible)

**File**: `src/llm/actions/common.rs`

Add new optional fields, keep old required field:

```rust
OpenServer {
    /// OLD FIELD: Port to bind (DEPRECATED, use `port` optional field instead)
    /// This will be removed in v2.0
    /// For backwards compatibility, if new `port` field is None, this is used
    #[deprecated(since = "1.x.0", note = "Use optional `port` field instead")]
    port: u16,

    // NEW FIELDS (all optional)

    /// MAC address (for layer 2 protocols like ARP spoofing)
    /// Format: "00:11:22:33:44:55"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mac_address: Option<String>,

    /// Network interface to bind (for raw protocols like ICMP, ARP, DataLink)
    /// Examples: "lo", "eth0", "wlan0", "any"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interface: Option<String>,

    /// Host address (hostname or IP) to bind
    /// Examples: "127.0.0.1", "0.0.0.0", "::1", "::", "localhost", "example.com"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<String>,

    /// Port to bind (for socket-based protocols like TCP, HTTP, DNS)
    /// Use 0 for dynamic port assignment
    /// Note: This is separate from the deprecated `port` field above
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "port_new")]
    port_new: Option<u16>,

    base_stack: String,

    // ... rest unchanged
    #[serde(default)]
    send_first: bool,
    #[serde(default)]
    initial_memory: Option<String>,
    instruction: String,
    #[serde(default)]
    startup_params: Option<serde_json::Value>,
    #[serde(default)]
    event_handlers: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    #[serde(default)]
    feedback_instructions: Option<String>,
}
```

**WAIT - Better approach**: Make old `port` field optional too, avoid two port fields:

```rust
OpenServer {
    /// MAC address (for layer 2 protocols like ARP spoofing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mac_address: Option<String>,

    /// Network interface to bind (for raw protocols like ICMP, ARP, DataLink)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interface: Option<String>,

    /// Host address (hostname or IP) to bind (IPv4, IPv6, or hostname)
    /// Examples: "127.0.0.1", "0.0.0.0", "::1", "localhost", "example.com"
    /// Default: Protocol-specific (usually "127.0.0.1" for port-based protocols)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<String>,

    /// Port to bind (for socket-based protocols like TCP, HTTP, DNS)
    /// Use 0 for dynamic port assignment
    /// Default: Protocol-specific (usually 0 for dynamic assignment)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<u16>,  // ← NOW OPTIONAL (breaking change, but handled with defaults)

    base_stack: String,
    // ... rest unchanged
}
```

**Backwards Compatibility**: Old code that passes `port: 8080` still works (Some(8080)).

### 2. SpawnContext Changes (Backwards Compatible)

**File**: `src/protocol/spawn_context.rs`

Add new fields, keep old field:

```rust
pub struct SpawnContext {
    /// OLD FIELD: Address to listen on (DEPRECATED)
    /// This will be removed in v2.0
    /// For backwards compatibility with protocols that haven't migrated
    #[deprecated(since = "1.x.0", note = "Use ip/port fields instead")]
    pub listen_addr: SocketAddr,

    // NEW FIELDS (all optional)

    /// MAC address (optional, for layer 2 protocols)
    pub mac_address: Option<String>,

    /// Network interface (optional, for raw socket protocols)
    pub interface: Option<String>,

    /// Host address (hostname or IP) to bind (optional, for socket protocols)
    /// Can be IPv4 ("127.0.0.1"), IPv6 ("::1"), or hostname ("localhost")
    pub host: Option<String>,

    /// Port to bind (optional, for transport protocols)
    pub port: Option<u16>,

    /// LLM client for generating responses
    pub llm_client: OllamaClient,

    /// Application state
    pub state: Arc<AppState>,

    /// Channel for sending status updates to UI
    pub status_tx: mpsc::UnboundedSender<String>,

    /// Server ID for tracking
    pub server_id: ServerId,

    /// Optional startup parameters (validated against protocol schema)
    pub startup_params: Option<StartupParams>,
}
```

**Helper Methods** (backwards compatible):

```rust
impl SpawnContext {
    /// Build SocketAddr from IP and port (for socket-based protocols)
    /// Falls back to deprecated listen_addr if new fields not set
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        // Try new fields first
        if let (Some(ip_str), Some(port)) = (&self.ip, self.port) {
            // Parse IP or resolve hostname
            let ip = if let Ok(addr) = ip_str.parse::<IpAddr>() {
                addr
            } else {
                // Try to resolve hostname (simple cases)
                match ip_str.as_str() {
                    "localhost" => IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    _ => {
                        // For now, return error. Future: DNS resolution
                        return Err(anyhow::anyhow!(
                            "Hostname resolution not yet supported: {}. Use IP address instead.",
                            ip_str
                        ));
                    }
                }
            };
            Ok(SocketAddr::new(host, port))
        } else {
            // Fall back to deprecated field
            #[allow(deprecated)]
            Ok(self.listen_addr)
        }
    }

    /// Get interface name (for interface-based protocols)
    pub fn interface(&self) -> Result<&str> {
        self.interface.as_deref()
            .ok_or_else(|| anyhow::anyhow!("Interface required"))
    }

    /// Get MAC address (for layer 2 protocols)
    pub fn mac_address(&self) -> Result<&str> {
        self.mac_address.as_deref()
            .ok_or_else(|| anyhow::anyhow!("MAC address required"))
    }
}
```

### 3. Binding Defaults System

**File**: `src/protocol/binding_defaults.rs` (new file)

```rust
/// Default binding parameters for a protocol
#[derive(Debug, Clone, Default)]
pub struct BindingDefaults {
    pub mac_address: Option<String>,
    pub interface: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl BindingDefaults {
    /// Apply defaults to user-provided values
    /// User values take precedence, defaults fill in gaps
    pub fn apply(
        &self,
        mac: Option<String>,
        interface: Option<String>,
        host: Option<String>,
        port: Option<u16>,
    ) -> (Option<String>, Option<String>, Option<String>, Option<u16>) {
        (
            mac.or_else(|| self.mac_address.clone()),
            interface.or_else(|| self.interface.clone()),
            ip.or_else(|| self.ip.clone()),
            port.or(self.port),
        )
    }

    /// Create defaults for port-based protocols
    pub fn port_based() -> Self {
        Self {
            mac_address: None,
            interface: None,
            host: Some("127.0.0.1".to_string()),  // Localhost by default
            port: Some(0),                       // Dynamic port by default
        }
    }

    /// Create defaults for interface-based protocols
    pub fn interface_based() -> Self {
        Self {
            mac_address: None,
            interface: Some("lo".to_string()),  // Loopback by default
            host: None,
            port: None,
        }
    }
}
```

### 4. Protocol Trait Changes (Backwards Compatible)

**File**: `src/llm/actions/protocol_trait.rs`

Add new method with default implementation:

```rust
#[async_trait]
pub trait Server: Send + Sync {
    /// Get default binding parameters for this protocol
    ///
    /// Default implementation returns None, indicating the protocol
    /// hasn't been migrated yet and should use the old listen_addr path.
    ///
    /// Migrated protocols should override this to return Some(BindingDefaults).
    fn default_binding(&self) -> Option<BindingDefaults> {
        None  // ← Backwards compatible: old protocols return None
    }

    /// Spawn a new server instance
    ///
    /// Returns a string describing where the server is listening.
    /// For backwards compatibility, protocols can still return SocketAddr
    /// formatted as string.
    async fn spawn(&self, ctx: SpawnContext) -> Result<String>;

    // ... rest unchanged (get_async_actions, get_sync_actions, etc.)
}
```

**Migration Indicator**: Protocols opt-in by implementing `default_binding()` to return `Some(...)`.

### 5. Server Startup Changes (Backwards Compatible)

**File**: `src/cli/server_startup.rs`

#### Function Signature (Backwards Compatible):

```rust
pub async fn start_server_from_action(
    state: &AppState,
    mac_address: Option<String>,     // ← NEW
    interface: Option<String>,       // ← NEW
    host: Option<String>,              // ← NEW
    port: Option<u16>,               // ← NOW OPTIONAL (breaking, but has defaults)
    base_stack: &str,
    _send_first: bool,
    initial_memory: Option<String>,
    instruction: String,
    startup_params: Option<serde_json::Value>,
    event_handlers: Option<Vec<serde_json::Value>>,
    scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    feedback_instructions: Option<String>,
) -> Result<ServerId>
```

#### Default Application Logic (Backwards Compatible):

```rust
// Get protocol from registry
let protocol = crate::protocol::server_registry::registry()
    .get(base_stack)
    .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", base_stack))?;

// Check if protocol has been migrated to new binding system
let (final_mac, final_interface, final_host, final_port, use_new_path) =
    if let Some(defaults) = protocol.default_binding() {
        // NEW PATH: Protocol has been migrated, use flexible binding
        let (mac, iface, host, port) = defaults.apply(mac_address, interface, host, port);
        (mac, iface, host, port, true)
    } else {
        // OLD PATH: Protocol hasn't been migrated, use backwards-compatible behavior
        // For old protocols, port is required (use default 0 if not provided)
        let final_port = port.unwrap_or(0);
        (
            None,  // mac_address ignored for old protocols
            None,  // interface ignored for old protocols
            Some("127.0.0.1".to_string()),  // Always use localhost for old protocols
            Some(final_port),
            false  // use old path
        )
    };

// Handle dynamic port assignment if port is 0
let actual_port = if final_port == Some(0) {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let found_port = listener.local_addr()?.port();
    drop(listener);
    Some(found_port)
} else {
    final_port
};

// Create server instance
let server = ServerInstance {
    id: ServerId::new(0),
    binding: ServerBinding {
        mac_address: final_mac.clone(),
        interface: final_interface.clone(),
        host: final_host.clone(),
        port: actual_port,
        local_addr: None,  // Will be filled after spawn
    },
    protocol_name: base_stack.to_string(),
    instruction: instruction.clone(),
    memory: String::new(),
    status: ServerStatus::Starting,
    // ... rest of initialization
};

let server_id = state.add_server(server).await;

// Build spawn context
let spawn_ctx = if use_new_path {
    // NEW PATH: Use new flexible binding fields
    SpawnContext {
        listen_addr: "0.0.0.0:0".parse().unwrap(),  // Dummy value (deprecated)
        mac_address: final_mac,
        interface: final_interface,
        host: final_host.clone(),
        port: actual_port,
        llm_client,
        state: Arc::new(state.clone()),
        status_tx,
        server_id,
        startup_params: startup_params_obj,
    }
} else {
    // OLD PATH: Build listen_addr for backwards compatibility
    let listen_addr = format!("127.0.0.1:{}", actual_port.unwrap_or(0))
        .parse()
        .unwrap();

    SpawnContext {
        listen_addr,  // OLD protocols use this
        mac_address: None,
        interface: None,
        host: None,
        port: None,
        llm_client,
        state: Arc::new(state.clone()),
        status_tx,
        server_id,
        startup_params: startup_params_obj,
    }
};

// Spawn the server
match protocol.spawn(spawn_ctx).await {
    Ok(server_address) => {
        let msg = format!(
            "[SERVER] Starting server #{} ({}) on {}",
            server_id.as_u32(),
            base_stack,
            server_address
        );
        let _ = status_tx.send(msg);

        // Update server state with actual address
        // ... (update binding.local_addr if applicable)

        Ok(server_id)
    }
    Err(e) => {
        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
        Err(e)
    }
}
```

### 6. Server State Changes (Backwards Compatible)

**File**: `src/state/server.rs`

Add new `ServerBinding` struct, use it in `ServerInstance`:

```rust
/// Server binding information (new flexible binding system)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Actual bound address (for port-based protocols)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_addr: Option<SocketAddr>,
}

pub struct ServerInstance {
    pub id: ServerId,

    /// NEW FIELD: Flexible binding information
    #[serde(default)]
    pub binding: ServerBinding,

    /// OLD FIELDS (DEPRECATED, for backwards compatibility)
    /// These will be removed in v2.0
    #[deprecated(since = "1.x.0", note = "Use binding.port instead")]
    #[serde(skip)]  // Don't serialize, derive from binding
    pub port: u16,

    #[deprecated(since = "1.x.0", note = "Use binding.local_addr instead")]
    #[serde(skip)]  // Don't serialize, derive from binding
    pub local_addr: Option<SocketAddr>,

    pub protocol_name: String,
    pub instruction: String,
    pub memory: String,
    pub status: ServerStatus,
    pub connections: HashMap<ConnectionId, ConnectionInfo>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub created_at: std::time::Instant,
    pub status_changed_at: std::time::Instant,
    pub startup_params: Option<serde_json::Value>,
    pub event_handler_config: Option<EventHandlerConfig>,
    pub protocol_data: serde_json::Value,
    pub log_files: HashMap<String, Vec<String>>,
    pub feedback_instructions: Option<String>,
    pub feedback_buffer: Vec<serde_json::Value>,
    pub last_feedback_processed: Option<std::time::Instant>,
}
```

**WAIT - Simpler approach**: Keep old fields, add new field alongside:

```rust
pub struct ServerInstance {
    pub id: ServerId,

    /// OLD FIELD: Port (for backwards compatibility)
    /// Will be migrated to binding.port
    pub port: u16,

    /// NEW FIELD: Flexible binding (empty for unmigrated protocols)
    #[serde(default)]
    pub binding: Option<ServerBinding>,

    pub protocol_name: String,
    pub instruction: String,
    pub memory: String,
    pub status: ServerStatus,
    pub connections: HashMap<ConnectionId, ConnectionInfo>,
    pub local_addr: Option<SocketAddr>,  // Keep this too
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub created_at: std::time::Instant,
    pub status_changed_at: std::time::Instant,
    pub startup_params: Option<serde_json::Value>,
    pub event_handler_config: Option<EventHandlerConfig>,
    pub protocol_data: serde_json::Value,
    pub log_files: HashMap<String, Vec<String>>,
    pub feedback_instructions: Option<String>,
    pub feedback_buffer: Vec<serde_json::Value>,
    pub last_feedback_processed: Option<std::time::Instant>,
}
```

**Helper Methods** (backwards compatible):

```rust
impl ServerInstance {
    /// Get port (works for both old and new protocols)
    pub fn get_port(&self) -> Option<u16> {
        if let Some(ref binding) = self.binding {
            binding.port
        } else {
            Some(self.port)  // Old field
        }
    }

    /// Display address for UI (works for both old and new)
    pub fn display_address(&self) -> String {
        if let Some(ref binding) = self.binding {
            // New flexible binding
            match (&binding.interface, &binding.local_addr, binding.port) {
                (Some(iface), None, None) => iface.clone(),
                (_, Some(addr), _) => addr.to_string(),
                (_, None, Some(port)) => {
                    if let Some(ref host) = binding.ip {
                        format!("{}:{}", host, port)
                    } else {
                        format!("0.0.0.0:{}", port)
                    }
                }
                (Some(iface), None, Some(port)) => {
                    if let Some(ref host) = binding.ip {
                        format!("{} ({}:{})", iface, host, port)
                    } else {
                        format!("{} (port {})", iface, port)
                    }
                }
                _ => "unknown".to_string(),
            }
        } else {
            // Old port-based binding
            if let Some(addr) = self.local_addr {
                addr.to_string()
            } else {
                format!("0.0.0.0:{}", self.port)
            }
        }
    }
}
```

### 7. Protocol Migration Examples

#### Before Migration: TCP Protocol (Old Way)

**File**: `src/server/tcp/actions.rs`

```rust
impl Server for TcpProtocol {
    // No default_binding() implementation - uses old path

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Uses deprecated listen_addr field
        #[allow(deprecated)]
        let socket_addr = ctx.listen_addr;

        let actual_addr = crate::server::tcp::TcpServer::spawn_with_llm(
            socket_addr,
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
        ).await?;

        Ok(actual_addr.to_string())
    }
}
```

**Status**: Works perfectly, no changes needed.

#### After Migration: TCP Protocol (New Way)

**File**: `src/server/tcp/actions.rs`

```rust
impl Server for TcpProtocol {
    // NEW: Implement default_binding() to opt-in to flexible binding
    fn default_binding(&self) -> Option<BindingDefaults> {
        Some(BindingDefaults {
            mac_address: None,
            interface: None,
            host: Some("127.0.0.1".to_string()),  // Default to localhost
            port: Some(0),                       // Default to dynamic port
        })
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // NEW: Use new socket_addr() helper (handles both old and new fields)
        let socket_addr = ctx.socket_addr()
            .context("TCP requires IP address and port")?;

        let actual_addr = crate::server::tcp::TcpServer::spawn_with_llm(
            socket_addr,
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
        ).await?;

        Ok(actual_addr.to_string())
    }
}
```

**Changes**:
1. Add `default_binding()` implementation
2. Use `ctx.socket_addr()` instead of `ctx.listen_addr`

#### New Protocol: ICMP (Uses New System from Start)

**File**: `src/server/icmp/actions.rs`

```rust
impl Server for IcmpProtocol {
    fn default_binding(&self) -> Option<BindingDefaults> {
        Some(BindingDefaults {
            mac_address: None,
            interface: Some("lo".to_string()),  // Default to loopback
            host: None,
            port: None,
        })
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract interface (required for ICMP)
        let interface = ctx.interface()
            .context("ICMP requires interface")?
            .to_string();

        // Spawn ICMP server
        crate::server::icmp::IcmpServer::spawn_with_llm(
            interface.clone(),
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
        ).await?;

        Ok(interface)
    }
}
```

**Status**: Uses new system, provides default interface.

#### New Protocol: ARP (Interface + Optional MAC)

**File**: `src/server/arp/actions.rs`

```rust
impl Server for ArpProtocol {
    fn default_binding(&self) -> Option<BindingDefaults> {
        Some(BindingDefaults {
            mac_address: None,                  // Optional (uses interface's MAC)
            interface: Some("lo".to_string()),  // Default to loopback
            host: None,
            port: None,
        })
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract interface (required)
        let interface = ctx.interface()
            .context("ARP requires interface")?
            .to_string();

        // MAC address is optional
        let mac = ctx.mac_address.clone();

        // Spawn ARP server
        crate::server::arp::ArpServer::spawn_with_llm(
            interface.clone(),
            mac.clone(),
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
        ).await?;

        Ok(if let Some(mac_addr) = mac {
            format!("{} (MAC: {})", interface, mac_addr)
        } else {
            interface
        })
    }
}
```

## Migration Guide

### For Each Protocol

**Step 1**: Identify what the protocol needs (MAC, interface, IP, port)

**Step 2**: Implement `default_binding()` method:
```rust
fn default_binding(&self) -> Option<BindingDefaults> {
    Some(BindingDefaults {
        mac_address: None,  // or Some("...")
        interface: None,    // or Some("lo")
        host: Some("127.0.0.1".to_string()),  // or None
        port: Some(0),      // or None
    })
}
```

**Step 3**: Update `spawn()` to use new context fields:
- Replace `ctx.listen_addr` with `ctx.socket_addr()?`
- Or use `ctx.interface()?`, `ctx.mac_address()?` as needed

**Step 4**: Test the protocol with:
- Default binding (no parameters in action)
- Custom binding (specific parameters in action)
- Combined binding (e.g., interface + IP + port)

### Migration Priority

**Phase 1: Infrastructure** (0 protocols affected)
- Add BindingDefaults
- Update SpawnContext (keep listen_addr)
- Update Server trait (default_binding() with default None)
- Update start_server_from_action (handle both paths)
- Update ServerInstance (add optional binding field)

**Phase 2: Interface-based protocols** (CRITICAL - unblocks ICMP test)
- ICMP ← **HIGH PRIORITY**
- ARP
- DataLink

**Phase 3: Port-based protocols with interface needs**
- WireGuard (needs interface + port)
- DHCP (may need interface)
- NTP (may need interface)

**Phase 4: Standard port-based protocols** (can migrate anytime)
- TCP
- HTTP
- DNS
- UDP
- All others (~40 protocols)

**Phase 5: Cleanup** (breaking change, requires major version)
- Remove deprecated `listen_addr` from SpawnContext
- Remove deprecated `port` from ServerInstance
- Remove backwards compatibility code

## Implementation Plan

### Phase 1: Core Infrastructure (Zero Breaking Changes)

1. **Create BindingDefaults** (`src/protocol/binding_defaults.rs`)
   - New file, no impact on existing code
   - ✅ No breaking changes

2. **Update Server trait** (`src/llm/actions/protocol_trait.rs`)
   - Add `default_binding()` with default impl returning `None`
   - ✅ No breaking changes (default impl)

3. **Update SpawnContext** (`src/protocol/spawn_context.rs`)
   - Add new optional fields (mac_address, interface, host, port)
   - **Keep** deprecated `listen_addr`
   - Add helper methods (socket_addr(), interface(), etc.)
   - ✅ No breaking changes (additive only)

4. **Update OpenServer action** (`src/llm/actions/common.rs`)
   - Make `port` optional (was required)
   - Add `mac_address`, `interface`, `host` (all optional)
   - ⚠️  **BREAKING**: `port` now optional, but handled with defaults
   - ✅ Mitigation: Old code passing port still works (Some(port))

5. **Update start_server_from_action** (`src/cli/server_startup.rs`)
   - Accept new optional parameters
   - Check if protocol has `default_binding()`
   - If yes: use new path with defaults
   - If no: use old path with listen_addr
   - ✅ No breaking changes (protocols choose their path)

6. **Update ServerInstance** (`src/state/server.rs`)
   - Add `binding: Option<ServerBinding>` field
   - **Keep** old `port` and `local_addr` fields
   - Add helper methods for backwards compat
   - ✅ No breaking changes (additive + helpers)

7. **Update all callers** (4 files)
   - Extract new fields from OpenServer action
   - Pass to start_server_from_action
   - ✅ No breaking changes (new fields are optional)

### Phase 2: Migrate Interface-Based Protocols (Unblocks ICMP)

8. **Migrate ICMP** (`src/server/icmp/actions.rs`)
   - Implement `default_binding()` → `{interface: "lo"}`
   - Update `spawn()` to use `ctx.interface()`
   - Test E2E with default and custom interface
   - ✅ ICMP E2E test now passes!

9. **Migrate ARP** (`src/server/arp/actions.rs`)
   - Implement `default_binding()` → `{interface: "lo"}`
   - Update `spawn()` to use `ctx.interface()` and optional `ctx.mac_address`
   - Test E2E

10. **Migrate DataLink** (`src/server/datalink/actions.rs`)
    - Similar to ARP

### Phase 3: Migrate Selected Port-Based Protocols (Validation)

11. **Migrate TCP** (proof of concept for port-based protocols)
    - Implement `default_binding()` → `{ip: "127.0.0.1", port: 0}`
    - Update `spawn()` to use `ctx.socket_addr()`
    - Test E2E

12. **Migrate HTTP**
    - Same pattern as TCP

13. **Migrate DNS**
    - Same pattern as TCP

### Phase 4: Migrate Remaining Protocols (As Needed)

14. **Migrate protocols one by one**
    - ~40 protocols remaining
    - Low priority, can be done gradually
    - Each protocol follows same pattern

### Phase 5: Future Cleanup (Breaking Change)

15. **Remove deprecated fields** (requires major version bump)
    - Remove `SpawnContext.listen_addr`
    - Remove `ServerInstance.port`
    - Remove `ServerInstance.local_addr`
    - Remove backwards compatibility code
    - Make `Server::default_binding()` required (return `BindingDefaults`, not `Option`)

## Migration Status Tracking

| Protocol | Status | Defaults | Notes |
|----------|--------|----------|-------|
| **ICMP** | ⬜ Not Migrated | `{interface: "lo"}` | **High priority** - unblocks E2E test |
| **ARP** | ⬜ Not Migrated | `{interface: "lo"}` | High priority |
| **DataLink** | ⬜ Not Migrated | `{interface: "lo"}` | High priority |
| **TCP** | ⬜ Not Migrated | `{ip: "127.0.0.1", port: 0}` | Proof of concept |
| **HTTP** | ⬜ Not Migrated | `{ip: "127.0.0.1", port: 0}` | Common protocol |
| **DNS** | ⬜ Not Migrated | `{ip: "127.0.0.1", port: 0}` | Common protocol |
| ... | ⬜ Not Migrated | ... | ~40 more protocols |

**Legend**:
- ⬜ Not Migrated (uses old listen_addr path)
- 🟡 In Progress (implementing default_binding)
- ✅ Migrated (using new flexible binding)

## Usage Examples

### Port-Based Protocols

**HTTP with defaults**:
```json
{
  "type": "open_server",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests"
}
```
→ Binds to `127.0.0.1:8080` (IP defaults to localhost)

**HTTP with hostname**:
```json
{
  "type": "open_server",
  "host": "localhost",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests"
}
```
→ Binds to `127.0.0.1:8080` (localhost resolved)

**HTTP on specific IP**:
```json
{
  "type": "open_server",
  "host": "192.168.1.100",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests"
}
```
→ Binds to `192.168.1.100:8080`

**HTTP on specific interface**:
```json
{
  "type": "open_server",
  "interface": "eth0",
  "host": "192.168.1.100",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP on eth0"
}
```
→ Binds to `192.168.1.100:8080` on interface `eth0`

**TCP with dynamic port** (unmigrated protocol):
```json
{
  "type": "open_server",
  "port": 0,
  "base_stack": "TCP",
  "instruction": "Accept TCP connections"
}
```
→ Binds to `127.0.0.1:<random_port>` (works with old path)

**TCP with dynamic port** (after migration):
```json
{
  "type": "open_server",
  "base_stack": "TCP",
  "instruction": "Accept TCP connections"
}
```
→ Binds to `127.0.0.1:<random_port>` (defaults: ip="127.0.0.1", port=0)

### Interface-Based Protocols

**ICMP with default**:
```json
{
  "type": "open_server",
  "base_stack": "ICMP",
  "instruction": "Respond to ICMP echo requests"
}
```
→ Binds to interface `lo` (protocol default)

**ICMP on specific interface**:
```json
{
  "type": "open_server",
  "interface": "eth0",
  "base_stack": "ICMP",
  "instruction": "Respond to ICMP echo requests on eth0"
}
```
→ Binds to interface `eth0`

**ARP with default**:
```json
{
  "type": "open_server",
  "base_stack": "ARP",
  "instruction": "Respond to ARP requests"
}
```
→ Binds to interface `lo`, uses interface's actual MAC

**ARP with custom MAC**:
```json
{
  "type": "open_server",
  "interface": "eth0",
  "mac_address": "00:11:22:33:44:55",
  "base_stack": "ARP",
  "instruction": "Respond to ARP with custom MAC"
}
```
→ Binds to interface `eth0`, spoofs MAC as `00:11:22:33:44:55`

### Advanced Use Cases

**DHCP server on specific interface**:
```json
{
  "type": "open_server",
  "interface": "eth0",
  "port": 67,
  "base_stack": "DHCP",
  "instruction": "Provide DHCP services"
}
```
→ Binds to UDP port 67 on interface `eth0`

**WireGuard tunnel**:
```json
{
  "type": "open_server",
  "interface": "wg0",
  "host": "10.0.0.1",
  "port": 51820,
  "base_stack": "WireGuard",
  "instruction": "Run WireGuard VPN"
}
```
→ Creates tunnel on interface `wg0` with IP `10.0.0.1:51820`

## Breaking Changes Summary

### During Migration (Minimal Breaking Changes)

1. **OpenServer.port becomes optional**
   - Impact: LLM prompts, test code
   - Mitigation: Protocol defaults fill in missing values
   - Risk: **LOW** - old code passing port still works

2. **start_server_from_action signature adds parameters**
   - Impact: 4 call sites
   - Mitigation: Extract from action, pass None if not present
   - Risk: **MEDIUM** - requires changes to 4 files

### After Full Migration (Major Version Bump)

3. **Remove deprecated fields**
   - `SpawnContext.listen_addr`
   - `ServerInstance.port` and `local_addr`
   - Impact: Any code using old fields
   - Mitigation: All protocols migrated by this point
   - Risk: **HIGH** - breaking change, requires v2.0

## Verification Checklist

**Phase 1: Infrastructure**
- [ ] `BindingDefaults` created with `apply()` and helper constructors
- [ ] `Server::default_binding()` added with default `None` impl
- [ ] `SpawnContext` has new fields + deprecated `listen_addr` + helpers
- [ ] `OpenServer` action has all new optional fields
- [ ] `start_server_from_action` detects migration status and uses correct path
- [ ] `ServerInstance` has `binding: Option<ServerBinding>` + old fields + helpers
- [ ] All 4 callers updated to pass new parameters
- [ ] **All existing tests pass** (no regressions)

**Phase 2: ICMP Migration**
- [ ] ICMP implements `default_binding()` returning `Some(...)`
- [ ] ICMP `spawn()` uses `ctx.interface()`
- [ ] ICMP E2E test passes with default interface
- [ ] ICMP E2E test passes with custom interface
- [ ] **All existing tests still pass**

**Phase 3: Port-Based Migration**
- [ ] TCP implements `default_binding()`
- [ ] TCP `spawn()` uses `ctx.socket_addr()`
- [ ] HTTP implements `default_binding()`
- [ ] DNS implements `default_binding()`
- [ ] **All existing tests still pass**

**Phase 4: Validation**
- [ ] Combined binding works (interface + IP + port)
- [ ] Hostname resolution works (localhost → 127.0.0.1)
- [ ] UI displays bindings correctly for both old and new protocols
- [ ] State serialization/deserialization works
- [ ] Can migrate protocols independently without breaking others

## Future Enhancements

1. **Hostname resolution**: Full DNS resolution for `host` field
2. **Interface validation**: Check if interface exists before binding
3. **IP-interface validation**: Verify IP belongs to specified interface
4. **IPv6 support**: Ensure parsing handles IPv6 correctly
5. **Interface discovery**: List available interfaces in UI/CLI
6. **MAC address validation**: Parse and validate MAC address format
7. **Multi-interface binding**: Allow protocols to bind to multiple interfaces
8. **Interface aliases**: Support "any", "all", "loopback" aliases
9. **Dynamic interface switching**: Detect interface changes, rebind
10. **Privilege checking**: Validate CAP_NET_RAW before spawning raw protocols
11. **Binding presets**: Save/load common binding configurations

## References

- **Current ICMP implementation**: `src/server/icmp/mod.rs`
- **Current ARP implementation**: `src/server/arp/mod.rs`
- **E2E test blocker**: `tests/server/icmp/e2e_test.rs` line 106
- **Port-based example**: `src/server/tcp/actions.rs`
- **Action handling**: `src/events/handler.rs` → `src/cli/server_startup.rs`
- **State management**: `src/state/server.rs`
- **Protocol trait**: `src/llm/actions/protocol_trait.rs`
