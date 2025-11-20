# Interface-Based Protocol Support Architecture

## Problem Statement

ICMP, ARP, and DataLink protocols operate at the network/link layer and require binding to network **interfaces** (e.g., "lo", "eth0") rather than IP addresses and **ports**. The current action handler and spawn infrastructure only supports port-based protocols.

### Current Blocker

When the ICMP E2E test tries to start a server with:
```json
{
  "type": "open_server",
  "interface": "lo",
  "base_stack": "ICMP",
  "instruction": "..."
}
```

**Error**: "Unknown action type, skipping: {...}"

**Root Cause**:
- `OpenServer` action requires `port: u16` field (not optional)
- `SpawnContext` has `listen_addr: SocketAddr` (IP:port combination)
- `start_server_from_action()` builds SocketAddr from port only
- No code path to handle interface-based protocols

## Current Architecture

### Port-Based Protocols (TCP, HTTP, DNS, etc.)

```
User → OpenServer{port: 8080} → start_server_from_action() → SpawnContext{listen_addr: 127.0.0.1:8080} → protocol.spawn()
```

### Interface-Based Protocols (ICMP, ARP, DataLink)

```
❌ BLOCKED: No action support
✅ IMPLEMENTED: IcmpServer::spawn_with_llm(interface: "lo", ...)
```

## Proposed Solution

### 1. Action Schema Changes

**File**: `src/llm/actions/common.rs`

Make both `port` and `interface` optional, with validation logic:

```rust
OpenServer {
    /// Port to bind (for socket-based protocols like TCP, HTTP, DNS)
    #[serde(default)]
    port: Option<u16>,

    /// Network interface to bind (for raw protocols like ICMP, ARP, DataLink)
    #[serde(default)]
    interface: Option<String>,

    base_stack: String,
    // ... rest unchanged
}
```

**Validation Rules**:
- Exactly one of `port` OR `interface` must be provided
- Error if both or neither are specified
- Port-based: `{"port": 8080, "base_stack": "HTTP"}`
- Interface-based: `{"interface": "lo", "base_stack": "ICMP"}`

**Default Values**:
- Port protocols: No default (explicit port required, or 0 for dynamic assignment)
- Interface protocols: Could default to "lo" or "any" (to be decided per protocol)

### 2. SpawnContext Changes

**File**: `src/protocol/spawn_context.rs`

Replace single `listen_addr: SocketAddr` with enum-based binding:

```rust
/// Binding type for protocol spawning
pub enum ProtocolBinding {
    /// Port-based binding (TCP, HTTP, UDP, DNS, etc.)
    Port(SocketAddr),

    /// Interface-based binding (ICMP, ARP, DataLink, etc.)
    Interface(String),
}

pub struct SpawnContext {
    /// What to bind to (port or interface)
    pub binding: ProtocolBinding,

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

**Alternative (simpler)**: Keep both fields, make them optional:

```rust
pub struct SpawnContext {
    /// Address to listen on (for port-based protocols)
    pub listen_addr: Option<SocketAddr>,

    /// Interface to bind (for interface-based protocols)
    pub interface: Option<String>,

    // ... rest unchanged
}
```

**Recommendation**: Use enum-based `ProtocolBinding` for type safety and clarity.

### 3. Protocol Trait Changes

**File**: `src/llm/actions/protocol_trait.rs`

The `Server` trait's `spawn()` method already returns `Result<SocketAddr>`. For interface-based protocols, this needs to change:

**Option A**: Keep SocketAddr return type, use dummy value for interface protocols:
```rust
// ICMP returns dummy SocketAddr (not ideal)
Ok("0.0.0.0:0".parse().unwrap())
```

**Option B**: Change return type to support both:
```rust
pub enum ServerAddress {
    Socket(SocketAddr),
    Interface(String),
}

