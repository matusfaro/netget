//! Tor client implementation using arti
pub mod actions;

pub use actions::TorClientProtocol;

use anyhow::{Context, Result};
use arti_client::{TorClient as ArtiClient, TorClientConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, warn};

#[cfg(feature = "tor")]
use tor_netdir::{NetDir, Relay};
#[cfg(feature = "tor")]
use serde::Serialize;

use crate::client::tor::actions::{TOR_CLIENT_CONNECTED_EVENT, TOR_CLIENT_DATA_RECEIVED_EVENT};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_data: Vec<u8>,
    memory: String,
}

/// Relay filter criteria for directory queries
#[cfg(feature = "tor")]
#[derive(Debug, Clone, Default)]
pub struct RelayFilter {
    pub flags: Option<Vec<String>>,
    pub min_bandwidth: Option<u64>,
    pub nickname_pattern: Option<String>,
    pub limit: Option<usize>,
}

#[cfg(feature = "tor")]
impl RelayFilter {
    /// Check if a relay matches this filter
    fn matches(&self, relay: &Relay<'_>) -> bool {
        // Check flags
        if let Some(ref required_flags) = self.flags {
            for flag in required_flags {
                let matches = match flag.as_str() {
                    "Guard" => relay.is_flagged_guard(),
                    "Exit" => relay.is_flagged_exit(),
                    "Fast" => relay.is_flagged_fast(),
                    "Stable" => relay.is_flagged_stable(),
                    "Running" => relay.is_flagged_running(),
                    "Valid" => relay.is_flagged_valid(),
                    _ => false,
                };
                if !matches {
                    return false;
                }
            }
        }

        // Check nickname pattern (simple contains check)
        if let Some(ref pattern) = self.nickname_pattern {
            if !relay.nickname().contains(pattern.as_str()) {
                return false;
            }
        }

        true
    }
}

/// Simplified relay information for LLM
#[cfg(feature = "tor")]
#[derive(Debug, Clone, Serialize)]
pub struct RelayInfo {
    pub nickname: String,
    pub fingerprint: String,
    pub flags: Vec<String>,
    pub is_guard: bool,
    pub is_exit: bool,
    pub is_fast: bool,
    pub is_stable: bool,
    pub is_running: bool,
    pub is_valid: bool,
}

#[cfg(feature = "tor")]
impl RelayInfo {
    /// Create RelayInfo from a Tor relay
    fn from_relay(relay: &Relay<'_>) -> Self {
        let mut flags = Vec::new();
        if relay.is_flagged_guard() {
            flags.push("Guard".to_string());
        }
        if relay.is_flagged_exit() {
            flags.push("Exit".to_string());
        }
        if relay.is_flagged_fast() {
            flags.push("Fast".to_string());
        }
        if relay.is_flagged_stable() {
            flags.push("Stable".to_string());
        }
        if relay.is_flagged_running() {
            flags.push("Running".to_string());
        }
        if relay.is_flagged_valid() {
            flags.push("Valid".to_string());
        }

        Self {
            nickname: relay.nickname().to_string(),
            fingerprint: format!("{:?}", relay.rsa_identity()),
            flags,
            is_guard: relay.is_flagged_guard(),
            is_exit: relay.is_flagged_exit(),
            is_fast: relay.is_flagged_fast(),
            is_stable: relay.is_flagged_stable(),
            is_running: relay.is_flagged_running(),
            is_valid: relay.is_flagged_valid(),
        }
    }
}

/// Tor client that connects through the Tor network
pub struct TorClient;

impl TorClient {
    /// Connect to a destination through Tor with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("Tor client {} initializing...", client_id);
        let _ = status_tx.send(format!("[CLIENT] Tor client {} initializing...", client_id));

        // Create and bootstrap Tor client
        let config = TorClientConfig::default();
        let tor_client = ArtiClient::create_bootstrapped(config)
            .await
            .context("Failed to bootstrap Tor client")?;

        info!("Tor client {} bootstrapped successfully", client_id);
        let _ = status_tx.send(format!("[CLIENT] Tor client {} bootstrapped", client_id));

        // Store Tor client for directory queries (requires experimental-api feature)
        #[cfg(feature = "tor")]
        app_state.set_tor_client(client_id, Arc::new(tor_client.clone())).await;

        // Parse target address (can be hostname:port or .onion:port)
        let target = remote_addr.clone();

        // Connect through Tor
        let stream = tor_client
            .connect(target.as_str())
            .await
            .context(format!("Failed to connect to {} through Tor", target))?;

        // Get a dummy local address since Tor connections don't have real local addresses
        let local_addr = SocketAddr::from(([127, 0, 0, 1], 0));

