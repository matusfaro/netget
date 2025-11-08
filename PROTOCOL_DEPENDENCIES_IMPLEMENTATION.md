# Protocol Dependencies Implementation

## Overview

This document describes the new Protocol trait and dependency checking system implemented for NetGet. This system allows protocols to declare runtime dependencies (system libraries, tools, privileges) and automatically excludes protocols with missing dependencies.

## Architecture

### 1. Protocol Trait

A new `Protocol` trait (`src/llm/actions/protocol_trait.rs`) serves as the base trait that both `Server` and `Client` traits extend. This provides common functionality including:

- `get_dependencies()` - Returns a list of runtime dependencies
- Common metadata methods (`protocol_name`, `stack_name`, `keywords`, `metadata`, `description`, `example_prompt`, `group_name`)
- Common action methods (`get_startup_parameters`, `get_async_actions`, `get_sync_actions`, `get_event_types`)

### 2. Dependency System

**ProtocolDependency Enum** (`src/protocol/dependencies.rs`):

```rust
pub enum ProtocolDependency {
    SystemLibrary(&'static str),      // e.g., "pcap", "smbclient", "git2"
    ToolInPath(&'static str),         // e.g., "protoc", "openssl"
    RawSocketAccess,                  // CAP_NET_RAW or root
    PrivilegedPort(u16),              // Port < 1024
    RootAccess,                       // Full root/administrator
    TunDeviceAccess,                  // CAP_NET_ADMIN or root (for VPN)
    PromiscuousMode,                  // CAP_NET_RAW or root (for packet capture)
}
```

Each dependency provides:
- `is_available(&SystemCapabilities)` - Check if dependency is met
- `name()` - Human-readable name
- `description()` - Detailed description
- `installation_hint()` - Platform-specific installation instructions

### 3. Registry Integration

**ServerRegistry** (`src/protocol/server_registry.rs`):
- `get_excluded_protocols(&caps)` - Returns map of protocol name -> missing dependencies
- `get_available_protocols(&caps)` - Returns list of available protocol names
- `is_protocol_available(name, &caps)` - Check if specific protocol is available

**ClientRegistry** (`src/protocol/client_registry.rs`):
- Same methods as ServerRegistry

### 4. UI Integration

**/dep Command** (`/deps`, `/dependencies`):
- Shows system capabilities (root, privileged ports, raw sockets)
- Lists excluded server protocols with missing dependencies
- Lists excluded client protocols with missing dependencies
- Provides installation hints for each missing dependency

**TUI Status Bar** (`src/cli/sticky_footer.rs`):
- Replaced separate "Ports<1024 denied" and "PCAP denied" indicators
- New unified indicator: `N excluded (/dep)` where N is the count
- Shows nothing if all protocols are available
- Hints user to run `/dep` command for details

## Protocol Dependency Mapping

Based on analysis of all protocol CLAUDE.md files, here are the runtime dependencies:

### System Libraries

| Library | Protocols (Server) | Protocols (Client) |
|---------|-------------------|-------------------|
| **libpcap** | ARP, DataLink | ARP, DataLink |
| **libsmbclient** | - | SMB |
| **libgit2** | - | Git |

### Privileges

| Requirement | Protocols (Server) | Protocols (Client) |
|-------------|-------------------|-------------------|
| **Root Access** | WireGuard, OpenVPN | WireGuard |
| **TUN Device** | WireGuard, OpenVPN | WireGuard |
| **Raw Sockets** | ARP, DataLink, IGMP, OSPF | ARP, DataLink, IGMP |
| **Promiscuous Mode** | ARP, DataLink | ARP, DataLink |
| **Privileged Ports** | RIP (520), Syslog (514), SSH (22), HTTP (80), DHCP (67/68) | - |

### Tools in PATH

| Tool | Protocols | Purpose |
|------|-----------|---------|
| **protoc** | gRPC (server/client), etcd (client) | .proto file support (gRPC runtime, etcd build-time) |

