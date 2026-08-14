//! WireGuard VPN server implementation with LLM control
//!
//! This is a FULL WireGuard VPN server that creates real tunnels for clients. It
//! uses `defguard_wireguard_rs` for cross-platform WireGuard support (kernel
//! WireGuard on Linux/FreeBSD/Windows, userspace wireguard-go on macOS). All
//! cryptography and the tunnel data plane are handled by WireGuard itself, so
//! this is the one VPN protocol in NetGet that is genuinely functional.
//!
//! # What the LLM actually controls
//!
//! WireGuard performs its handshake in the kernel/userspace backend, so NetGet
//! cannot gate the handshake crypto itself. Control has two levers:
//!
//! **Up front (user-triggered):** `wireguard_add_peer` pre-authorizes a peer's
//! public key + allowed IPs on the live interface *before* it connects. A WireGuard
//! responder drops a handshake whose static key is not already configured, so this
//! is what makes a new peer's handshake succeed and the authorize decision
//! reachable at all. Dispatched via [`crate::llm::actions::protocol_trait::Server::execute_action_with_state`]
//! against the [`WireguardServer`] handle registered in `spawn`.
//!
//! **Post-handshake (event-driven):** a peer monitoring loop polls the interface
//! every 5 seconds and, when a *new* peer appears, raises a
//! `wireguard_peer_connected` event. That event is routed through
//! [`crate::llm::action_helper::call_llm`], which first tries any configured
//! script/static event handler and only falls back to a real LLM call when none
//! is configured. The resulting actions are applied to the live interface:
//!
//! - `authorize_peer` -> peer is (re)configured with the given `allowed_ips`
//! - `disconnect_peer` / `reject_peer` -> peer is removed from the interface
//!
//! Fail-closed throughout: with no `wireguard_add_peer` call, an unknown peer's
//! handshake is dropped by the backend and it never appears.
//!
//! # Limitations
//!
//! - Peers are observed *after* their handshake succeeds; the LLM cannot block a
//!   handshake before it completes (but it can refuse to pre-add the key, which
//!   *does* prevent the handshake from ever succeeding).
//! - `set_peer_traffic_limit` is recorded but NOT enforced (no tc/iptables setup).
//! - Requires root / CAP_NET_ADMIN to create the interface.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use actions::{WireguardProtocol, WIREGUARD_PEER_CONNECTED_EVENT};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, trace, warn};

/// Truncate a key for display without panicking on short or non-ASCII input.
///
/// `&key[..16]` panics when the string is shorter than 16 bytes or when byte 16
/// is not a char boundary. Public keys arriving from LLM output are arbitrary
/// strings, so this must never index blindly.
fn short_key(key: &str) -> &str {
    match key.char_indices().nth(16) {
        Some((idx, _)) => &key[..idx],
        None => key,
    }
}

use defguard_wireguard_rs::{
    host::Peer as WGPeer, key::Key, net::IpAddrMask, InterfaceConfiguration, WGApi,
    WireguardInterfaceApi,
};

/// Maximum number of peers to allow
const MAX_PEERS: usize = 100;

/// WireGuard server state
pub struct WireguardServer {
    /// Interface name
    _interface_name: String,
    /// WireGuard API instance
    #[cfg(not(target_os = "macos"))]
    wgapi: Arc<RwLock<WGApi<defguard_wireguard_rs::Kernel>>>,
    #[cfg(target_os = "macos")]
    wgapi: Arc<RwLock<WGApi<defguard_wireguard_rs::Userspace>>>,
    /// Server private key
    _private_key: String,
    /// Server public key
    public_key: String,
    /// Listen port
    listen_port: u16,
    /// Peer tracking: public_key -> ConnectionId
    peers: Arc<RwLock<HashMap<String, ConnectionId>>>,
    /// LLM client used to raise peer events (script/static handlers take priority)
    llm_client: Arc<OllamaClient>,
    /// Status stream used for dual logging. Stored on the struct so live-instance
    /// actions (reached via `execute_action_with_state`) can log to the same TUI/MCP
    /// stream as the monitoring loop without having a channel threaded in.
    status_tx: mpsc::UnboundedSender<String>,
}

