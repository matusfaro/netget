//! TURN server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::{TURN_ALLOCATE_REQUEST_EVENT, TURN_REFRESH_REQUEST_EVENT, TURN_CREATE_PERMISSION_REQUEST_EVENT, TURN_SEND_INDICATION_EVENT};
use crate::server::TurnProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// TURN allocation information
#[derive(Clone, Debug)]
struct TurnAllocation {
    client_addr: SocketAddr,
    relay_addr: SocketAddr,
    #[allow(dead_code)]
    allocated_at: Instant,
    expires_at: Instant,
    lifetime_seconds: u32,
    permitted_peers: Vec<SocketAddr>,
}

/// TURN server that handles relay allocations
pub struct TurnServer {
    allocations: Arc<Mutex<HashMap<String, TurnAllocation>>>, // Key: allocation_id (hex string)
}

impl TurnServer {
    pub fn new() -> Self {
        Self {
            allocations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn TURN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("TURN server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] TURN server listening on {}", local_addr));

        let protocol = Arc::new(TurnProtocol::new());
        let server = Arc::new(Self::new());

        // Spawn allocation cleanup task
        Self::spawn_cleanup_task(server.clone(), status_tx.clone());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 2048]; // TURN messages are typically < 2KB

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();

                        // Get allocation info for this client
                        let allocations = server.allocations.lock().await;
                        let allocation_ids: Vec<String> = allocations
                            .iter()
                            .filter(|(_, alloc)| alloc.client_addr == peer_addr)
                            .map(|(id, _)| id.clone())
                            .collect();
                        drop(allocations);

                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Turn {
                                allocation_ids,
                                relay_addresses: Vec::new(),
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        debug!("TURN received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] TURN received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        trace!("TURN data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] TURN data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();
                        let server_clone = server.clone();

                        tokio::spawn(async move {
                            // Parse TURN/STUN message to determine type
                            let (transaction_id, message_type, is_valid) = Self::parse_turn_header(&data);

                            if !is_valid {
                                debug!("TURN invalid message from {}", peer_addr);
                                let _ = status_clone.send(format!("[DEBUG] TURN invalid message from {}", peer_addr));
                                return;
                            }

                            let transaction_id_hex = transaction_id.map(|tid| hex::encode(tid)).unwrap_or_default();

                            // Determine event type based on message
                            let event_type = match message_type.as_str() {
                                "AllocateRequest" => &TURN_ALLOCATE_REQUEST_EVENT,
                                "RefreshRequest" => &TURN_REFRESH_REQUEST_EVENT,
                                "CreatePermissionRequest" => &TURN_CREATE_PERMISSION_REQUEST_EVENT,
                                "SendIndication" => &TURN_SEND_INDICATION_EVENT,
                                _ => {
                                    debug!("TURN unknown message type: {}", message_type);
                                    let _ = status_clone.send(format!("[DEBUG] TURN unknown message type: {}", message_type));
                                    return;
                                }
                            };

                            // Get current allocations for this client
                            let allocations = server_clone.allocations.lock().await;
                            let client_allocations: Vec<serde_json::Value> = allocations
                                .iter()
                                .filter(|(_, alloc)| alloc.client_addr == peer_addr)
                                .map(|(id, alloc)| {
                                    serde_json::json!({
                                        "allocation_id": id,
                                        "relay_address": alloc.relay_addr.to_string(),
                                        "lifetime_seconds": alloc.lifetime_seconds,
                                        "expires_in_seconds": alloc.expires_at.saturating_duration_since(Instant::now()).as_secs(),
                                        "permitted_peers": alloc.permitted_peers.iter().map(|p| p.to_string()).collect::<Vec<_>>()
                                    })
                                })
                                .collect();
                            drop(allocations);

                            // Create TURN event
                            let event_data = serde_json::json!({
                                "peer_addr": peer_addr.to_string(),
                                "local_addr": local_addr.to_string(),
                                "transaction_id": transaction_id_hex,
                                "message_type": message_type,
                                "bytes_received": data.len(),
                                "existing_allocations": client_allocations
                            });

                            let event = Event::new(event_type, event_data);

                            debug!("TURN calling LLM for {} from {}", message_type, peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] TURN calling LLM for {} from {}", message_type, peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,  // TURN uses UDP, no persistent connection
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("TURN parsed {} actions", execution_result.raw_actions.len());
                                    let _ = status_clone.send(format!("[DEBUG] TURN parsed {} actions", execution_result.raw_actions.len()));

                                    // Extract allocation info from raw actions before execution
                                    for action in &execution_result.raw_actions {
                                        if let Some(action_type) = action.get("type").and_then(|v| v.as_str()) {
                                            if action_type == "send_turn_allocate_response" {
                                                // Track new allocation from action parameters
                                                if let (Some(alloc_id), Some(relay_addr_str), Some(lifetime)) = (
                                                    action.get("allocation_id").and_then(|v| v.as_str()),
                                                    action.get("relay_address").and_then(|v| v.as_str()),
                                                    action.get("lifetime_seconds").and_then(|v| v.as_u64())
                                                ) {
                                                    if let Ok(relay_addr) = relay_addr_str.parse::<SocketAddr>() {
                                                        let now = Instant::now();
                                                        let lifetime_secs = lifetime as u32;

                                                        let allocation = TurnAllocation {
                                                            client_addr: peer_addr,
                                                            relay_addr,
                                                            allocated_at: now,
                                                            expires_at: now + Duration::from_secs(lifetime as u64),
                                                            lifetime_seconds: lifetime_secs,
                                                            permitted_peers: Vec::new(),
                                                        };

                                                        server_clone.allocations.lock().await.insert(alloc_id.to_string(), allocation);
                                                        debug!("TURN created allocation {} for {} -> {}", alloc_id, peer_addr, relay_addr);
                                                        let _ = status_clone.send(format!("[DEBUG] TURN created allocation {} for {} -> {}", alloc_id, peer_addr, relay_addr));
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Send protocol results
                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!("TURN sent {} bytes to {}", output_data.len(), peer_addr);
                                            let _ = status_clone.send(format!("[DEBUG] TURN sent {} bytes to {}", output_data.len(), peer_addr));

                                            // TRACE: Log full payload
                                            let hex_str = hex::encode(output_data);
                                            trace!("TURN sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] TURN sent (hex): {}", hex_str));

                                            let _ = status_clone.send(format!("→ TURN response to {} ({} bytes)", peer_addr, output_data.len()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("TURN LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ TURN LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("TURN receive error: {}", e);
                        let _ = status_tx.send(format!("✗ TURN receive error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Spawn task to periodically clean up expired allocations
    fn spawn_cleanup_task(server: Arc<Self>, status_tx: mpsc::UnboundedSender<String>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                let now = Instant::now();
                let mut allocations = server.allocations.lock().await;
                let initial_count = allocations.len();

                allocations.retain(|id, alloc| {
                    if alloc.expires_at <= now {
                        debug!("TURN expired allocation {} for {}", id, alloc.client_addr);
                        let _ = status_tx.send(format!("[DEBUG] TURN expired allocation {} for {}", id, alloc.client_addr));
                        false
                    } else {
                        true
                    }
                });

                let removed = initial_count - allocations.len();
                if removed > 0 {
                    debug!("TURN cleanup removed {} expired allocations", removed);
                    let _ = status_tx.send(format!("[DEBUG] TURN cleanup removed {} expired allocations", removed));
                }
            }
        });
    }

    /// Parse TURN/STUN message header
    /// Returns (transaction_id, message_type_string, is_valid)
    fn parse_turn_header(data: &[u8]) -> (Option<Vec<u8>>, String, bool) {
        // TURN uses STUN message format, minimum 20 bytes
        if data.len() < 20 {
            return (None, "invalid".to_string(), false);
        }

        // Check magic cookie (bytes 4-7 should be 0x2112A442)
        let magic_cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        if magic_cookie != 0x2112A442 {
            return (None, "invalid".to_string(), false);
        }

        // Extract message type (first 2 bytes)
        let message_type_raw = u16::from_be_bytes([data[0], data[1]]);

        // Message type encoding: 0bMMMMMMMMMMCCCCMM
        let class = ((message_type_raw & 0x0110) >> 4) | ((message_type_raw & 0x0100) >> 7);
        let method = (message_type_raw & 0x000F)
                   | ((message_type_raw & 0x00E0) >> 1)
                   | ((message_type_raw & 0x3E00) >> 2);

        let message_type = match (class, method) {
            (0, 3) => "AllocateRequest",
            (1, 3) => "AllocateResponse",
            (2, 3) => "AllocateError",
            (0, 4) => "RefreshRequest",
            (1, 4) => "RefreshResponse",
            (0, 8) => "CreatePermissionRequest",
            (1, 8) => "CreatePermissionResponse",
            (0, 6) => "SendIndication",
            (0, 7) => "DataIndication",
            _ => "Unknown",
        };

        // Extract transaction ID (12 bytes, from byte 8 to 19)
        let transaction_id = data[8..20].to_vec();

        (Some(transaction_id), message_type.to_string(), true)
    }

    /// Get allocation by ID
    pub async fn get_allocation(&self, allocation_id: &str) -> Option<TurnAllocation> {
        self.allocations.lock().await.get(allocation_id).cloned()
    }

    /// Update allocation permissions
    pub async fn add_peer_permission(&self, allocation_id: &str, peer_addr: SocketAddr) -> Result<()> {
        let mut allocations = self.allocations.lock().await;
        if let Some(allocation) = allocations.get_mut(allocation_id) {
            if !allocation.permitted_peers.contains(&peer_addr) {
                allocation.permitted_peers.push(peer_addr);
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("Allocation not found"))
        }
    }
}
