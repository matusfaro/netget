# Flexible Protocol Binding Architecture

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

## Design Philosophy: Flexible Binding

Instead of "port OR interface", support **all binding parameters as optional**:

- **MAC address**: For layer 2 protocols (e.g., spoofing source MAC in ARP)
- **Interface**: For raw socket protocols (ICMP, ARP, DataLink)
- **IP address**: For specific IP binding (IPv4 or IPv6)
- **Port**: For transport layer protocols (TCP, UDP, etc.)

**Key Insight**: A protocol may need **zero or more** of these parameters. Let each protocol decide what it needs and provide sensible defaults.

### Examples

**TCP server on specific interface**:
```json
{"interface": "eth0", "ip": "192.168.1.100", "port": 8080, "base_stack": "TCP"}
```

**HTTP server with defaults**:
```json
{"port": 8080, "base_stack": "HTTP"}
// Defaults: ip = "127.0.0.1", interface = any
```

**ICMP server on loopback**:
```json
{"interface": "lo", "base_stack": "ICMP"}
// Defaults: No port needed
```

**ARP server with custom MAC**:
```json
{"interface": "eth0", "mac_address": "00:11:22:33:44:55", "base_stack": "ARP"}
```

**DataLink server**:
```json
{"interface": "eth0", "base_stack": "DataLink"}
```

## Protocol Requirements Matrix

| Protocol | MAC | Interface | IP | Port | Notes |
|----------|-----|-----------|----|----|-------|
| **TCP** | - | Optional | Optional | **Required** | Default IP: 127.0.0.1, Default port: 0 (dynamic) |
| **HTTP** | - | Optional | Optional | **Required** | Same as TCP |
| **UDP** | - | Optional | Optional | **Required** | Same as TCP |
| **DNS** | - | Optional | Optional | **Required** | Same as TCP |
| **ICMP** | - | **Required** | Optional | - | Default interface: "lo" |
| **ARP** | Optional | **Required** | Optional | - | Default interface: "lo" |
| **DataLink** | Optional | **Required** | - | - | Default interface: "lo" |
| **WireGuard** | - | **Required** | Optional | Optional | Needs interface for tunnel |
| **DHCP** | - | Optional | Optional | **Required** | UDP-based but may need interface |

### Protocol-Specific Defaults

Each protocol defines its own defaults through the `Server` trait:

```rust
trait Server {
    fn default_binding_params(&self) -> BindingDefaults {
        BindingDefaults {
            mac_address: None,
            interface: None,
            ip: None,
            port: None,
        }
    }
}
```

**Examples**:
- **TCP**: `{ip: Some("127.0.0.1"), port: Some(0)}`
- **ICMP**: `{interface: Some("lo")}`
- **ARP**: `{interface: Some("lo")}`
- **HTTP**: `{ip: Some("127.0.0.1"), port: Some(0)}`

## Proposed Solution

### 1. Action Schema Changes

**File**: `src/llm/actions/common.rs`

Make all binding parameters optional:

```rust
OpenServer {
    /// MAC address (for layer 2 protocols like ARP spoofing)
    /// Format: "00:11:22:33:44:55"
    #[serde(default)]
    mac_address: Option<String>,

    /// Network interface to bind (for raw protocols like ICMP, ARP, DataLink)
    /// Examples: "lo", "eth0", "wlan0", "any"
    #[serde(default)]
    interface: Option<String>,

    /// IP address to bind (IPv4 or IPv6)
    /// Examples: "127.0.0.1", "0.0.0.0", "::1", "::"
    #[serde(default)]
    ip: Option<String>,

    /// Port to bind (for socket-based protocols like TCP, HTTP, DNS)
    /// Use 0 for dynamic port assignment
    #[serde(default)]
    port: Option<u16>,

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

**Validation**: Protocols validate their requirements individually. No global validation needed.

### 2. SpawnContext Changes

**File**: `src/protocol/spawn_context.rs`

Replace `listen_addr: SocketAddr` with flexible binding parameters:

```rust
pub struct SpawnContext {
    /// MAC address (optional, for layer 2 protocols)
    pub mac_address: Option<String>,

    /// Network interface (optional, for raw socket protocols)
    pub interface: Option<String>,

