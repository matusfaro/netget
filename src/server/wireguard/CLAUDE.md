# WireGuard VPN Server Implementation

## Overview

A WireGuard VPN server that stands up a real tunnel by delegating **entirely** to `defguard_wireguard_rs`. This is the
closest thing NetGet has to a working VPN - OpenVPN (`src/server/openvpn/`) has a stubbed control channel and is marked
`Incomplete`, and IPSec (`src/server/ipsec/`) is a parse-and-log honeypot - but read the honesty note below before
treating it as a reference implementation.

**Status**: `DevelopmentState::Beta` (demoted from Stable - see below)
**Privileges**: `PrivilegeRequirement::Root` (interface creation needs root / CAP_NET_ADMIN, and on macOS the external
`wireguard-go` binary in PATH)
**Protocol Spec**: [WireGuard White Paper](https://www.wireguard.com/papers/wireguard.pdf)
**Port**: UDP 51820 (default)

### Honesty note: this is a thin wrapper, and Beta not Stable

NetGet implements **none** of the WireGuard protocol itself - no Noise_IK handshake, no ChaCha20-Poly1305, no packet
parsing. All of it lives in the platform backend that `defguard_wireguard_rs` drives: the kernel module on
Linux/FreeBSD/Windows, and the **external `wireguard-go` binary on macOS** (`wgapi_userspace.rs` does
`Command::new("wireguard-go")` and talks to it over `/var/run/wireguard/<iface>.sock`). NetGet only generates a
keypair, configures the interface, polls it every 5s, and applies LLM peer actions.

It was demoted from `Stable` to `Beta` because the project's rule for `Stable` is "real spec compliance, **validated
against a real client**", and this protocol has **never** been validated end-to-end against any real WireGuard client.
It cannot be, in CI or unprivileged: creating the interface needs root, and on macOS also `wireguard-go` installed.
That is the same defect that got `tor_relay` and `openvpn` demoted - claiming a rating the evidence doesn't support.

**Known design bug (not yet fixed): the reactive authorization model is backwards for WireGuard.** A WireGuard
responder decrypts the initiator's static public key from the handshake and **drops the handshake if that key is not
already a configured peer**. But NetGet only learns of a peer by polling `read_interface_data()` *after* it appears -
and an unconfigured peer never appears. There is also no user-triggered action to pre-add a peer
(`get_async_actions()` is empty). So `wireguard_peer_connected` can effectively never fire for a genuinely new peer,
and the entire LLM authorize/reject flow is unreachable in practice; it only works for peers configured out-of-band.
Earning `Stable` requires fixing this (accept the peer's public key ahead of the handshake, e.g. via a startup
parameter or an "expected peers" list) and then proving a real client completes a handshake and exchanges a transport
packet. See `tests/server/wireguard/`.

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

### Event flow

The monitoring loop (`monitor_peers`) polls `read_interface_data()` every 5 seconds. The first time a peer appears it:

1. Registers a `ConnectionState` in `AppState`
2. Builds a `wireguard_peer_connected` `Event`
3. Calls `crate::llm::action_helper::call_llm(...)`

`call_llm` tries any configured script/static event handler first and only performs a real LLM call when none is
configured, so a scripted WireGuard server costs zero model calls. Exactly one event is raised per peer connect (not
per poll), so LLM traffic is bounded by peer churn.

The returned `raw_actions` are applied to the live interface in `handle_peer_connected`.

### Async Actions (User-triggered)

**None.** `get_async_actions()` returns an empty list.

`list_peers` / `remove_peer` / `get_server_info` were previously advertised, but the action executor calls
`Server::execute_action()` on a *stateless* `WireguardProtocol` struct that holds no handle to the running
`WireguardServer`, so those actions could never return peer data or reconfigure the interface. They are still accepted
by `execute_action` as no-ops for backwards compatibility but are no longer offered to the LLM. Re-enabling them needs
a server-instance registry in `llm/actions/protocol_trait.rs` or `state/`, which is outside this module.

### Sync Actions (raised by `wireguard_peer_connected`)

1. **authorize_peer**: (re)configure the peer with specific allowed IPs via `wgapi.configure_peer()`
    - Parameters: `public_key` (required), `allowed_ips` (required, CIDR list), `endpoint`, `message`
    - Defaults to the event's own public key / allowed IPs if omitted
2. **reject_peer**: remove the peer from the interface (same effect as `disconnect_peer`)
3. **set_peer_traffic_limit**: **NOT ENFORCED** - logged only. No tc/iptables configuration is performed.
4. **disconnect_peer**: remove the peer from the interface, tearing down its tunnel

### Event Types

- `wireguard_peer_connected`: peer completed its handshake and appeared on the interface

Data fields: `public_key`, `endpoint`, `allowed_ips`, `server_public_key`, `listen_port`.

**Important**: WireGuard performs its Noise handshake inside the kernel (or wireguard-go) backend. NetGet observes
peers *after* the handshake succeeds and therefore **cannot gate a handshake before it completes**. `authorize_peer`
sets post-connection policy; `disconnect_peer`/`reject_peer` remove a peer that already got on. A `wireguard_peer_request`
event was previously declared but never emitted by any code path - it has been removed rather than left as a false promise.

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

- **Max 100 peers**: Hard-coded limit, enforced in `add_peer` only
- **No traffic limiting**: `set_peer_traffic_limit` is logged but never enforced
- **No QoS**: All peers treated equally
- **Up to 5s detection latency**: peers are discovered by polling, not by handshake callback

### Task Lifecycle

The monitoring task's `JoinHandle` is registered with `AppState::register_server_task()`, so `stop_server` aborts it.
Note that `register_server_task` stores a *single* handle per server - a protocol that needs several long-lived loops
must combine them into one task (e.g. `tokio::select!`) rather than calling it twice.

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
