//! WireGuard VPN client implementation
pub mod actions;

pub use actions::WireguardClientProtocol;

use anyhow::{Context, Result};
use defguard_wireguard_rs::{host::Peer, key::Key, InterfaceConfiguration, WGApi, WireguardInterfaceApi};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::client::wireguard::actions::{
    WIREGUARD_CLIENT_CONNECTED_EVENT, WIREGUARD_CLIENT_DISCONNECTED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Commands that can be sent to WireGuard client
#[derive(Debug)]
pub enum WireguardCommand {
    GetStatus(tokio::sync::oneshot::Sender<Result<serde_json::Value>>),
    Disconnect,
}

/// Global storage for WireGuard client command channels
static WIREGUARD_CLIENTS: LazyLock<Arc<RwLock<HashMap<ClientId, mpsc::UnboundedSender<WireguardCommand>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

/// WireGuard client startup parameters
#[derive(Debug, Clone)]
pub struct WireguardClientParams {
    /// Server's public key (base64 encoded)
    pub server_public_key: String,
    /// Server endpoint (IP:port)
    pub server_endpoint: String,
    /// Client's VPN IP address (e.g., "10.20.30.2/32")
    pub client_address: String,
    /// IPs to route through VPN (e.g., ["0.0.0.0/0"] for all traffic)
    pub allowed_ips: Vec<String>,
    /// Keepalive interval in seconds (0 to disable)
    pub keepalive: Option<u16>,
    /// Private key (optional - will be generated if not provided)
    pub private_key: Option<String>,
}

/// WireGuard client
pub struct WireguardClient {
    interface_name: String,
    wgapi: Arc<RwLock<WGApi>>,
    _private_key: String,
    public_key: String,
    server_public_key: Key,
    server_public_key_str: String,
    server_endpoint: String,
    client_address: String,
    allowed_ips: Vec<String>,
}

impl WireguardClient {
    /// Connect to a WireGuard VPN server with LLM integration
    pub async fn connect_with_llm_actions(
        remote_addr: String, // Format: "server_ip:port"
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        params: WireguardClientParams,
    ) -> Result<SocketAddr> {
        info!(
            "WireGuard client {} connecting to server {} (endpoint: {})",
            client_id, remote_addr, params.server_endpoint
        );

        // Generate or use provided keypair
        let private_key = if let Some(key) = params.private_key {
            Key::decode(&key).context("Failed to decode private key")?
        } else {
            Key::generate()
        };
        let public_key = private_key.public_key();

        let private_key_b64 = private_key.to_string();
        let public_key_b64 = public_key.to_string();

        info!("WireGuard client public key: {}", public_key_b64);
        let _ = status_tx.send(format!(
            "[CLIENT] WireGuard client {} public key: {}",
            client_id, public_key_b64
        ));

        // Platform-specific interface naming
        #[cfg(target_os = "macos")]
        let interface_name = "utun20".to_string();

        #[cfg(not(target_os = "macos"))]
        let interface_name = format!("netget_wg_client{}", client_id);

        // Create WireGuard interface
        let ifname = interface_name.clone();

        let wgapi = WGApi::new(ifname)
            .context("Failed to create WireGuard interface (requires elevated privileges)")?;

        info!("Created WireGuard interface: {}", interface_name);
        let _ = status_tx.send(format!(
            "[CLIENT] Created interface: {}",
            interface_name
        ));

        // Parse client address (e.g., "10.20.30.2/32")
        use defguard_wireguard_rs::net::IpAddrMask;
        let client_addr_mask: IpAddrMask = params.client_address.parse().context("Invalid client address")?;

        // Parse server endpoint
        let server_endpoint_addr: SocketAddr = params
            .server_endpoint
            .parse()
            .context("Invalid server endpoint")?;

        // Parse server public key
        let server_key =
            Key::decode(&params.server_public_key).context("Invalid server public key")?;

        // Parse listen port from remote_addr (use client_id as offset from base port)
        let listen_port = 51820u16.wrapping_add((client_id.as_u32() % 1000) as u16);

        // Configure interface
        let config = InterfaceConfiguration {
            name: interface_name.clone(),
            prvkey: private_key_b64.clone(),
            addresses: vec![client_addr_mask],
            port: listen_port as u32,
            peers: vec![Peer {
                public_key: server_key.clone(),
                preshared_key: None,
                protocol_version: None,
                endpoint: Some(server_endpoint_addr),
                last_handshake: None,
                tx_bytes: 0,
                rx_bytes: 0,
                persistent_keepalive_interval: params.keepalive,
                allowed_ips: params
                    .allowed_ips
                    .iter()
                    .map(|ip| ip.parse())
                    .collect::<Result<Vec<_>, _>>()
                    .context("Invalid allowed IPs")?,
            }],
            #[cfg(target_family = "unix")]
            mtu: None,
        };

        wgapi
            .configure_interface(&config)
            .context("Failed to configure WireGuard interface")?;

        info!("WireGuard interface configured");
        let _ = status_tx.send(format!(
            "[CLIENT] Interface configured: {} → {}",
            interface_name, params.server_endpoint
        ));

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] WireGuard client {} connected",
            client_id
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Wrap wgapi in Arc<RwLock>
        let wgapi_arc = Arc::new(RwLock::new(wgapi));

        // Create client instance
        let client = Arc::new(WireguardClient {
            interface_name: interface_name.clone(),
            wgapi: wgapi_arc.clone(),
            _private_key: private_key_b64,
            public_key: public_key_b64.clone(),
            server_public_key: server_key.clone(),
            server_public_key_str: params.server_public_key.clone(),
            server_endpoint: params.server_endpoint.clone(),
            client_address: params.client_address.clone(),
            allowed_ips: params.allowed_ips.clone(),
        });

        // Create command channel
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();

        // Store command channel in global map
        WIREGUARD_CLIENTS.write().await.insert(client_id, cmd_tx);

        // Spawn monitoring loop
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();
        let wgapi_monitor = wgapi_arc.clone();
        let server_key_clone = server_key.clone();
        let client_clone = client.clone();

        tokio::spawn(async move {
            Self::monitoring_loop(
                client_id,
                client_clone,
                wgapi_monitor,
                server_key_clone,
                &mut cmd_rx,
                app_state_clone,
                status_tx_clone,
                llm_client_clone,
            )
            .await;

            // Clean up on exit
            WIREGUARD_CLIENTS.write().await.remove(&client_id);
        });

        // Call LLM with connected event
        let event = Event::new(
            &WIREGUARD_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "server_endpoint": params.server_endpoint,
                "client_public_key": public_key_b64,
                "client_address": params.client_address,
            }),
        );

        // Get client instruction and memory
        let instruction = app_state.get_instruction_for_client(client_id).await.unwrap_or_default();
        let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

        // Create protocol instance for LLM call
        let protocol = Arc::new(crate::client::wireguard::WireguardClientProtocol::new());

        // Call LLM
        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            &status_tx,
        )
        .await
        {
            Ok(result) => {
                debug!("LLM response for connected event: {:?}", result);
                // Update memory if provided
                if let Some(new_memory) = result.memory_updates {
                    app_state
                        .set_memory_for_client(client_id, new_memory)
                        .await;
                }
            }
            Err(e) => {
                error!("Failed to call LLM for connected event: {}", e);
            }
        }

        // Return a dummy SocketAddr (WireGuard is UDP-based)
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            listen_port,
        ))
    }

    /// Monitoring loop to track connection status and handle commands
    async fn monitoring_loop(
        client_id: ClientId,
        client: Arc<WireguardClient>,
        wgapi: Arc<RwLock<WGApi>>,
        server_public_key: Key,
        cmd_rx: &mut mpsc::UnboundedReceiver<WireguardCommand>,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        llm_client: OllamaClient,
    ) {
        let mut was_connected = false;
        let mut interval = time::interval(Duration::from_secs(5));
        let mut should_exit = false;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Continue with status monitoring
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        WireguardCommand::GetStatus(response_tx) => {
                            let status = client.get_status().await;
                            let _ = response_tx.send(status);
                        }
                        WireguardCommand::Disconnect => {
                            info!("WireGuard client {} received disconnect command", client_id);
                            if let Err(e) = client.disconnect().await {
                                error!("Failed to disconnect WireGuard client {}: {}", client_id, e);
                            }
                            app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                            let _ = status_tx.send(format!("[CLIENT] WireGuard client {} disconnected", client_id));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());
                            should_exit = true;
                        }
                    }
                }
            }

            if should_exit {
                break;
            }

            // Check if client is still active
            if let Some(client) = app_state.get_client(client_id).await {
                if client.status == ClientStatus::Disconnected {
                    info!("WireGuard client {} stopped, exiting monitor loop", client_id);
                    break;
                }
            } else {
                // Client removed from state
                break;
            }

            // Read interface data
            let wgapi_lock = wgapi.read().await;
            match wgapi_lock.read_interface_data() {
                Ok(interface_data) => {
                    // Check server peer status
                    if let Some(peer) = interface_data.peers.get(&server_public_key) {
                        let is_connected = peer.last_handshake.is_some()
                            && peer
                                .last_handshake
                                .unwrap()
                                .elapsed()
                                .unwrap_or(Duration::from_secs(999))
                                < Duration::from_secs(180); // 3 minutes

                        if is_connected && !was_connected {
                            info!(
                                "WireGuard client {} handshake successful",
                                client_id
                            );
                            let _ = status_tx.send(format!(
                                "[CLIENT] WireGuard client {} handshake successful",
                                client_id
                            ));

                            was_connected = true;
                        } else if !is_connected && was_connected {
                            warn!(
                                "WireGuard client {} lost connection to server",
                                client_id
                            );
                            let _ = status_tx.send(format!(
                                "[CLIENT] WireGuard client {} lost connection",
                                client_id
                            ));

                            // Trigger disconnected event
                            let event = Event::new(
                                &WIREGUARD_CLIENT_DISCONNECTED_EVENT,
                                serde_json::json!({
                                    "reason": "handshake_timeout"
                                }),
                            );

                            let instruction = app_state.get_instruction_for_client(client_id).await.unwrap_or_default();
                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
                            let protocol = Arc::new(crate::client::wireguard::WireguardClientProtocol::new());

                            if let Ok(result) = call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol.as_ref(),
                                &status_tx,
                            )
                            .await
                            {
                                if let Some(new_memory) = result.memory_updates {
                                    app_state
                                        .set_memory_for_client(client_id, new_memory)
                                        .await;
                                }
                            }

                            was_connected = false;
                        }

                        // Log stats
                        if is_connected {
                            debug!(
                                "WireGuard client {} stats: tx={} rx={} last_handshake={}",
                                client_id,
                                peer.tx_bytes,
                                peer.rx_bytes,
                                peer.last_handshake
                                    .map(|t| format!("{:?} ago", t.elapsed().unwrap_or_default()))
                                    .unwrap_or_else(|| "never".to_string())
                            );
                        }
                    } else {
                        if was_connected {
                            warn!(
                                "WireGuard client {} server peer disappeared",
                                client_id
                            );
                            was_connected = false;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to read WireGuard interface data for client {}: {}",
                        client_id, e
                    );
                }
            }
        }
    }

    /// Disconnect the client
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting WireGuard client");

        // Remove interface (this will clean up everything)
        let wgapi = self.wgapi.read().await;
        wgapi
            .remove_interface()
            .context("Failed to remove WireGuard interface")?;

        Ok(())
    }

    /// Get the client's public key and configuration info
    pub fn get_client_info(&self) -> serde_json::Value {
        serde_json::json!({
            "interface": self.interface_name,
            "public_key": self.public_key,
            "private_key": "<redacted>",
            "client_address": self.client_address,
            "server_endpoint": self.server_endpoint,
            "server_public_key": self.server_public_key_str,
            "allowed_ips": self.allowed_ips,
        })
    }

    /// Get connection status
    pub async fn get_status(&self) -> Result<serde_json::Value> {
        let wgapi = self.wgapi.read().await;
        let interface_data = wgapi
            .read_interface_data()
            .context("Failed to read interface data")?;

        let peer = interface_data.peers.get(&self.server_public_key);

        Ok(serde_json::json!({
            "interface": self.interface_name,
            "public_key": self.public_key,
            "client_address": self.client_address,
            "server_endpoint": self.server_endpoint,
            "server_public_key": self.server_public_key_str,
            "connected": peer.and_then(|p| p.last_handshake).is_some(),
            "last_handshake": peer.and_then(|p| p.last_handshake)
                .map(|t| t.elapsed().unwrap_or_default().as_secs()),
            "tx_bytes": peer.map(|p| p.tx_bytes).unwrap_or(0),
            "rx_bytes": peer.map(|p| p.rx_bytes).unwrap_or(0),
            "allowed_ips": self.allowed_ips,
        }))
    }
}

/// Send a command to a WireGuard client
pub async fn send_command(client_id: ClientId, command: WireguardCommand) -> Result<()> {
    let clients = WIREGUARD_CLIENTS.read().await;
    let tx = clients
        .get(&client_id)
        .context("WireGuard client not found")?;
    tx.send(command)
        .map_err(|_| anyhow::anyhow!("Failed to send command to WireGuard client"))?;
    Ok(())
}

/// Get the connection status of a WireGuard client
pub async fn get_client_status(client_id: ClientId) -> Result<serde_json::Value> {
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    send_command(client_id, WireguardCommand::GetStatus(response_tx)).await?;

    match tokio::time::timeout(Duration::from_secs(5), response_rx).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => Err(anyhow::anyhow!("Failed to get status from client")),
        Err(_) => Err(anyhow::anyhow!("Timeout waiting for status response")),
    }
}

/// Disconnect a WireGuard client
pub async fn disconnect_client(client_id: ClientId) -> Result<()> {
    send_command(client_id, WireguardCommand::Disconnect).await
}