#[async_trait]
pub trait Server: Send + Sync {
    async fn spawn(&self, ctx: SpawnContext) -> Result<ServerAddress>;
    // ... rest unchanged
}
```

**Option C**: Return String for all protocols:
```rust
async fn spawn(&self, ctx: SpawnContext) -> Result<String>;
// Port-based: "127.0.0.1:8080"
// Interface-based: "lo"
```

**Recommendation**: Option C (String) for simplicity and flexibility. Port-based protocols already convert SocketAddr → String for display.

### 4. Server Startup Changes

**File**: `src/cli/server_startup.rs`

#### Function Signature Change:

```rust
pub async fn start_server_from_action(
    state: &AppState,
    port: Option<u16>,           // ← NOW OPTIONAL
    interface: Option<String>,   // ← NEW PARAMETER
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

#### Validation Logic:

```rust
// Validate exactly one of port OR interface
match (port, interface.as_ref()) {
    (Some(_), Some(_)) => {
        return Err(anyhow::anyhow!(
            "Cannot specify both port and interface for {}", base_stack
        ));
    }
    (None, None) => {
        return Err(anyhow::anyhow!(
            "Must specify either port or interface for {}", base_stack
        ));
    }
    _ => {} // Valid: exactly one specified
}
```

#### Binding Creation:

```rust
// Build binding based on protocol type
let binding = if let Some(port_num) = port {
    // Port-based protocol
    let actual_port = if port_num == 0 {
        // Find available port
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let found_port = listener.local_addr()?.port();
        drop(listener);
        found_port
    } else {
        port_num
    };

    let listen_addr: SocketAddr = format!("127.0.0.1:{}", actual_port).parse()?;
    ProtocolBinding::Port(listen_addr)
} else if let Some(iface) = interface {
    // Interface-based protocol
    ProtocolBinding::Interface(iface.clone())
} else {
    unreachable!("Validation ensures one of port/interface is present");
};

// Create SpawnContext with binding
let spawn_ctx = SpawnContext {
    binding,
    llm_client,
    state: Arc::new(state.clone()),
    status_tx,
    server_id,
    startup_params: startup_params_obj,
};
```

#### Protocol Dispatch:

```rust
// Spawn the server using the protocol's spawn method
match protocol.spawn(spawn_ctx).await {
    Ok(server_address) => {
        // server_address is now a String ("127.0.0.1:8080" or "lo")
        let msg = format!(
            "[SERVER] Starting server #{} ({}) on {}",
            server_id.as_u32(),
            base_stack,
            server_address
        );
        let _ = status_tx.send(msg);

        // Update server state
        // ... (may need changes to ServerInstance to store binding type)

        Ok(server_id)
    }
    Err(e) => {
        // Error handling unchanged
        Err(e)
    }
}
```

### 5. Server State Changes

**File**: `src/state/server.rs`

The `ServerInstance` struct currently has:
```rust
pub struct ServerInstance {
    pub port: u16,  // ← Only for port-based
    pub local_addr: Option<SocketAddr>,  // ← Only for port-based
    // ...
}
```

**Proposed Changes**:

```rust
/// What the server is bound to
pub enum ServerBinding {
    Port { port: u16, addr: Option<SocketAddr> },
    Interface { name: String },
}

pub struct ServerInstance {
    pub binding: ServerBinding,  // ← Replace port + local_addr
    // ... rest unchanged
}
```

**Backward Compatibility**: Add helper methods:

```rust
impl ServerInstance {
    pub fn port(&self) -> Option<u16> {
        match &self.binding {
            ServerBinding::Port { port, .. } => Some(*port),
            ServerBinding::Interface { .. } => None,
        }
    }

    pub fn display_address(&self) -> String {
        match &self.binding {
            ServerBinding::Port { addr: Some(a), .. } => a.to_string(),
            ServerBinding::Port { port, .. } => format!("0.0.0.0:{}", port),
            ServerBinding::Interface { name } => name.clone(),
        }
    }
}
```

### 6. Client Support (Already Good!)

**File**: `src/llm/actions/common.rs`

`OpenClient` action already has flexible addressing:
```rust
OpenClient {
    protocol: String,
    remote_addr: String,  // ← Already flexible!
    // ...
}
```

**File**: `src/protocol/connect_context.rs`

`ConnectContext` already uses String:
```rust
pub struct ConnectContext {
    pub remote_addr: String,  // ← Already flexible!
    // ...
}
```

**Conclusion**: Client infrastructure already supports interface-based protocols. No changes needed.

### 7. Protocol Implementation Changes

#### ICMP Server

**File**: `src/server/icmp/actions.rs`

Update `IcmpProtocol::spawn()` implementation:

```rust
#[async_trait]
impl Server for IcmpProtocol {
    async fn spawn(&self, ctx: SpawnContext) -> Result<String> {
        // Extract interface from binding
        let interface = match ctx.binding {
            ProtocolBinding::Interface(iface) => iface,
            ProtocolBinding::Port(_) => {
                return Err(anyhow::anyhow!("ICMP protocol requires interface binding, not port"));
            }
        };

        // Call existing spawn_with_llm function
        crate::server::icmp::IcmpServer::spawn_with_llm(
            interface.clone(),
            ctx.llm_client,
            ctx.state,
            ctx.status_tx,
            ctx.server_id,
        ).await?;

        // Return interface name as server address
        Ok(interface)
    }
}
```

#### ARP Server

**File**: `src/server/arp/actions.rs`

Similar changes as ICMP (extract interface from binding, call spawn_with_llm).

#### DataLink Server

**File**: `src/server/datalink/actions.rs`

Similar changes as ICMP and ARP.

### 8. Caller Updates

All callers of `start_server_from_action()` need to pass the new parameters:

**Files to Update**:
- `src/cli/rolling_tui.rs` (line ~2589)
- `src/cli/non_interactive.rs` (line ~309)
- `src/events/handler.rs` (line ~1809)
- `src/cli/easy_startup.rs` (line ~136)

**Pattern**:

```rust
// Extract port and interface from OpenServer action
let (port, interface) = match action {
    CommonAction::OpenServer { port, interface, .. } => {
        (port, interface)
    }
    _ => unreachable!(),
};

// Call with both parameters
match server_startup::start_server_from_action(
    state,
    port,           // ← Option<u16>
    interface,      // ← Option<String>
    &base_stack,
    send_first,
    initial_memory,
    instruction.clone(),
    startup_params,
    event_handlers,
    scheduled_tasks,
    feedback_instructions,
).await {
    // ... error handling
}
```

## Implementation Plan

### Phase 1: Core Infrastructure (No Breaking Changes)

1. **Add ProtocolBinding enum** (`src/protocol/spawn_context.rs`)
   - Define `ProtocolBinding` enum (Port vs Interface)
   - Keep backward compatibility by adding `interface` field as optional initially

2. **Update SpawnContext** (`src/protocol/spawn_context.rs`)
   - Add `binding: ProtocolBinding` field
   - Temporarily keep `listen_addr` for backward compatibility
   - Mark `listen_addr` as deprecated

3. **Update Server trait return type** (`src/llm/actions/protocol_trait.rs`)
   - Change `spawn()` return from `Result<SocketAddr>` to `Result<String>`
   - Update all port-based protocols to return `addr.to_string()`

### Phase 2: Action Schema Changes

4. **Update OpenServer action** (`src/llm/actions/common.rs`)
   - Make `port` optional: `port: Option<u16>`
   - Add `interface: Option<String>` field
   - Add validation in deserialization or action handler

5. **Update start_server_from_action** (`src/cli/server_startup.rs`)
   - Accept `port: Option<u16>` and `interface: Option<String>`
   - Add validation logic (exactly one must be Some)
   - Build appropriate `ProtocolBinding` based on parameters
   - Update SpawnContext construction

### Phase 3: State Changes

6. **Update ServerInstance** (`src/state/server.rs`)
   - Replace `port` + `local_addr` with `binding: ServerBinding`
   - Add helper methods for backward compatibility
   - Update all state access code

### Phase 4: Protocol Updates

7. **Update interface-based protocols** (`src/server/{icmp,arp,datalink}/actions.rs`)
   - Implement `spawn()` with `ProtocolBinding::Interface` handling
   - Extract interface from context
   - Call existing `spawn_with_llm()` functions

8. **Update port-based protocols** (all other protocols)
   - Change return type from `SocketAddr` to `String`
   - Extract port from `ProtocolBinding::Port`
   - Return `addr.to_string()`

### Phase 5: Caller Updates

9. **Update all callers** (rolling_tui.rs, non_interactive.rs, handler.rs, easy_startup.rs)
   - Extract both `port` and `interface` from OpenServer
   - Pass both to `start_server_from_action()`

### Phase 6: Testing

10. **Test ICMP E2E**
    - Run `./test-e2e.sh icmp`
    - Verify open_server with interface works
    - Verify echo request/reply flow

11. **Test backward compatibility**
    - Run existing port-based protocol tests
    - Ensure no regressions

## Default Values Discussion

### For Interface Field

**Option 1: No default** (explicit required)
- Pro: Clear intent, no surprises
- Con: More verbose for testing
- Example: `{"interface": "lo", "base_stack": "ICMP"}`

**Option 2: Default to "lo"** (loopback)
- Pro: Convenient for testing
- Con: Production use might want different interface
- Example: `{"base_stack": "ICMP"}` → uses "lo"

**Option 3: Default to "any"** (all interfaces)
- Pro: Production-ready default
- Con: Security risk, might capture too much
- Example: `{"base_stack": "ICMP"}` → listens on all interfaces

**Option 4: Protocol-specific defaults**
- ICMP: "lo" (safe for testing)
- ARP: No default (must specify)
- DataLink: No default (must specify)

**Recommendation**: Option 4 (protocol-specific defaults) - allows each protocol to decide based on safety and common usage.

### For Port Field

**Current**: 0 = dynamic port assignment (OS chooses unused port)
- Unchanged, this works well

## Breaking Changes Summary

### Unavoidable Breaking Changes

1. **Server trait return type**: `Result<SocketAddr>` → `Result<String>`
   - All protocols must update their spawn() implementation
   - Impact: ~50 protocol implementations

2. **SpawnContext structure**: Add `binding` field
   - All protocol spawn() methods access new field
   - Impact: ~50 protocol implementations

3. **start_server_from_action signature**: Add `interface` parameter
   - All callers must pass new parameter
   - Impact: 4 call sites

4. **ServerInstance structure**: `port` + `local_addr` → `binding`
   - State access code needs updates
   - Impact: UI code, state management, possibly tests

### Mitigations

1. **Deprecation warnings**: Mark old fields as deprecated, provide migration period
2. **Helper methods**: Backward-compatible accessors (e.g., `server.port()`)
3. **Default values**: Use `#[serde(default)]` to avoid deserialization breaks
4. **Comprehensive testing**: Run full test suite after each phase

## Examples

### Port-Based Protocol (HTTP)

```json
{
  "type": "open_server",
  "port": 8080,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests"
}
```

**Or with dynamic port**:
```json
{
  "type": "open_server",
  "port": 0,
  "base_stack": "HTTP",
  "instruction": "Serve HTTP requests"
}
```

### Interface-Based Protocol (ICMP)

```json
{
  "type": "open_server",
  "interface": "lo",
  "base_stack": "ICMP",
  "instruction": "Respond to ICMP echo requests with echo replies"
}
```

### Error Cases

**Both specified**:
```json
{
  "type": "open_server",
  "port": 8080,
  "interface": "lo",
  "base_stack": "ICMP"
}
```
**Error**: "Cannot specify both port and interface for ICMP"

**Neither specified**:
```json
{
  "type": "open_server",
  "base_stack": "ICMP",
  "instruction": "..."
}
```
**Error**: "Must specify either port or interface for ICMP"

## Verification Checklist

- [ ] OpenServer action accepts optional port and interface
- [ ] Validation ensures exactly one of port/interface is provided
- [ ] SpawnContext supports both ProtocolBinding types
- [ ] Server trait returns String instead of SocketAddr
- [ ] start_server_from_action handles both binding types
- [ ] ServerInstance stores binding information correctly
- [ ] ICMP protocol spawns with interface binding
- [ ] ARP protocol spawns with interface binding
- [ ] DataLink protocol spawns with interface binding
- [ ] All port-based protocols still work (backward compatibility)
- [ ] ICMP E2E test passes
- [ ] All existing E2E tests still pass
- [ ] UI displays interface-based servers correctly
- [ ] State serialization/deserialization works with new binding

## Future Enhancements

1. **Interface validation**: Check if interface exists before spawning
2. **Interface selection UI**: Let user pick from available interfaces
3. **Multi-interface binding**: Allow protocols to bind to multiple interfaces
4. **IPv6 interface support**: Extend to ICMPv6, IPv6 interfaces
5. **Dynamic interface switching**: Detect interface changes, rebind automatically

## References

- **Current ICMP implementation**: `src/server/icmp/mod.rs`
- **Current ARP implementation**: `src/server/arp/mod.rs`
- **E2E test blocker**: `tests/server/icmp/e2e_test.rs` line 106 (open_server action)
- **Port-based spawn example**: `src/server/tcp/actions.rs`
- **Action handling flow**: `src/events/handler.rs` → `src/cli/server_startup.rs`