        info!(
            "Tor client {} connected to {} through Tor network",
            client_id, remote_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] Tor client {} connected to {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::tor::actions::TorClientProtocol::new());
            let event = Event::new(
                &TOR_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "target": remote_addr,
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                "",
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(_) => {
                    trace!(
                        "LLM called successfully for Tor client {} connection",
                        client_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to call LLM for Tor client {} connection: {}",
                        client_id, e
                    );
                }
            }
        }

        // Split stream into read/write halves
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
        }));

        // Spawn read loop
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 8192];

            loop {
                match read_half.read(&mut buffer).await {
                    Ok(0) => {
                        info!("Tor client {} disconnected", client_id);
                        app_state
                            .update_client_status(client_id, ClientStatus::Disconnected)
                            .await;
                        let _ = status_tx
                            .send(format!("[CLIENT] Tor client {} disconnected", client_id));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    Ok(n) => {
                        let data = buffer[..n].to_vec();
                        trace!("Tor client {} received {} bytes", client_id, n);

                        // Handle data with LLM
                        let mut client_data_lock = client_data.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Call LLM
                                if let Some(instruction) =
                                    app_state.get_instruction_for_client(client_id).await
                                {
                                    let protocol = Arc::new(
                                        crate::client::tor::actions::TorClientProtocol::new(),
                                    );
                                    let event = Event::new(
                                        &TOR_CLIENT_DATA_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&data),
                                            "data_length": data.len(),
                                        }),
                                    );

                                    match call_llm_for_client(
                                        &llm_client,
                                        &app_state,
                                        client_id.to_string(),
                                        &instruction,
                                        &client_data.lock().await.memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx,
                                    )
                                    .await
                                    {
                                        Ok(ClientLlmResult {
                                            actions,
                                            memory_updates,
                                        }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            for action in actions {
                                                use crate::llm::actions::client_trait::Client;
                                                match protocol.as_ref().execute_action(action) {
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(bytes)) => {
                                                        if let Ok(_) = write_half_arc.lock().await.write_all(&bytes).await {
                                                            trace!("Tor client {} sent {} bytes", client_id, bytes.len());
                                                        }
                                                    }
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                        info!("Tor client {} disconnecting", client_id);
                                                        break;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for Tor client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued data if any
                                let mut client_data_lock = client_data.lock().await;
                                if !client_data_lock.queued_data.is_empty() {
                                    client_data_lock.queued_data.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue data
                                client_data_lock.queued_data.extend_from_slice(&data);
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_data.extend_from_slice(&data);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Tor client {} read error: {}", client_id, e);
                        app_state
                            .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Get network directory from Arti (requires experimental-api feature)
    #[cfg(feature = "tor")]
    pub async fn get_netdir(
        app_state: &Arc<crate::state::app_state::AppState>,
        client_id: crate::state::ClientId,
    ) -> Result<Arc<NetDir>> {
        let tor_client = app_state
            .get_tor_client(client_id)
            .await
            .context("Tor client not found in app state")?;

        // Access directory manager (requires experimental-api feature)
        let dirmgr = tor_client.dirmgr();

        // Get current network directory
        let netdir = dirmgr
            .netdir(tor_netdir::Timeliness::Timely)
            .context("No network directory available - client may still be bootstrapping")?;

        Ok(netdir)
    }

    /// Query relays from network directory with optional filter
    #[cfg(feature = "tor")]
    pub async fn query_relays(
        app_state: &Arc<crate::state::app_state::AppState>,
        client_id: crate::state::ClientId,
        filter: RelayFilter,
    ) -> Result<Vec<RelayInfo>> {
        let netdir = Self::get_netdir(app_state, client_id).await?;

        let limit = filter.limit.unwrap_or(100);
        let mut relays = Vec::new();

        for relay in netdir.relays() {
            if filter.matches(&relay) {
                relays.push(RelayInfo::from_relay(&relay));
                if relays.len() >= limit {
                    break;
                }
            }
        }

        Ok(relays)
    }

    /// Get consensus metadata (relay count, validity times)
    #[cfg(feature = "tor")]
    pub async fn get_consensus_info(
        app_state: &Arc<crate::state::app_state::AppState>,
        client_id: crate::state::ClientId,
    ) -> Result<serde_json::Value> {
        let netdir = Self::get_netdir(app_state, client_id).await?;

        let relay_count = netdir.relays().count();
        let lifetime = netdir.lifetime();

        Ok(serde_json::json!({
            "relay_count": relay_count,
            "valid_after": lifetime.valid_after().to_string(),
            "fresh_until": lifetime.fresh_until().to_string(),
            "valid_until": lifetime.valid_until().to_string(),
        }))
    }
}
