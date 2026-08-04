# OpenVPN Server Implementation — INCOMPLETE AND INSECURE

## ⚠️ Read this first

**This is not a working OpenVPN server and must not be used to carry real traffic.**

It implements the OpenVPN *packet format* and a real TUN data path, but the security-critical half of the protocol is
absent:

| Component                        | Status                                                                       |
|----------------------------------|------------------------------------------------------------------------------|
| TLS control channel              | ❌ **Does not exist.** `create_tls_config()` builds a config that is never used |
| Peer authentication              | ❌ **None.** Any host sending a HARD_RESET is accepted                          |
| Client certificate verification  | ❌ None                                                                        |
| Data-channel key derivation      | ❌ **Hardcoded constants** — every peer derives the same key                    |
| `handle_control_message()`       | ❌ Empty stub                                                                  |
| `handle_ack_packet()`            | ❌ Empty stub                                                                  |
| Packet parse/serialize (V1/V2)   | ✅ Implemented                                                                 |
| TUN interface + IP pool          | ✅ Implemented                                                                 |
| AES-256-GCM / ChaCha20-Poly1305  | ✅ Correct AEAD calls — operating on worthless keys                            |

### The key problem, specifically

`initialize_data_channel()` (`mod.rs`) derives every peer's cipher key with HKDF over three fixed literals:

```rust
let master_secret = b"simplified_master_secret_for_mvp";
let client_random = b"client_random_data_12345678";
let server_random = b"server_random_data_87654321";
```

There is no per-session entropy. **Every peer, on every run, on every installation derives the identical key**, and
the inputs are in this repository and its public git history. Anyone who can read the source can decrypt the tunnel.
The encryption is decorative.

A real OpenVPN server derives these keys from the TLS master secret negotiated during the control-channel handshake.
No handshake happens here, so there is no master secret to derive from.

**Status**: `DevelopmentState::Incomplete` — hidden from the LLM by `ProtocolMetadataV2::is_available_to_llm()`
**Privileges**: `PrivilegeRequirement::Root` (TUN device creation)
**Use instead**: WireGuard (`src/server/wireguard/`), NetGet's only functional VPN

### Why it is kept

As a packet-format testbed and as a honeypot that speaks plausible-looking OpenVPN to a scanner. Not as a VPN.

## Library Choices

Custom implementation — no viable Rust OpenVPN *server* library exists (the reference C++ implementation is 500k+
LOC; `openvpn-parser` is read-only and unmaintained; `libopenvpn3` FFI is client-only).

Dependencies used:

- `tun` v0.7 — TUN interface creation
- `aes-gcm` v0.10, `chacha20poly1305` v0.10 — data-channel AEAD
- `rustls` + `rcgen` — certificate generation (**generated, then unused**)
- `hkdf` + `sha2` — key derivation (**over constants, see above**)

## Architecture

### TUN interface

Platform-specific naming: `netget_ovpn0` (Linux/Windows), `utun11` (macOS). Server takes `10.8.0.1` on `10.8.0.0/24`;
clients get `10.8.0.2`–`10.8.0.254` from `IpPool`.

The device is **split** with `tokio::io::split()`. The read loop owns the `ReadHalf`; the write side is an
`Arc<Mutex<WriteHalf>>`. This matters: an earlier version held a single `RwLock<AsyncDevice>` write guard while parked
in `read()`, so no decrypted client packet could ever be written to the TUN while the TUN was idle — a deadlock that
silently broke the client-to-network direction. Per the project rule: never hold a lock across I/O.

### Task lifecycle

`AppState::register_server_task()` stores exactly **one** handle per server, so the UDP listener and the TUN reader are
run inside a single task joined by `tokio::select!`. Registering them as two tasks would silently drop the first
handle and leak that loop past `stop_server`. Aborting the one registered handle cancels both loops.

### Connection flow (what actually happens)