## Implementation Status

### Completed

- ✅ Protocol trait with `get_dependencies()` method
- ✅ ProtocolDependency enum with availability checking
- ✅ Dependency checking in ServerRegistry and ClientRegistry
- ✅ `/dep` slash command to show excluded protocols
- ✅ TUI status bar showing unified dependency status
- ✅ System library checking (ldconfig, pkg-config, dyld)
- ✅ Tool checking (which/where command)
- ✅ Linux capability checking (CAP_NET_RAW, CAP_NET_ADMIN)

### Pending

- ⏳ Update individual protocols to return their dependencies
- ⏳ Filter LLM prompts to exclude unavailable protocols
- ⏳ Show dependency warnings when user tries to use excluded protocol

## How to Add Dependencies to a Protocol

### Example: ARP Server

```rust
// In src/server/arp/actions.rs

use crate::protocol::dependencies::ProtocolDependency;
use crate::llm::actions::Protocol;

impl Protocol for ArpProtocol {
    // ... other trait methods ...

    fn get_dependencies(&self) -> Vec<ProtocolDependency> {
        vec![
            ProtocolDependency::SystemLibrary("pcap"),
            ProtocolDependency::RawSocketAccess,
            ProtocolDependency::PromiscuousMode,
        ]
    }
}
```

### Example: WireGuard Server

```rust
impl Protocol for WireguardProtocol {
    fn get_dependencies(&self) -> Vec<ProtocolDependency> {
        vec![
            ProtocolDependency::TunDeviceAccess,
            ProtocolDependency::RootAccess,
        ]
    }
}
```

### Example: SSH Server (on port 22)

```rust
impl Protocol for SshProtocol {
    fn get_dependencies(&self) -> Vec<ProtocolDependency> {
        vec![
            ProtocolDependency::PrivilegedPort(22),
        ]
    }
}
```

## Testing

To test the dependency system:

1. **Without root/capabilities:**
   ```bash
   ./netget
   # In TUI, type: /dep
   # Should show excluded protocols (ARP, DataLink, etc.)
   ```

2. **With CAP_NET_RAW:**
   ```bash
   sudo setcap cap_net_raw+ep ./target/release/netget
   ./target/release/netget
   # /dep should show fewer exclusions
   ```

3. **As root:**
   ```bash
   sudo ./netget
   # /dep should show only protocols missing system libraries/tools
   ```

## Future Enhancements

1. **LLM Prompt Filtering**: Exclude unavailable protocols from `open_server` action documentation
2. **Runtime Warnings**: When user tries to use excluded protocol, show friendly error with /dep hint
3. **Conditional Dependencies**: Some protocols have optional dependencies (e.g., gRPC protoc only for .proto files)
4. **Dependency Installation**: Integrate with package managers to suggest installation commands
5. **Docker Support**: Special handling for containerized environments

## Files Modified

1. `src/llm/actions/protocol_trait.rs` - New Protocol trait, Server/Client extend it
2. `src/llm/actions/client_trait.rs` - Updated to extend Protocol
3. `src/llm/actions/mod.rs` - Export Protocol trait
4. `src/protocol/dependencies.rs` - NEW: ProtocolDependency enum and checking logic
5. `src/protocol/mod.rs` - Export dependencies module
6. `src/protocol/server_registry.rs` - Added dependency checking methods
7. `src/protocol/client_registry.rs` - Added dependency checking methods
8. `src/events/types.rs` - Added ShowDependencies command
9. `src/events/handler.rs` - Added handle_show_dependencies() method
10. `src/cli/sticky_footer.rs` - Updated status bar with unified dependency indicator

## References

- Research: Comprehensive analysis of 80+ protocol CLAUDE.md files
- Privilege System: Existing `src/privilege.rs` for capability detection
- Metadata System: Existing `src/protocol/metadata.rs` for protocol information