/// Build a validated `WGPeer` from action inputs.
///
/// This is the config-mutation logic for pre-adding / (re)configuring a peer, split
/// out as a pure function so it can be unit-tested without a running WireGuard
/// backend (creating the interface needs root, and on macOS the external
/// `wireguard-go` binary). It is the single validation point shared by the reactive
/// `authorize_peer` path and the new user-triggered `wireguard_add_peer` action.
///
/// Fail-closed: a malformed public key, an unparyseable allowed-IP, or an empty
/// allowed-IP list is an `Err`, so nothing half-valid is ever pushed to
/// `configure_peer`. It never fabricates a peer for missing input.
pub fn build_peer_config(
    public_key: &str,
    allowed_ips: &[String],
    endpoint: Option<SocketAddr>,
) -> Result<WGPeer> {
    let peer_key: Key = public_key
        .parse()
        .with_context(|| format!("Invalid peer public key: {}", short_key(public_key)))?;

    if allowed_ips.is_empty() {
        return Err(anyhow::anyhow!(
            "allowed_ips must not be empty - a peer with no allowed IPs can route nothing"
        ));
    }

    // Reject the whole request if any allowed-IP is malformed rather than silently
    // dropping it: a peer configured with fewer routes than asked for is a policy
    // surprise, and the caller should see the error and fix it.
    let mut allowed_ip_masks: Vec<IpAddrMask> = Vec::with_capacity(allowed_ips.len());
    for ip_str in allowed_ips {
        let mask = ip_str
            .parse::<IpAddrMask>()
            .with_context(|| format!("Invalid allowed IP (expected CIDR, e.g. 10.20.30.2/32): {ip_str}"))?;
        allowed_ip_masks.push(mask);
    }

    let mut peer = WGPeer::new(peer_key);
    peer.allowed_ips = allowed_ip_masks;
    if let Some(ep) = endpoint {
        peer.endpoint = Some(ep);
    }

    Ok(peer)
}

impl WireguardServer {
    /// Spawn WireGuard VPN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        bind_addr: SocketAddr,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        info!("Starting WireGuard VPN server on {}", bind_addr);
        let _ = status_tx.send(format!(
            "[INFO] Starting WireGuard VPN server on {} (full VPN tunnel support)",
            bind_addr
        ));

        // Generate server keypair
        let private_key = Key::generate();
        let public_key = private_key.public_key();

        let private_key_str = private_key.to_string();
        let public_key_str = public_key.to_string();

        info!("WireGuard server public key: {}", public_key_str);
        let _ = status_tx.send(format!("[INFO] Server public key: {}", public_key_str));

        // Determine interface name based on OS
        let interface_name: String = if cfg!(target_os = "linux") || cfg!(target_os = "freebsd") {
            "netget_wg0".into()
        } else if cfg!(target_os = "macos") {
            "utun10".into()
        } else if cfg!(target_os = "windows") {
            "netget_wg0".into()
        } else {
            return Err(anyhow::anyhow!(
                "Unsupported operating system for WireGuard"
            ));
        };

        info!("Creating WireGuard interface: {}", interface_name);
        let _ = status_tx.send(format!("[INFO] Creating interface: {}", interface_name));

        // Create WGApi instance
        #[cfg(not(target_os = "macos"))]
        let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(interface_name.clone())
            .context("Failed to create WireGuard API")?;

        #[cfg(target_os = "macos")]
        let wgapi = WGApi::<defguard_wireguard_rs::Userspace>::new(interface_name.clone())
            .context("Failed to create WireGuard API")?;

        // Create interface
        wgapi
            .create_interface()
            .context("Failed to create WireGuard interface")?;

        info!("WireGuard interface created successfully");
        let _ = status_tx.send("[INFO] Interface created successfully".to_string());