    /// IP address to bind (optional, for socket protocols)
    pub ip: Option<std::net::IpAddr>,

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

**Helper Methods**:

```rust
impl SpawnContext {
    /// Build SocketAddr from IP and port (for socket-based protocols)
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        let ip = self.ip.ok_or_else(|| anyhow::anyhow!("IP address required"))?;
        let port = self.port.ok_or_else(|| anyhow::anyhow!("Port required"))?;
        Ok(SocketAddr::new(ip, port))
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
    pub ip: Option<String>,
    pub port: Option<u16>,
}

impl BindingDefaults {
    /// Apply defaults to user-provided values
    pub fn apply(&self,
        mac: Option<String>,
        interface: Option<String>,
        ip: Option<String>,
        port: Option<u16>,
    ) -> (Option<String>, Option<String>, Option<String>, Option<u16>) {
        (
            mac.or_else(|| self.mac_address.clone()),
            interface.or_else(|| self.interface.clone()),
            ip.or_else(|| self.ip.clone()),
            port.or(self.port),
        )
    }
}
```

### 4. Protocol Trait Changes

**File**: `src/llm/actions/protocol_trait.rs`

Add method to get default binding parameters:

```rust
#[async_trait]
pub trait Server: Send + Sync {
    /// Get default binding parameters for this protocol
    fn default_binding(&self) -> BindingDefaults {
        BindingDefaults::default()
    }

    /// Spawn a new server instance
    /// Returns a string describing where the server is listening
    async fn spawn(&self, ctx: SpawnContext) -> Result<String>;

    // ... rest unchanged
}
```

**Return Type**: Change from `Result<SocketAddr>` to `Result<String>`:
- Port-based: `"127.0.0.1:8080"`
- Interface-based: `"lo"`
- Combined: `"eth0 (192.168.1.100:8080)"`

### 5. Server Startup Changes

**File**: `src/cli/server_startup.rs`

#### Function Signature:

```rust
pub async fn start_server_from_action(
    state: &AppState,
    mac_address: Option<String>,     // ← NEW
    interface: Option<String>,       // ← NEW
    ip: Option<String>,              // ← NEW (was always 127.0.0.1)
    port: Option<u16>,               // ← NOW OPTIONAL
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

#### Default Application Logic:

```rust
// Get protocol from registry
let protocol = crate::protocol::server_registry::registry()
    .get(base_stack)
    .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", base_stack))?;

// Get protocol defaults
let defaults = protocol.default_binding();

// Apply defaults to user-provided values
let (final_mac, final_interface, final_ip_str, final_port) =
    defaults.apply(mac_address, interface, ip, port);

// Parse IP address if provided
let final_ip = if let Some(ip_str) = final_ip_str {
    Some(ip_str.parse::<IpAddr>()
        .with_context(|| format!("Invalid IP address: {}", ip_str))?)
} else {
    None
};

// Handle dynamic port assignment if port is 0
let actual_port = if let Some(0) = final_port {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let found_port = listener.local_addr()?.port();
    drop(listener);
    Some(found_port)
} else {
    final_port
};

// Build spawn context with all parameters
let spawn_ctx = SpawnContext {
    mac_address: final_mac,
    interface: final_interface,
    ip: final_ip,
    port: actual_port,
    llm_client,
    state: Arc::new(state.clone()),
    status_tx,
    server_id,
    startup_params: startup_params_obj,
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
        Ok(server_id)
    }
    Err(e) => Err(e)
}
```

### 6. Server State Changes

**File**: `src/state/server.rs`

Replace `port` with flexible binding information:

```rust
/// Server binding information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<IpAddr>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Actual bound address (for port-based protocols)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_addr: Option<SocketAddr>,
}

