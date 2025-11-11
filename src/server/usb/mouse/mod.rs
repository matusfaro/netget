//! USB HID Mouse server implementation
//!
//! This module implements a virtual USB HID mouse using the USB/IP protocol.
//! The mouse can be controlled by the LLM to move the cursor, click buttons,
//! and scroll the wheel.

pub mod actions;

// Re-export protocol struct for registration
#[cfg(feature = "usb-mouse")]
pub use actions::UsbMouseProtocol;

#[cfg(feature = "usb-mouse")]
use anyhow::Result;
#[cfg(feature = "usb-mouse")]
use std::collections::HashMap;
#[cfg(feature = "usb-mouse")]
use std::net::SocketAddr;
#[cfg(feature = "usb-mouse")]
use std::sync::Arc;
#[cfg(feature = "usb-mouse")]
use tokio::sync::{mpsc, Mutex};
#[cfg(feature = "usb-mouse")]
use tracing::{debug, error, info, warn};

#[cfg(feature = "usb-mouse")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "usb-mouse")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-mouse")]
use crate::protocol::Event;
#[cfg(feature = "usb-mouse")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-mouse")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-mouse")]
use actions::USB_MOUSE_ATTACHED_EVENT;

/// Connection state for LLM processing
#[cfg(feature = "usb-mouse")]
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for USB mouse
#[cfg(feature = "usb-mouse")]
struct ConnectionData {
    state: ConnectionState,
    #[allow(dead_code)]
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
        console_info!(status_tx, "USB Mouse server listening on {}");

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(crate::server::usb::mouse::UsbMouseProtocol::new());

        // Spawn accept loop for USB/IP connections
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "USB/IP connection {} from {} (USB mouse device)",
                            connection_id, remote_addr
                        );

                        // Add connection to ServerInstance
                        use crate::state::server::{
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
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
                        console_info!(status_tx, "__UPDATE_UI__");

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

    /// Handle USB/IP server lifecycle
    ///
    /// This creates a USB/IP server that exports a virtual HID mouse device.
    /// The server handles USB/IP protocol operations and integrates with LLM actions.
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        _stream: tokio::net::TcpStream,
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
            "USB mouse connection {} from {} - device ready for USB/IP import",
            connection_id, remote_addr
        );

        // Initialize connection data
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                memory: String::new(),
            },
        );

        // TODO: USB mouse handler not yet available in usbip crate
        // Once usbip crate adds UsbHidMouseHandler, uncomment the code below

        console_warn!(status_tx, "USB mouse device for connection {} from {} - NOT YET FUNCTIONAL (waiting for usbip crate mouse support)");

        // Placeholder: Would create HID mouse handler here
        // let handler = Arc::new(std::sync::Mutex::new(
        //     Box::new(usbip::hid::UsbHidMouseHandler::new_mouse())
        //         as Box<dyn usbip::UsbInterfaceHandler + Send>,
        // ));
        // protocol.set_handler(connection_id, handler.clone()).await;

        // Call LLM on device attach
        if let Err(e) = Self::call_llm_on_attach(
            connection_id,
            &llm_client,
            &app_state,
            &status_tx,
            &connections,
            &protocol,
            server_id,
        )
        .await
        {
            error!(
                "Failed to call LLM on mouse attach for connection {}: {}",
                connection_id, e
            );
        }

        // Keep connection alive - the USB/IP protocol runs independently
        tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;

        Ok(())
    }

    /// Call LLM when device is attached
    async fn call_llm_on_attach(
        connection_id: ConnectionId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        _status_tx: &mpsc::UnboundedSender<String>,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: &Arc<crate::server::usb::mouse::UsbMouseProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        // Check if already processing
        {
            let conns = connections.lock().await;
            if let Some(conn_data) = conns.get(&connection_id) {
                if conn_data.state != ConnectionState::Idle {
                    debug!(
                        "USB mouse connection {} already processing, skipping LLM call",
                        connection_id
                    );
                    return Ok(());
                }
            }
        }

        // Set state to processing
        {
            let mut conns = connections.lock().await;
            if let Some(conn_data) = conns.get_mut(&connection_id) {
                conn_data.state = ConnectionState::Processing;
            }
        }

        // Get instruction and memory
        let (_instruction, _memory) = {
            if let Some(server) = app_state.get_server(server_id).await {
                (server.instruction.clone(), String::new())
            } else {
                return Err(anyhow::anyhow!("Server not found"));
            }
        };

        // Create attached event
        let event = Event::new(
            &USB_MOUSE_ATTACHED_EVENT,
            serde_json::json!({
                "connection_id": connection_id.to_string(),
            }),
        );

        info!(
            "Calling LLM for USB mouse attached event on connection {}",
            connection_id
        );

        // Call LLM
        let result = call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        )
        .await;

        // Process result
        match result {
            Ok(_execution_result) => {
                // Actions have already been executed by call_llm
                info!(
                    "USB mouse LLM call completed for connection {}",
                    connection_id
                );

                // Set state back to idle
                let mut conns = connections.lock().await;
                if let Some(conn_data) = conns.get_mut(&connection_id) {
                    conn_data.state = ConnectionState::Idle;
                }
            }
            Err(e) => {
                error!(
                    "LLM call failed for USB mouse connection {}: {}",
                    connection_id, e
                );

                // Set state back to idle
                let mut conns = connections.lock().await;
                if let Some(conn_data) = conns.get_mut(&connection_id) {
                    conn_data.state = ConnectionState::Idle;
                }
            }
        }

        Ok(())
    }
}