```
Client                                    Server
  ├──── HARD_RESET_CLIENT_V2 ─────────────>│
  │                                        ├─ (no authentication of any kind)
  │                                        ├─ Allocate VPN IP
  │                                        ├─ Initialize cipher from CONSTANTS
  │<─────── HARD_RESET_SERVER_V2 ──────────┤   (empty TLS payload)
  │                                        ├─ raise openvpn_peer_connected
  ├──── DATA_V2 ──────────────────────────>│
  │                                        ├─ Decrypt, write to TUN
```

A real `openvpn` client will not interoperate: it expects a TLS session inside the control channel and gets an empty
payload, so it never reaches the data phase.

## LLM Integration

### Event flow

`openvpn_peer_connected` is raised from `handle_handshake_initiation` via `crate::llm::action_helper::call_llm`,
which runs any configured script/static handler first and only falls back to a real model call when none is set.

Because the protocol is `Incomplete`, it is **not offered to the LLM** during server selection; it can still be
started explicitly (e.g. tests using `with_include_disabled_protocols`).

### Event Types

- `openvpn_peer_connected` — a peer was accepted and assigned a VPN IP

Data fields: `peer_addr`, `vpn_ip`, `session_id`, `authenticated` (always `false`).

An `openvpn_peer_request` authorization event was previously declared but **never emitted** — peers are auto-accepted
inside `handle_handshake_initiation` — so it has been removed rather than left as a false promise of pre-connection
authorization.

### Sync Actions

All are **observation/logging hooks**. None gates a connection, because the peer is already accepted, addressed and
keyed before the event fires.

1. **authorize_peer** — NOT ENFORCED; records that a peer is considered authorized
2. **reject_peer** — NOT ENFORCED; nothing tears the peer down
3. **set_peer_limit** — NOT ENFORCED; no traffic shaping is configured
4. **inspect_traffic** — flags the peer's traffic for logging

### Async Actions

**None.** `get_async_actions()` returns an empty list.

`list_peers` / `remove_peer` / `get_server_info` were previously advertised, but the action executor calls
`Server::execute_action()` on a stateless `OpenvpnProtocol` struct with no handle to the running `OpenvpnServer`, so
they returned `NoAction` unconditionally. They remain accepted by `execute_action` for backwards compatibility.

## Peer Management

- `PeerState`: WaitingForHandshake → TlsHandshaking → KeyExchange → Connected → Disconnecting
  (in practice peers jump straight to Connected)
- Max 100 peers
- Packet IDs tracked per peer for basic replay rejection; `received_packet_ids` grows without bound for long-lived
  peers (no window eviction)

## Storage

None, per NetGet's protocol rules. Peer state is in-memory runtime state only — no database, no filesystem, no
persistence.

## Future Work (large — not a patch)

Making this a real OpenVPN server requires the whole control channel:

1. Wrap a genuine TLS 1.3 session over the OpenVPN reliability layer
2. Implement control-channel retransmission, ACK windows and sequencing
3. Exchange key material through the TLS session
4. Derive per-session keys from the negotiated master secret via the OpenVPN PRF (replacing the constants)
5. Verify client certificates
6. Implement configuration push/pull, then compression and TCP transport

Steps 1–4 are the minimum for the protocol to stop being insecure. This is a project in its own right and is
deliberately out of scope.

## Testing

`tests/server/openvpn/e2e_test.rs` requires the `openvpn` binary and root, and skips otherwise. It cannot verify a
successful tunnel — a real client cannot complete a handshake against a server with no control channel.

## References

- [OpenVPN Protocol Documentation](https://openvpn.net/community-resources/openvpn-protocol/)
- [OpenVPN Source Code](https://github.com/OpenVPN/openvpn)
- [AES-GCM](https://datatracker.ietf.org/doc/html/rfc5116)
- [ChaCha20-Poly1305](https://datatracker.ietf.org/doc/html/rfc8439)
- [HKDF](https://datatracker.ietf.org/doc/html/rfc5869)
