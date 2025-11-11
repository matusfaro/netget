//! MQTT broker implementation
//!
//! Basic MQTT v3.1.1 broker that accepts connections and handles pub/sub.
//! This is a simplified implementation focused on LLM integration.

pub mod actions;

use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use crate::{console_debug, console_info, console_trace};
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

/// MQTT broker
pub struct MqttServer;

impl MqttServer {
    /// Spawn MQTT broker with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        _startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        console_info!(status_tx, "MQTT broker listening on {}", local_addr);

        // Shared state for connected clients
        let clients: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        debug!("MQTT connection from {}", peer_addr);
                        let _ = status_tx_clone
                            .send(format!("[DEBUG] MQTT connection from {}", peer_addr));

                        let clients_clone = clients.clone();
                        let app_state_clone2 = app_state_clone.clone();
                        let status_tx_clone2 = status_tx_clone.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_mqtt_connection(
                                socket,
                                peer_addr,
                                local_addr,
                                clients_clone,
                                app_state_clone2,
                                status_tx_clone2,
                                server_id,
                            )
                            .await
                            {
                                error!("MQTT connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("MQTT accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

async fn handle_mqtt_connection(
    mut socket: TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    _clients: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
) -> Result<()> {
    let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

    // Wait for CONNECT packet
    let mut buf = vec![0u8; 1024];
    let n = socket.read(&mut buf).await?;

    if n == 0 {
        return Ok(());
    }

    console_trace!(status_tx, "MQTT received {} bytes", n);

    // Parse CONNECT packet (very basic)
    let data = &buf[..n];

    // MQTT CONNECT packet type is 0x10 (packet type 1, flags 0)
    if n < 2 || data[0] != 0x10 {
        error!("Invalid MQTT CONNECT packet");
        return Ok(());
    }

    // Extract client ID from CONNECT payload (simplified parsing)
    let client_id = extract_client_id(data).unwrap_or_else(|| "unknown".to_string());

    console_info!(status_tx, "MQTT client connected: {}", client_id);

    // Add connection to app state
    let now = std::time::Instant::now();

    // Create MQTT-specific connection info
    let mqtt_info = serde_json::json!({
        "client_id": client_id.clone(),
        "subscriptions": Vec::<String>::new(),
    });

    let conn_state = ConnectionState {
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
        protocol_info: ProtocolConnectionInfo::new(mqtt_info),
    };
    app_state
        .add_connection_to_server(server_id, conn_state)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());

    // Send CONNACK (connection acknowledgment)
    let connack = build_connack();
    socket.write_all(&connack).await?;

    console_debug!(status_tx, "MQTT sent CONNACK to {}", client_id);

    // Keep connection alive and handle subsequent packets
    loop {
        let n = socket.read(&mut buf).await?;

        if n == 0 {
            // Connection closed
            console_info!(status_tx, "MQTT client disconnected: {}", client_id);
            break;
        }

        trace!("MQTT received {} bytes from {}", n, client_id);

        // Basic packet handling (PINGREQ, DISCONNECT, etc.)
        let packet_type = (buf[0] & 0xF0) >> 4;

        match packet_type {
            12 => {
                // PINGREQ - respond with PINGRESP
                let pingresp = vec![0xD0, 0x00];
                socket.write_all(&pingresp).await?;
                trace!("MQTT sent PINGRESP to {}", client_id);
            }
            14 => {
                // DISCONNECT
                info!("MQTT client {} sent DISCONNECT", client_id);
                break;
            }
            _ => {
                trace!("MQTT packet type {} from {}", packet_type, client_id);
            }
        }
    }

    Ok(())
}

/// Extract client ID from CONNECT packet payload (simplified)
fn extract_client_id(data: &[u8]) -> Option<String> {
    // MQTT CONNECT packet structure (simplified):
    // Byte 0: Packet type (0x10)
    // Byte 1: Remaining length
    // Bytes 2-3: Protocol name length (0x00 0x04 for "MQTT")
    // Bytes 4-7: "MQTT"
    // Byte 8: Protocol level (4 for v3.1.1)
    // Byte 9: Connect flags
    // Bytes 10-11: Keep alive
    // Bytes 12-13: Client ID length
    // Bytes 14+: Client ID

    if data.len() < 14 {
        return None;
    }

    // Skip to client ID length (byte 12-13)
    let client_id_len = u16::from_be_bytes([data[12], data[13]]) as usize;

    if data.len() < 14 + client_id_len {
        return None;
    }

    let client_id_bytes = &data[14..14 + client_id_len];
    String::from_utf8(client_id_bytes.to_vec()).ok()
}

/// Build CONNACK packet (Connection Acknowledgment)
fn build_connack() -> Vec<u8> {
    vec![
        0x20, // Packet type: CONNACK
        0x02, // Remaining length: 2 bytes
        0x00, // Connect Acknowledge Flags (session present = 0)
        0x00, // Connect Return code: 0 = Connection Accepted
    ]
}
