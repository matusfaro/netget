//! WireGuard VPN server implementation with LLM control
//!
//! This is a FULL WireGuard VPN server that creates actual tunnels for clients.
//! It uses defguard_wireguard_rs for cross-platform WireGuard support.
//!
//! The LLM controls:
//! - Peer authorization (approve/reject new peers)
//! - Traffic inspection policies (which peers to monitor)
//! - Routing decisions
//! - Connection limits and rate limiting

pub mod actions;

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ProtocolConnectionInfo, ConnectionStatus};
use crate::server::connection::ConnectionId;
use anyhow::{Result, Context};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, trace};

use defguard_wireguard_rs::{
    InterfaceConfiguration, WGApi, WireguardInterfaceApi,
    host::Peer as WGPeer,
    key::Key,
    net::IpAddrMask,
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
}

impl WireguardServer {
    /// Spawn WireGuard VPN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        bind_addr: SocketAddr,
        _llm_client: Arc<OllamaClient>,
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
            return Err(anyhow::anyhow!("Unsupported operating system for WireGuard"));
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
        wgapi.create_interface()
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
        wgapi.configure_interface(&interface_config)
            .context("Failed to configure WireGuard interface")?;

        #[cfg(windows)]
        wgapi.configure_interface(&interface_config, &[])
            .context("Failed to configure WireGuard interface")?;

        info!("WireGuard interface configured on port {}", listen_port);
        let _ = status_tx.send(format!("[INFO] Interface listening on UDP port {}", listen_port));
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
        });

        // Spawn monitoring task to track peer connections
        let server_clone = server.clone();
        let status_clone = status_tx.clone();
        let app_state_clone = app_state.clone();
        tokio::spawn(async move {
            server_clone.monitor_peers(
                app_state_clone,
                server_id,
                status_clone,
            ).await;
        });

        info!("WireGuard VPN server ready on {}", actual_addr);
        let _ = status_tx.send(format!("→ WireGuard VPN server ready on {}", actual_addr));
        let _ = status_tx.send(format!("[INFO] Clients can connect using server public key: {}", public_key_str));

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

            trace!("WireGuard interface status: {} peers", interface_data.peers.len());

            // Track new peers and update existing
            for (pub_key, peer) in interface_data.peers.iter() {
                let peer_key = pub_key.to_string();

                let mut peers = self.peers.write().await;

                if !peers.contains_key(&peer_key) {
                    // New peer discovered
                    let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                    peers.insert(peer_key.clone(), connection_id);

                    info!("New WireGuard peer connected: {}", peer_key);
                    let _ = status_tx.send(format!("[INFO] New peer: {}", &peer_key[..16]));

                    // Determine endpoint
                    let remote_addr = peer.endpoint;

                    // Add connection to server state
                    let now = std::time::Instant::now();
                    let conn_state = ConnectionState {
                        id: connection_id,
                        remote_addr: remote_addr.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap()),
                        local_addr: SocketAddr::new("10.20.30.1".parse().unwrap(), self.listen_port),
                        bytes_sent: peer.tx_bytes,
                        bytes_received: peer.rx_bytes,
                        packets_sent: 0,
                        packets_received: 0,
                        last_activity: now,
                        status: ConnectionStatus::Active,
                        status_changed_at: now,
                        protocol_info: ProtocolConnectionInfo::empty(),
                    };

                    app_state.add_connection_to_server(server_id, conn_state).await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                } else {
                    // Update existing peer stats
                    let connection_id = peers.get(&peer_key).unwrap();
                    app_state.update_connection_stats(
                        server_id,
                        *connection_id,
                        Some(peer.rx_bytes),
                        Some(peer.tx_bytes),
                        None,
                        None,
                    ).await;
                }
            }

            // Clean up disconnected peers
            let current_peer_keys: Vec<String> = interface_data.peers.iter()
                .map(|(pub_key, _peer)| pub_key.to_string())
                .collect();

            let mut peers = self.peers.write().await;
            let disconnected_peers: Vec<String> = peers.keys()
                .filter(|k| !current_peer_keys.contains(k))
                .cloned()
                .collect();

            for peer_key in disconnected_peers {
                if let Some(connection_id) = peers.remove(&peer_key) {
                    info!("WireGuard peer disconnected: {}", peer_key);
                    let _ = status_tx.send(format!("[INFO] Peer disconnected: {}", &peer_key[..16]));

                    app_state.close_connection_on_server(server_id, connection_id).await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                }
            }
        }
    }

    /// Add a peer to the WireGuard interface (called by LLM action)
    pub async fn add_peer(
        &self,
        peer_public_key: String,
        allowed_ips: Vec<String>,
        endpoint: Option<SocketAddr>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("Adding WireGuard peer: {}", peer_public_key);
        let _ = status_tx.send(format!("[INFO] Adding peer: {}", &peer_public_key[..16]));

        // Check peer limit
        let peer_count = {
            let peers = self.peers.read().await;
            peers.len()
        };

        if peer_count >= MAX_PEERS {
            return Err(anyhow::anyhow!("Maximum number of peers ({}) reached", MAX_PEERS));
        }

        // Parse peer public key
        let peer_key: Key = peer_public_key.parse()
            .context("Invalid peer public key")?;

        // Parse allowed IPs
        let allowed_ip_masks: Vec<IpAddrMask> = allowed_ips.iter()
            .filter_map(|ip_str| {
                ip_str.parse::<IpAddrMask>().ok()
            })
            .collect();

        if allowed_ip_masks.is_empty() {
            return Err(anyhow::anyhow!("No valid allowed IPs provided"));
        }

        // Create peer configuration
        let mut peer = WGPeer::new(peer_key);
        peer.allowed_ips = allowed_ip_masks;

        if let Some(ep) = endpoint {
            peer.endpoint = Some(ep);
        }

        // Configure peer
        let wgapi = self.wgapi.write().await;
        wgapi.configure_peer(&peer)
            .context("Failed to configure peer")?;

        info!("WireGuard peer added successfully: {}", peer_public_key);
        let _ = status_tx.send(format!("→ Peer authorized: {}", &peer_public_key[..16]));

        Ok(())
    }

    /// Remove a peer from the WireGuard interface (called by LLM action)
    pub async fn remove_peer(
        &self,
        peer_public_key: String,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("Removing WireGuard peer: {}", peer_public_key);
        let _ = status_tx.send(format!("[INFO] Removing peer: {}", &peer_public_key[..16]));

        // Parse peer public key
        let peer_key: Key = peer_public_key.parse()
            .context("Invalid peer public key")?;

        // Remove peer
        let wgapi = self.wgapi.write().await;
        wgapi.remove_peer(&peer_key)
            .context("Failed to remove peer")?;

        // Remove from tracking
        {
            let mut peers = self.peers.write().await;
            peers.remove(&peer_public_key);
        }

        info!("WireGuard peer removed: {}", peer_public_key);
        let _ = status_tx.send(format!("→ Peer removed: {}", &peer_public_key[..16]));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_peers_constant() {
        assert_eq!(MAX_PEERS, 100);
    }
}