pub struct ServerInstance {
    pub id: ServerId,
    pub binding: ServerBinding,           // ← REPLACES: port + local_addr
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

**Helper Methods**:

```rust
impl ServerInstance {
    /// Get port if port-based protocol
    pub fn port(&self) -> Option<u16> {
        self.binding.port
    }

    /// Display address for UI
    pub fn display_address(&self) -> String {
        match (&self.binding.interface, &self.binding.local_addr, self.binding.port) {
            // Interface-based
            (Some(iface), None, None) => iface.clone(),

            // Port-based with actual address
            (_, Some(addr), _) => addr.to_string(),

            // Port-based with IP + port
            (_, None, Some(port)) => {
                if let Some(ip) = &self.binding.ip {
                    format!("{}:{}", ip, port)
                } else {
                    format!("0.0.0.0:{}", port)
                }
            }

            // Combined (interface + IP + port)
            (Some(iface), None, Some(port)) => {
                if let Some(ip) = &self.binding.ip {
                    format!("{} ({}:{})", iface, ip, port)
                } else {
                    format!("{} (port {})", iface, port)
                }
            }

            // Fallback
            _ => "unknown".to_string(),
        }
    }
}
```

### 7. Protocol Implementation Examples

#### TCP Protocol (Port-Based)

**File**: `src/server/tcp/actions.rs`

```rust
impl Server for TcpProtocol {
    fn default_binding(&self) -> BindingDefaults {
        BindingDefaults {
            mac_address: None,
            interface: None,
            ip: Some("127.0.0.1".to_string()),  // Default to localhost
            port: Some(0),                       // Default to dynamic port
        }
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract IP and port (required for TCP)
        let socket_addr = ctx.socket_addr()
            .context("TCP requires IP address and port")?;

        // Spawn TCP server
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

#### ICMP Protocol (Interface-Based)

**File**: `src/server/icmp/actions.rs`

```rust
impl Server for IcmpProtocol {
    fn default_binding(&self) -> BindingDefaults {
        BindingDefaults {
            mac_address: None,
            interface: Some("lo".to_string()),  // Default to loopback
            ip: None,
            port: None,
        }
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

#### ARP Protocol (Interface + Optional MAC)

**File**: `src/server/arp/actions.rs`

```rust
impl Server for ArpProtocol {
    fn default_binding(&self) -> BindingDefaults {
        BindingDefaults {
            mac_address: None,                  // Optional (protocol will use interface's MAC)
            interface: Some("lo".to_string()),  // Default to loopback
            ip: None,
            port: None,
        }
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract interface (required for ARP)
        let interface = ctx.interface()
            .context("ARP requires interface")?
            .to_string();

        // MAC address is optional (will use interface's actual MAC if not provided)
        let mac = ctx.mac_address.clone();

        // Spawn ARP server (pass MAC if provided)
        crate::server::arp::ArpServer::spawn_with_llm(
            interface.clone(),
            mac,
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

#### HTTP Protocol (Port-Based, Optional Interface)

**File**: `src/server/http/actions.rs`

```rust
impl Server for HttpProtocol {
    fn default_binding(&self) -> BindingDefaults {
        BindingDefaults {
            mac_address: None,
            interface: None,                    // Optional (can bind to specific interface IP)
            ip: Some("127.0.0.1".to_string()),  // Default to localhost
            port: Some(0),                       // Default to dynamic port
        }
    }

    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract IP and port (required for HTTP)
        let socket_addr = ctx.socket_addr()
            .context("HTTP requires IP address and port")?;

        // If interface specified, could validate IP belongs to that interface
        // (optional feature for advanced use cases)

        // Spawn HTTP server
        let actual_addr = crate::server::http::HttpServer::spawn_with_llm(
            socket_addr,
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
            ctx.startup_params,
        ).await?;

        Ok(if let Some(iface) = ctx.interface {
            format!("{} ({})", iface, actual_addr)
        } else {
            actual_addr.to_string()
        })
    }
}
```

### 8. Caller Updates

All callers of `start_server_from_action()` need to pass the new parameters:

**Files to Update**:
- `src/cli/rolling_tui.rs` (line ~2589)
- `src/cli/non_interactive.rs` (line ~309)
- `src/events/handler.rs` (line ~1809)
- `src/cli/easy_startup.rs` (line ~136)

**Pattern**:

```rust
// Extract all binding parameters from OpenServer action
CommonAction::OpenServer {
    mac_address,
    interface,
    ip,
    port,
    base_stack,
    send_first,
    initial_memory,
    instruction,
    startup_params,
    event_handlers,
    scheduled_tasks,
    feedback_instructions,
} => {
    match server_startup::start_server_from_action(
        state,
        mac_address,     // ← Option<String>
        interface,       // ← Option<String>
        ip,              // ← Option<String>
        port,            // ← Option<u16>
        &base_stack,
        send_first,
        initial_memory,
        instruction.clone(),
        startup_params,
        event_handlers,
        scheduled_tasks,
        feedback_instructions,
    ).await {
        Ok(server_id) => { /* ... */ }
        Err(e) => { /* ... */ }
    }
}
```

### 9. Client Support

**File**: `src/llm/actions/common.rs`

Apply same pattern to `OpenClient`:

```rust
OpenClient {
    protocol: String,

    /// MAC address (for layer 2 client protocols)
    #[serde(default)]
    mac_address: Option<String>,

    /// Network interface (for raw socket clients)
    #[serde(default)]
    interface: Option<String>,

    /// Remote address (hostname:port or IP:port)
    /// Examples: "example.com:80", "192.168.1.1:6379"
    remote_addr: String,  // ← KEEP: Already flexible

    instruction: String,

    // ... rest unchanged
}
```

**File**: `src/protocol/connect_context.rs`

```rust
pub struct ConnectContext {
    /// MAC address (optional, for layer 2 protocols)
    pub mac_address: Option<String>,