        // Configure interface
        let listen_port = bind_addr.port();
        let interface_config = InterfaceConfiguration {
            name: interface_name.clone(),
            prvkey: private_key_str.clone(),
            addresses: vec!["10.20.30.1".parse().unwrap()],
            port: listen_port as u32,
            peers: vec![],
            mtu: Some(1420),
        };

        #[cfg(not(windows))]
        wgapi
            .configure_interface(&interface_config)
            .context("Failed to configure WireGuard interface")?;

        #[cfg(windows)]
        wgapi
            .configure_interface(&interface_config, &[])
            .context("Failed to configure WireGuard interface")?;

        info!("WireGuard interface configured on port {}", listen_port);
        let _ = status_tx.send(format!(
            "[INFO] Interface listening on UDP port {}",
            listen_port
        ));
        let _ = status_tx.send(format!("[INFO] VPN subnet: 10.20.30.0/24"));

        let actual_addr = SocketAddr::new(bind_addr.ip(), listen_port);

        let wgapi_arc = Arc::new(RwLock::new(wgapi));
        let peers = Arc::new(RwLock::new(HashMap::new()));

        let server = Arc::new(WireguardServer {
            _interface_name: interface_name.clone(),
            wgapi: wgapi_arc.clone(),
            _private_key: private_key_str,
            public_key: public_key_str.clone(),
            listen_port,
            peers: peers.clone(),
            llm_client,
            status_tx: status_tx.clone(),
        });

        // Register the live server instance so user-triggered actions
        // (wireguard_add_peer) can reach it via `AppState::server_handle`. Without
        // this the action executor only ever sees the stateless protocol struct and
        // could not touch the interface. Dropped automatically when the server is
        // removed, so it can never outlive the interface it points at.
        app_state
            .register_server_handle(server_id, server.clone())
            .await;

        // Spawn monitoring task to track peer connections
        let server_clone = server.clone();
        let status_clone = status_tx.clone();
        let app_state_clone = app_state.clone();
        let monitor_handle = tokio::spawn(async move {
            server_clone
                .monitor_peers(app_state_clone, server_id, status_clone)
                .await;
        });

        // Register the monitoring loop so stop_server can abort it. Without this
        // the task outlives the server and keeps polling a dead interface.
        app_state
            .register_server_task(server_id, monitor_handle)
            .await;

        info!("WireGuard VPN server ready on {}", actual_addr);
        let _ = status_tx.send(format!("→ WireGuard VPN server ready on {}", actual_addr));
        let _ = status_tx.send(format!(
            "[INFO] Clients can connect using server public key: {}",
            public_key_str
        ));

