//! USB HID Mouse server implementation
//!
//! This module implements a virtual USB HID mouse using the USB/IP protocol.
//! The mouse can be controlled by the LLM to move the cursor, click buttons,
//! and scroll the wheel.

pub mod actions;

#[cfg(feature = "usb-mouse")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-mouse")]
use std::collections::HashMap;
#[cfg(feature = "usb-mouse")]
use std::net::SocketAddr;
#[cfg(feature = "usb-mouse")]
use std::sync::Arc;
#[cfg(feature = "usb-mouse")]
use tokio::sync::{mpsc, Mutex};
#[cfg(feature = "usb-mouse")]
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "usb-mouse")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "usb-mouse")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-mouse")]
use crate::llm::ActionResult;
#[cfg(feature = "usb-mouse")]
use crate::protocol::Event;
#[cfg(feature = "usb-mouse")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-mouse")]
use crate::server::usb::descriptors::*;
#[cfg(feature = "usb-mouse")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-mouse")]
use actions::USB_MOUSE_ATTACHED_EVENT;

/// Connection state for LLM processing
#[cfg(feature = "usb-mouse")]
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for USB mouse
#[cfg(feature = "usb-mouse")]
struct ConnectionData {
    state: ConnectionState,
    memory: String,
}

/// USB HID Mouse server
#[cfg(feature = "usb-mouse")]
pub struct UsbMouseServer;

#[cfg(feature = "usb-mouse")]
impl UsbMouseServer {
    /// Spawn the USB mouse server with LLM integration
    ///
    /// This creates a USB/IP server that exports a virtual HID mouse device.
    /// The LLM can control the mouse through actions like move, click, and scroll.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // Create and bind TCP server for USB/IP protocol
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("USB Mouse server listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "USB Mouse server listening on {}",
            local_addr
        ));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(crate::server::usb::mouse::UsbMouseProtocol::new());

        // Spawn accept loop for USB/IP connections
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "USB/IP connection {} from {} (USB mouse device)",
                            connection_id, remote_addr
                        );

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                "state": "WaitingForImport"
                            })),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Handle USB/IP connection
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                connection_id,
                                remote_addr,
                                llm_client_clone,
                                app_state_clone,
                                status_tx_clone,
                                connections_clone,
                                protocol_clone,
                                server_id,
                            )
                            .await
                            {
                                error!("USB mouse connection {} error: {}", connection_id, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept USB/IP connection: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle a single USB/IP connection
    ///
    /// This implements the USB/IP protocol:
    /// 1. Wait for OP_REQ_DEVLIST or OP_REQ_IMPORT
    /// 2. Export virtual mouse device
    /// 3. Process URBs (USB Request Blocks) from host
    /// 4. Call LLM on device attach and for custom actions
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<crate::server::usb::mouse::UsbMouseProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        info!(
            "USB mouse connection {} starting USB/IP protocol handler",
            connection_id
        );

        // Initialize connection data
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                memory: String::new(),
            },
        );

        // Placeholder: Return error to indicate implementation needed
        Err(anyhow::anyhow!(
            "USB/IP protocol handler not yet fully implemented - requires usbip crate integration"
        ))
    }
}
