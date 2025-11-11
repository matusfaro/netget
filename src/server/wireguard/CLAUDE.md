# WireGuard VPN Server Implementation

## Overview

Full-featured WireGuard VPN server implementing the WireGuard protocol with actual tunnel support. This is NetGet's *
*only fully-functional VPN protocol** - it creates real TUN interfaces and establishes encrypted tunnels for clients.

**Status**: Production-ready, fully implemented
**Protocol Spec**: [WireGuard White Paper](https://www.wireguard.com/papers/wireguard.pdf)
**Port**: UDP 51820 (default)

## Library Choices

### defguard_wireguard_rs v0.7

**Why chosen**:

- Multi-platform unified API (Linux kernel, macOS userspace, Windows kernel, FreeBSD kernel)
- Production-ready Rust library with active maintenance
- Handles all crypto (Curve25519, ChaCha20Poly1305, BLAKE2s)
- Automatic TUN interface creation and management
- Built-in peer monitoring and statistics

**What it provides**:

- `WGApi` - Platform-specific WireGuard API (Kernel on Linux/FreeBSD/Windows, Userspace on macOS)
- `Key` - Curve25519 keypair generation and management
- `Peer` - Peer configuration with allowed IPs, endpoints, keepalive
- `InterfaceConfiguration` - Interface setup with addresses, port, MTU
- `read_interface_data()` - Peer connection status and statistics

**Why not alternatives**:

- `boringtun` - Userspace only, more complex integration
- Manual crypto - WireGuard crypto is complex, library handles it correctly
- Native CLI (`wg`, `wg-quick`) - Violates NetGet architecture (external dependencies)

## Architecture Decisions

### TUN Interface Creation

Platform-specific interface naming:

- **Linux/FreeBSD**: `netget_wg0` (kernel WireGuard)
- **macOS**: `utun10` (wireguard-go userspace)
- **Windows**: `netget_wg0` (kernel WireGuard)

Server assigns itself `10.20.30.1` on the VPN subnet `10.20.30.0/24`.

### Peer Monitoring Loop

Spawns async task that polls `read_interface_data()` every 5 seconds:

- Detects new peer connections (peers appear when handshake succeeds)
- Updates connection stats (bytes sent/received, last handshake time, endpoint)
- Tracks peer disconnections (peers disappear from interface data)
- Integrates with NetGet's connection tracking UI

### Keypair Management

Server generates Curve25519 keypair on startup:

```rust
let private_key = Key::generate();
let public_key = private_key.public_key();
```

Public key displayed to user for client configuration. Peers authenticate with their own public keys.

### State Machine

WireGuard handles its own state machine internally. NetGet tracks:

- **Peer tracking**: `HashMap<String, ConnectionId>` mapping public keys to connection IDs
- **Connection state**: Active when peer appears in interface data, Closed when removed
- **Max peers**: 100 peer limit to prevent resource exhaustion

## LLM Integration

### Async Actions (User-triggered)

Available anytime, no network context required:

1. **list_peers**: View all connected peers
2. **remove_peer**: Permanently remove peer from configuration
3. **get_server_info**: View server public key and config

### Sync Actions (Network event triggered)

Require peer connection context:

1. **authorize_peer**: Allow peer to connect with specific allowed IPs
    - Parameters: `peer_public_key`, `allowed_ips` (e.g., `["10.20.30.2/32"]`)
    - Creates peer configuration via `wgapi.configure_peer()`
2. **reject_peer**: Deny peer connection request
3. **set_peer_traffic_limit**: Configure bandwidth/data limits (placeholder)
4. **disconnect_peer**: Immediately disconnect a peer

### Event Types

- `wireguard_peer_request`: Peer requesting authorization (future feature)
- `wireguard_peer_connected`: Peer successfully connected

**Note**: Current implementation auto-detects peers via monitoring loop. Future versions may require explicit LLM
authorization before peer handshake completes.

## Connection Management

### Peer Detection

Peers detected when they appear in `interface_data.peers` after successful handshake:

```rust
for (pub_key, peer) in interface_data.peers.iter() {
    if !peers.contains_key(&peer_key) {
        // New peer - add to tracking
        let connection_id = ConnectionId::new();
        peers.insert(peer_key.clone(), connection_id);

        // Add to server state with stats
        app_state.add_connection_to_server(server_id, conn_state).await;
    }
}
```

### Stats Tracking

Each peer connection tracked with:

- `bytes_sent`: Total transmitted bytes (from `peer.tx_bytes`)
- `bytes_received`: Total received bytes (from `peer.rx_bytes`)
- `last_handshake`: Timestamp of last handshake
- `endpoint`: Client's UDP endpoint (IP:port)
- `allowed_ips`: VPN IP addresses assigned to peer

### Cleanup

Disconnected peers removed when they disappear from interface data:

```rust
for peer_key in disconnected_peers {
    if let Some(connection_id) = peers.remove(&peer_key) {
        app_state.close_connection_on_server(server_id, connection_id).await;
    }
}
```

## State Management

### Server State

```rust
pub struct WireguardServer {
    _interface_name: String,
    wgapi: Arc<RwLock<WGApi<Backend>>>,  // Kernel or Userspace based on OS
    _private_key: String,
    public_key: String,
    listen_port: u16,
    peers: Arc<RwLock<HashMap<String, ConnectionId>>>,
}
```

### Protocol Connection Info

```rust
ProtocolConnectionInfo::Wireguard {
    public_key: String,           // Peer's public key
    endpoint: Option<String>,     // Client UDP endpoint
    allowed_ips: Vec<String>,     // VPN IPs assigned to peer
    last_handshake: Option<SystemTime>,  // Last successful handshake
}
```

## Limitations

### Requires Elevated Privileges

- **Linux/FreeBSD**: Root or `CAP_NET_ADMIN` capability
- **macOS**: Requires wireguard-go userspace (automatically used)
- **Windows**: Administrator privileges

### Network Configuration

- **No automatic routing**: Server doesn't configure IP forwarding or NAT
- **Static subnet**: Always uses `10.20.30.0/24`
- **No IPv6**: Currently IPv4-only
- **No dynamic IP assignment**: LLM must manually assign IPs via `allowed_ips`

### Platform-Specific Behaviors

- **macOS**: Uses userspace wireguard-go (slower but works without kernel module)
- **Linux**: Uses kernel WireGuard (fastest, requires kernel 5.6+ or module)
- **Windows**: Uses kernel WireGuard (requires WireGuard driver installed)

### Peer Management

- **Max 100 peers**: Hard-coded limit to prevent resource exhaustion
- **No traffic limiting**: `set_peer_traffic_limit` action is placeholder
- **No QoS**: All peers treated equally

## Examples

### Server Startup

```
netget> Start a WireGuard VPN server on port 51820
```

LLM response:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Starting WireGuard VPN server on UDP port 51820. Generating keypair and creating TUN interface..."
    }
  ]
}
```

Server output:

```
[INFO] Starting WireGuard VPN server on 0.0.0.0:51820 (full VPN tunnel support)
[INFO] Server public key: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
[INFO] Creating interface: netget_wg0
[INFO] Interface created successfully
[INFO] Interface listening on UDP port 51820
[INFO] VPN subnet: 10.20.30.0/24
→ WireGuard VPN server ready on 0.0.0.0:51820
[INFO] Clients can connect using server public key: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
```

### Peer Connection

When peer connects, monitoring loop detects it:

```
[INFO] New peer: xTIBA5rboUvnH4hto...
```

LLM can authorize with allowed IPs:

```json
{
  "actions": [
    {
      "type": "authorize_peer",
      "peer_public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
      "allowed_ips": ["10.20.30.2/32"]
    },
    {
      "type": "show_message",
      "message": "Peer authorized with VPN IP 10.20.30.2"
    }
  ]
}
```

### Client Configuration

Clients configure using server's public key:

```ini
[Interface]
PrivateKey = <client_private_key>
Address = 10.20.30.2/32

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = <server_ip>:51820
AllowedIPs = 0.0.0.0/0  # Route all traffic through VPN
```

## References

- [WireGuard White Paper](https://www.wireguard.com/papers/wireguard.pdf)
- [defguard_wireguard_rs Documentation](https://docs.rs/defguard_wireguard_rs/)
- [WireGuard Protocol Spec](https://www.wireguard.com/protocol/)
- [Curve25519](https://cr.yp.to/ecdh.html)
- [ChaCha20-Poly1305](https://datatracker.ietf.org/doc/html/rfc8439)