        Ok(actual_addr)
    }

    /// Monitor peer connections and update state
    async fn monitor_peers(
        &self,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            interval.tick().await;

            // Read interface data
            let interface_data = {
                let wgapi = self.wgapi.read().await;
                match wgapi.read_interface_data() {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to read WireGuard interface data: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to read interface: {}", e));
                        continue;
                    }
                }
            };

            trace!(
                "WireGuard interface status: {} peers",
                interface_data.peers.len()
            );

            // Track new peers and update existing.
            //
            // IMPORTANT: never hold `self.peers` across an await that can call the
            // LLM or touch the WireGuard API - `add_peer`/`remove_peer` take the
            // same locks and would deadlock.
            let mut newly_connected = Vec::new();

            for (pub_key, peer) in interface_data.peers.iter() {
                let peer_key = pub_key.to_string();

                let existing = {
                    let peers = self.peers.read().await;
                    peers.get(&peer_key).copied()
                };

                match existing {
                    None => {
                        // New peer discovered
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        {
                            let mut peers = self.peers.write().await;
                            peers.insert(peer_key.clone(), connection_id);
                        }

                        info!("New WireGuard peer connected: {}", peer_key);
                        let _ =
                            status_tx.send(format!("[INFO] New peer: {}", short_key(&peer_key)));

                        // Determine endpoint
                        let remote_addr = peer.endpoint;

                        // Add connection to server state
                        let now = std::time::Instant::now();
                        let conn_state = ConnectionState {
                            id: connection_id,
                            remote_addr: remote_addr
                                .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0))),
                            local_addr: SocketAddr::new(
                                std::net::IpAddr::from([10, 20, 30, 1]),
                                self.listen_port,
                            ),
                            bytes_sent: peer.tx_bytes,
                            bytes_received: peer.rx_bytes,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };

                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        newly_connected.push((
                            peer_key,
                            connection_id,
                            remote_addr,
                            peer.allowed_ips
                                .iter()
                                .map(|ip| ip.to_string())
                                .collect::<Vec<_>>(),
                        ));
                    }
                    Some(connection_id) => {
                        // Update existing peer stats
                        app_state
                            .update_connection_stats(
                                server_id,
                                connection_id,
                                Some(peer.rx_bytes),
                                Some(peer.tx_bytes),
                                None,
                                None,
                            )
                            .await;
                    }
                }
            }

            // Raise a peer-connected event for each new peer. This routes through
            // script/static event handlers first and only falls back to a real LLM
            // call when no handler is configured.
            for (peer_key, connection_id, endpoint, allowed_ips) in newly_connected {
                self.handle_peer_connected(
                    &app_state,
                    server_id,
                    connection_id,
                    &peer_key,
                    endpoint,
                    allowed_ips,
                    &status_tx,
                )
                .await;
            }

            // Clean up disconnected peers
            let current_peer_keys: Vec<String> = interface_data
                .peers
                .iter()
                .map(|(pub_key, _peer)| pub_key.to_string())
                .collect();

            let disconnected_peers: Vec<(String, ConnectionId)> = {
                let mut peers = self.peers.write().await;
                let gone: Vec<String> = peers
                    .keys()
                    .filter(|k| !current_peer_keys.contains(k))
                    .cloned()
                    .collect();
                gone.into_iter()
                    .filter_map(|k| peers.remove(&k).map(|id| (k, id)))
                    .collect()
            };

            for (peer_key, connection_id) in disconnected_peers {
                info!("WireGuard peer disconnected: {}", peer_key);
                let _ = status_tx.send(format!(
                    "[INFO] Peer disconnected: {}",
                    short_key(&peer_key)
                ));

                app_state
                    .close_connection_on_server(server_id, connection_id)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
        }
    }

    /// Raise `wireguard_peer_connected` and apply whatever the handler/LLM decided.
    #[allow(clippy::too_many_arguments)]
    async fn handle_peer_connected(
        &self,
        app_state: &Arc<AppState>,
        server_id: crate::state::ServerId,
        connection_id: ConnectionId,
        peer_key: &str,
        endpoint: Option<SocketAddr>,
        allowed_ips: Vec<String>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let event = Event::new(
            &WIREGUARD_PEER_CONNECTED_EVENT,
            serde_json::json!({
                "public_key": peer_key,
                "endpoint": endpoint.map(|e| e.to_string()),
                "allowed_ips": allowed_ips,
                "server_public_key": self.public_key,
                "listen_port": self.listen_port,
            }),
        );

        let protocol = WireguardProtocol::new();
        let result = match call_llm(
            &self.llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                error!("WireGuard peer event handling failed: {}", e);
                let _ = status_tx.send(format!("[ERROR] WireGuard peer event failed: {}", e));
                return;
            }
        };

        for message in &result.messages {
            info!("{}", message);
            let _ = status_tx.send(format!("[INFO] {}", message));
        }

        // Apply the decided actions to the live interface.
        for action in &result.raw_actions {
            let action_type = action.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match action_type {
                "authorize_peer" => {
                    let public_key = action
                        .get("public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or(peer_key)
                        .to_string();
                    let ips: Vec<String> = action
                        .get("allowed_ips")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_else(|| allowed_ips.clone());

                    if ips.is_empty() {
                        warn!("authorize_peer for {} has no allowed_ips", public_key);
                        let _ = status_tx.send(format!(
                            "[WARN] authorize_peer for {} ignored: no allowed_ips",
                            short_key(&public_key)
                        ));
                        continue;
                    }

                    let ep = action
                        .get("endpoint")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<SocketAddr>().ok())
                        .or(endpoint);

                    if let Err(e) = self.add_peer(public_key.clone(), ips, ep).await {
                        error!("Failed to authorize WireGuard peer {}: {}", public_key, e);
                        let _ = status_tx.send(format!("[ERROR] authorize_peer failed: {}", e));
                    }
                }
                "disconnect_peer" | "reject_peer" => {
                    let public_key = action
                        .get("public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or(peer_key)
                        .to_string();

                    if let Err(e) = self.remove_peer(public_key.clone(), status_tx).await {
                        error!("Failed to remove WireGuard peer {}: {}", public_key, e);
                        let _ = status_tx.send(format!("[ERROR] disconnect_peer failed: {}", e));
                    }
                }
                other => {
                    debug!("WireGuard: no interface change for action '{}'", other);
                }
            }
        }
    }

    /// Add / (re)configure a peer on the WireGuard interface.
    ///
    /// Used by both the reactive `authorize_peer` path (after a peer already
    /// appeared) and the user-triggered `wireguard_add_peer` action (to
    /// **pre-authorize** a peer's public key *before* it handshakes). A WireGuard
    /// responder drops a handshake whose static key is not already a configured
    /// peer, so pre-adding the key is what makes a subsequent handshake succeed and
    /// `wireguard_peer_connected` reachable. Fail-closed: with no such call, no peer
    /// is configured and the handshake is dropped by the backend.
    pub async fn add_peer(
        &self,
        peer_public_key: String,
        allowed_ips: Vec<String>,
        endpoint: Option<SocketAddr>,
    ) -> Result<()> {
        info!("Adding WireGuard peer: {}", peer_public_key);
        let _ = self.status_tx.send(format!(
            "[INFO] Adding peer: {}",
            short_key(&peer_public_key)
        ));

        // Check peer limit
        let peer_count = {
            let peers = self.peers.read().await;
            peers.len()
        };

        if peer_count >= MAX_PEERS {
            return Err(anyhow::anyhow!(
                "Maximum number of peers ({}) reached",
                MAX_PEERS
            ));
        }

        // Validate and build the peer config (single validation point; fail-closed
        // on a malformed key or bad/empty allowed IPs).
        let peer = build_peer_config(&peer_public_key, &allowed_ips, endpoint)?;

        // Configure peer
        let wgapi = self.wgapi.write().await;
        wgapi
            .configure_peer(&peer)
            .context("Failed to configure peer")?;

        info!("WireGuard peer added successfully: {}", peer_public_key);
        let _ = self.status_tx.send(format!(
            "→ Peer authorized: {}",
            short_key(&peer_public_key)
        ));

        Ok(())
    }

    /// Remove a peer from the WireGuard interface (called by LLM action)
    pub async fn remove_peer(
        &self,
        peer_public_key: String,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("Removing WireGuard peer: {}", peer_public_key);
        let _ = status_tx.send(format!(
            "[INFO] Removing peer: {}",
            short_key(&peer_public_key)
        ));

        // Parse peer public key
        let peer_key: Key = peer_public_key.parse().context("Invalid peer public key")?;

        // Remove peer
        let wgapi = self.wgapi.write().await;
        wgapi
            .remove_peer(&peer_key)
            .context("Failed to remove peer")?;

        // Remove from tracking
        {
            let mut peers = self.peers.write().await;
            peers.remove(&peer_public_key);
        }

        info!("WireGuard peer removed: {}", peer_public_key);
        let _ = status_tx.send(format!("→ Peer removed: {}", short_key(&peer_public_key)));

        Ok(())
    }

    /// Get server public key
    pub fn get_public_key(&self) -> &str {
        &self.public_key
    }

    /// Get current peer list
    pub async fn list_peers(&self) -> Vec<String> {
        let peers = self.peers.read().await;
        peers.keys().cloned().collect()
    }
}

impl Drop for WireguardServer {
    fn drop(&mut self) {
        // Cleanup will be handled when wgapi is dropped
        info!("WireGuard server shutting down");
    }
}
