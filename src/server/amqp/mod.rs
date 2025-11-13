//! AMQP server implementation
//!
//! Basic AMQP 0.9.1 broker that accepts connections and handles messaging.
//! This is a simplified implementation focused on LLM integration.
//!
//! The LLM controls all queues, exchanges, and message routing.
//! No persistent storage - all state is maintained in memory and by the LLM.

pub mod actions;

use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use crate::{console_debug, console_error, console_info, console_trace};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error};

/// AMQP server/broker
pub struct AmqpServer;

impl AmqpServer {
    /// Spawn AMQP broker with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        _startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        console_info!(status_tx, "AMQP broker listening on {}", local_addr);

        // Shared state for connected clients
        let clients: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let state_clone = state.clone();
        let status_tx_clone = status_tx.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        debug!("AMQP connection from {}", peer_addr);
                        console_debug!(status_tx_clone, "AMQP connection from {}", peer_addr);

                        let clients_clone = clients.clone();
                        let state_clone2 = state_clone.clone();
                        let status_tx_clone2 = status_tx_clone.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_amqp_connection(
                                socket,
                                peer_addr,
                                local_addr,
                                clients_clone,
                                state_clone2,
                                status_tx_clone2,
                                server_id,
                            )
                            .await
                            {
                                error!("AMQP connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("AMQP accept error: {}", e);
                        console_error!(status_tx_clone, "AMQP accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

async fn handle_amqp_connection(
    mut socket: TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    _clients: Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>,
    state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
) -> Result<()> {
    let connection_id = ConnectionId::new(state.get_next_unified_id().await);

    // Track connection
    let conn_info = ProtocolConnectionInfo {
        data: serde_json::json!({
            "client_id": Option::<String>::None,
            "virtual_host": "/",
        }),
    };
    let now = std::time::Instant::now();
    let conn_state = ConnectionState {
        id: connection_id,
        remote_addr: peer_addr,
        local_addr,
        bytes_sent: 0,
        bytes_received: 0,
        packets_sent: 0,
        packets_received: 0,
        last_activity: now,
        status: ConnectionStatus::Active,
        status_changed_at: now,
        protocol_info: conn_info,
    };
    state
        .add_connection_to_server(server_id, conn_state)
        .await;

    console_info!(
        status_tx,
        "AMQP client {} connected from {}",
        connection_id,
        peer_addr
    );

    // Wait for AMQP protocol header
    // AMQP 0.9.1 starts with "AMQP\x00\x00\x09\x01"
    let mut header = vec![0u8; 8];
    socket
        .read_exact(&mut header)
        .await
        .context("Failed to read AMQP protocol header")?;

    if &header[0..4] != b"AMQP" {
        console_error!(
            status_tx,
            "Invalid AMQP protocol header from {}",
            peer_addr
        );
        return Err(anyhow::anyhow!("Invalid AMQP protocol header"));
    }

    console_trace!(
        status_tx,
        "AMQP protocol header received: {:?}",
        header
    );

    // Send Connection.Start
    // This is a simplified implementation - real AMQP would send proper frames
    let connection_start = create_connection_start_frame();
    socket
        .write_all(&connection_start)
        .await
        .context("Failed to send Connection.Start")?;

    console_debug!(status_tx, "Sent Connection.Start to {}", peer_addr);

    // Read frames and process with LLM
    let (read_half, write_half) = tokio::io::split(socket);
    let write_half = Arc::new(Mutex::new(write_half));

    // Spawn frame reader
    tokio::spawn(async move {
        let mut reader = read_half;
        loop {
            // Read frame header (7 bytes: type + channel + size)
            let mut frame_header = vec![0u8; 7];
            match reader.read_exact(&mut frame_header).await {
                Ok(_) => {
                    let frame_type = frame_header[0];
                    let channel = u16::from_be_bytes([frame_header[1], frame_header[2]]);
                    let size =
                        u32::from_be_bytes([frame_header[3], frame_header[4], frame_header[5], frame_header[6]]);

                    console_trace!(
                        status_tx,
                        "AMQP frame: type={}, channel={}, size={}",
                        frame_type,
                        channel,
                        size
                    );

                    // Read frame payload
                    let mut payload = vec![0u8; size as usize];
                    if let Err(e) = reader.read_exact(&mut payload).await {
                        console_error!(status_tx, "Failed to read frame payload: {}", e);
                        break;
                    }

                    // Read frame end marker (0xCE)
                    let mut frame_end = [0u8; 1];
                    if let Err(e) = reader.read_exact(&mut frame_end).await {
                        console_error!(status_tx, "Failed to read frame end: {}", e);
                        break;
                    }

                    if frame_end[0] != 0xCE {
                        console_error!(status_tx, "Invalid frame end marker");
                        break;
                    }

                    // Process frame with LLM
                    // For now, just send basic responses
                    match frame_type {
                        1 => {
                            // Method frame
                            console_debug!(status_tx, "Received AMQP method frame");
                            // Here the LLM would process the method and decide response
                        }
                        2 => {
                            // Content header
                            console_debug!(status_tx, "Received AMQP content header");
                        }
                        3 => {
                            // Content body
                            console_debug!(status_tx, "Received AMQP content body");
                        }
                        4 => {
                            // Heartbeat
                            console_trace!(status_tx, "Received AMQP heartbeat");
                            // Send heartbeat back
                            let heartbeat = create_heartbeat_frame();
                            let mut writer = write_half.lock().await;
                            let _ = writer.write_all(&heartbeat).await;
                        }
                        _ => {
                            console_error!(status_tx, "Unknown AMQP frame type: {}", frame_type);
                        }
                    }
                }
                Err(_) => {
                    console_info!(status_tx, "AMQP client {} disconnected", connection_id);
                    state
                        .update_connection_status(
                            server_id,
                            connection_id,
                            ConnectionStatus::Closed,
                        )
                        .await;
                    break;
                }
            }
        }
    });

    Ok(())
}

/// Create AMQP Connection.Start frame
fn create_connection_start_frame() -> Vec<u8> {
    // Simplified Connection.Start frame
    // Frame type (1 = method), channel (0), size, payload, frame end (0xCE)
    // This is a minimal implementation - production would use proper frame encoding
    vec![
        1,    // Frame type: Method
        0, 0, // Channel 0
        0, 0, 0, 20, // Payload size (20 bytes)
        // Payload: Connection.Start (class 10, method 10)
        0, 10, // Class: Connection
        0, 10, // Method: Start
        0, 0, 0, 9, 1, // Version 0-9-1
        0, 0, 0, 0, // Server properties (empty)
        0, 0, 0, 5, // Mechanisms: PLAIN
        b'P', b'L', b'A', b'I', b'N', 0, 0, 0, 5, // Locales: en_US
        b'e', b'n', b'_', b'U', b'S', 0xCE, // Frame end
    ]
}

/// Create AMQP heartbeat frame
fn create_heartbeat_frame() -> Vec<u8> {
    vec![
        8,    // Frame type: Heartbeat
        0, 0, // Channel 0
        0, 0, 0, 0, // Payload size 0
        0xCE, // Frame end
    ]
}