    /// Network interface (optional, for raw socket protocols)
    pub interface: Option<String>,

    /// Remote server address (hostname:port or IP:port)
    pub remote_addr: String,

    /// LLM client for generating requests
    pub llm_client: OllamaClient,

    /// Application state
    pub state: Arc<AppState>,

    /// Channel for sending status updates to UI
    pub status_tx: mpsc::UnboundedSender<String>,

    /// Client ID for tracking
    pub client_id: ClientId,

    /// Optional startup parameters
    pub startup_params: Option<StartupParams>,
}
```

**Note**: Client `remote_addr` stays as String for flexibility (DNS names, IP:port, etc.).

## Implementation Plan

### Phase 1: Core Infrastructure (No Breaking Changes)

1. **Add BindingDefaults** (`src/protocol/binding_defaults.rs`)
   - Create new file with `BindingDefaults` struct
   - Implement `apply()` method for merging user values with defaults

2. **Update Server trait** (`src/llm/actions/protocol_trait.rs`)
   - Add `default_binding()` method with default impl
   - Change `spawn()` return from `Result<SocketAddr>` to `Result<String>`

3. **Update SpawnContext** (`src/protocol/spawn_context.rs`)
   - Add `mac_address`, `interface`, `ip` fields (all optional)
   - Keep `listen_addr` for backward compat (deprecated)
   - Add helper methods (`socket_addr()`, `interface()`, `mac_address()`)

### Phase 2: Action Schema Changes

4. **Update OpenServer action** (`src/llm/actions/common.rs`)
   - Make `port` optional
   - Add `mac_address`, `interface`, `ip` fields (all optional)
   - Update serialization/deserialization

5. **Update OpenClient action** (`src/llm/actions/common.rs`)
   - Add `mac_address`, `interface` fields (optional)
   - Keep `remote_addr` as String (already flexible)

6. **Update ConnectContext** (`src/protocol/connect_context.rs`)
   - Add `mac_address`, `interface` fields

### Phase 3: Server Startup Changes

7. **Update start_server_from_action** (`src/cli/server_startup.rs`)
   - Accept all four binding parameters
   - Get protocol defaults
   - Apply defaults to user values
   - Build SpawnContext with all parameters

8. **Update start_client_from_action** (`src/cli/client_startup.rs`)
   - Accept `mac_address` and `interface` parameters
   - Build ConnectContext with all parameters

### Phase 4: State Changes

9. **Update ServerInstance** (`src/state/server.rs`)
   - Replace `port` + `local_addr` with `binding: ServerBinding`
   - Add helper methods for backward compatibility
   - Update serialization for state persistence

10. **Update ClientInstance** (`src/state/client.rs`)
    - Add `binding` fields if needed for interface-based clients

### Phase 5: Protocol Updates

11. **Update interface-based protocols**
    - ICMP: `default_binding()` → `{interface: "lo"}`, extract interface in `spawn()`
    - ARP: `default_binding()` → `{interface: "lo"}`, extract interface + optional MAC
    - DataLink: `default_binding()` → `{interface: "lo"}`, extract interface + optional MAC

12. **Update port-based protocols**
    - TCP: `default_binding()` → `{ip: "127.0.0.1", port: 0}`, change return to String
    - HTTP: Same as TCP
    - DNS: Same as TCP
    - All others: Same pattern

### Phase 6: Caller Updates

13. **Update all callers**
    - `src/cli/rolling_tui.rs`
    - `src/cli/non_interactive.rs`
    - `src/events/handler.rs`
    - `src/cli/easy_startup.rs`

### Phase 7: Testing

14. **Test interface-based protocols**
    - ICMP E2E test with `{interface: "lo"}`
    - ARP E2E test
    - DataLink E2E test

15. **Test port-based protocols**
    - TCP, HTTP, DNS with default bindings
    - Verify backward compatibility

16. **Test combined bindings**
    - HTTP with `{interface: "eth0", ip: "192.168.1.100", port: 8080}`
    - Verify interface + port combination works

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

**HTTP on specific IP**:
```json
{
  "type": "open_server",
  "ip": "192.168.1.100",
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
  "ip": "192.168.1.100",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests on eth0"
}
```
→ Binds to `192.168.1.100:8080` on interface `eth0` (displayed as "eth0 (192.168.1.100:8080)")

**TCP with dynamic port**:
```json
{
  "type": "open_server",
  "port": 0,
  "base_stack": "TCP",
  "instruction": "Accept TCP connections"
}
```
→ Binds to `127.0.0.1:<random_port>` (OS assigns port)

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
  "instruction": "Respond to ARP requests with custom MAC"
}
```
→ Binds to interface `eth0`, spoofs MAC as `00:11:22:33:44:55`

**DataLink**:
```json
{
  "type": "open_server",
  "interface": "eth0",
  "base_stack": "DataLink",
  "instruction": "Capture all layer 2 traffic"
}
```
→ Binds to interface `eth0`

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
  "ip": "10.0.0.1",
  "port": 51820,
  "base_stack": "WireGuard",
  "instruction": "Run WireGuard VPN"
}
```
→ Creates tunnel on interface `wg0` with IP `10.0.0.1:51820`

## Breaking Changes Summary

### Unavoidable Breaking Changes

1. **OpenServer action schema**: `port` becomes optional
   - Impact: LLM prompts, test code
   - Mitigation: Protocol defaults handle missing values

2. **Server trait**: `spawn()` returns `String` instead of `SocketAddr`
   - Impact: ~50 protocol implementations
   - Mitigation: `.to_string()` is trivial conversion

3. **SpawnContext structure**: Add 3 new fields
   - Impact: ~50 protocol implementations
   - Mitigation: All fields optional, backward compat with helpers

4. **start_server_from_action signature**: Add 3 parameters
   - Impact: 4 call sites
   - Mitigation: Extract from action, pass through

5. **ServerInstance structure**: `port` + `local_addr` → `binding`
   - Impact: State access code, UI, tests
   - Mitigation: Helper methods for backward compat

### Migration Strategy

1. **Deprecation period**: Keep old fields marked as `#[deprecated]`
2. **Helper methods**: Provide `server.port()`, `ctx.socket_addr()`
3. **Gradual rollout**: Update protocols one at a time
4. **Comprehensive testing**: Run full test suite after each phase
5. **Documentation**: Update all examples and docs

## Verification Checklist

- [ ] `BindingDefaults` struct created
- [ ] `Server::default_binding()` method added
- [ ] `SpawnContext` has all binding fields
- [ ] `OpenServer` action accepts all parameters
- [ ] `OpenClient` action accepts MAC + interface
- [ ] `start_server_from_action` applies defaults
- [ ] `ServerInstance` uses `ServerBinding`
- [ ] ICMP protocol implements defaults and spawn
- [ ] ARP protocol implements defaults and spawn
- [ ] DataLink protocol implements defaults and spawn
- [ ] All port-based protocols return String
- [ ] All callers pass new parameters
- [ ] ICMP E2E test passes
- [ ] ARP E2E test passes
- [ ] All existing E2E tests pass
- [ ] UI displays bindings correctly
- [ ] State serialization works
- [ ] Combined bindings work (interface + IP + port)

## Future Enhancements

1. **Interface validation**: Check if interface exists before binding
2. **IP-interface validation**: Verify IP belongs to specified interface
3. **IPv6 support**: Extend IP parsing to handle IPv6
4. **Interface discovery**: List available interfaces in UI
5. **MAC address validation**: Parse and validate MAC address format
6. **Multi-interface binding**: Allow protocols to bind to multiple interfaces
7. **Interface aliases**: Support "any", "all", "loopback" aliases
8. **Dynamic interface switching**: Detect interface changes, rebind
9. **Privilege checking**: Validate CAP_NET_RAW for raw socket protocols
10. **Binding presets**: Save common binding configurations

## References

- **Current ICMP implementation**: `src/server/icmp/mod.rs`
- **Current ARP implementation**: `src/server/arp/mod.rs`
- **E2E test blocker**: `tests/server/icmp/e2e_test.rs` line 106
- **Port-based example**: `src/server/tcp/actions.rs`
- **Action handling flow**: `src/events/handler.rs` → `src/cli/server_startup.rs`
- **State management**: `src/state/server.rs`
