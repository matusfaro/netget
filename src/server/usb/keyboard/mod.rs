//! USB HID Keyboard server implementation
//!
//! This module implements a virtual USB HID keyboard using the USB/IP protocol.
//! The keyboard can be controlled by the LLM to type text, press keys, and handle
//! key combinations (Ctrl+C, Alt+Tab, etc.).

pub mod actions;

// Re-export protocol struct for registration
#[cfg(feature = "usb-keyboard")]
pub use actions::UsbKeyboardProtocol;

#[cfg(feature = "usb-keyboard")]
use anyhow::Result;
#[cfg(feature = "usb-keyboard")]
use std::collections::HashMap;
#[cfg(feature = "usb-keyboard")]
use std::net::SocketAddr;
#[cfg(feature = "usb-keyboard")]
use std::sync::Arc;
#[cfg(feature = "usb-keyboard")]
use tokio::sync::{mpsc, Mutex};
#[cfg(feature = "usb-keyboard")]
use tracing::{debug, error, info};

#[cfg(feature = "usb-keyboard")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "usb-keyboard")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-keyboard")]
use crate::protocol::Event;
#[cfg(feature = "usb-keyboard")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-keyboard")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-keyboard")]
use actions::USB_KEYBOARD_ATTACHED_EVENT;

/// Connection state for LLM processing
#[cfg(feature = "usb-keyboard")]
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for USB keyboard
#[cfg(feature = "usb-keyboard")]
struct ConnectionData {
    state: ConnectionState,
    #[allow(dead_code)]
    memory: String,
    #[allow(dead_code)]
    led_status: u8, // Num Lock, Caps Lock, Scroll Lock
}

/// USB HID Keyboard server
#[cfg(feature = "usb-keyboard")]
pub struct UsbKeyboardServer;

#[cfg(feature = "usb-keyboard")]
impl UsbKeyboardServer {
    /// Spawn the USB keyboard server with LLM integration
    ///
    /// This creates a USB/IP server that exports a virtual HID keyboard device.
    /// The LLM can control the keyboard through actions like type_text and press_key.
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
        info!("USB Keyboard server listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "USB Keyboard server listening on {}",
            local_addr
        ));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(crate::server::usb::keyboard::UsbKeyboardProtocol::new());

        // Spawn accept loop for USB/IP connections
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "USB/IP connection {} from {} (USB keyboard device)",
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
                                "state": "WaitingForImport",
                                "led_status": 0
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
                                error!("USB keyboard connection {} error: {}", connection_id, e);
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
    /// This creates a USB/IP server that exports a virtual HID keyboard device.
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
        protocol: Arc<crate::server::usb::keyboard::UsbKeyboardProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        info!(
            "USB keyboard connection {} from {} - device ready for USB/IP import",
            connection_id, remote_addr
        );

        // Initialize connection data
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                memory: String::new(),
                led_status: 0,
            },
        );

        // Create HID keyboard handler from usbip crate
        let handler = Arc::new(std::sync::Mutex::new(
            Box::new(usbip::hid::UsbHidKeyboardHandler::new_keyboard())
                as Box<dyn usbip::UsbInterfaceHandler + Send>,
        ));

        // Store handler in protocol for action execution
        protocol.set_handler(connection_id, handler.clone()).await;

        // Create USB device with HID keyboard interface
        let device = usbip::UsbDevice::new(0).with_interface(
            usbip::ClassCode::HID as u8,
            0x00, // Subclass: no subclass
            0x00, // Protocol: none
            "NetGet Virtual Keyboard",
            vec![usbip::UsbEndpoint {
                address: 0x81,         // EP1 IN (interrupt)
                attributes: 0x03,      // Interrupt transfer
                max_packet_size: 0x08, // 8 bytes (keyboard report)
                interval: 10,          // 10ms polling interval
            }],
            handler.clone(),
        );

        // Create USB/IP server (not wrapped in Arc - usbip::server takes ownership)
        let server = usbip::UsbIpServer::new_simulated(vec![device]);

        // Get a unique address for this USB/IP device server
        // We bind to port 3240 (standard USB/IP port) on the remote address
        let usbip_addr = SocketAddr::new(remote_addr.ip(), 3240);

        info!(
            "Starting USB/IP server for keyboard on {} (connection {})",
            usbip_addr, connection_id
        );
        let _ = status_tx.send(format!(
            "USB keyboard device starting on {} - will be ready for: sudo usbip attach -r {} -b 1-1",
            usbip_addr, usbip_addr
        ));

        // Spawn USB/IP protocol server
        let connection_id_clone = connection_id;
        tokio::spawn(async move {
            usbip::server(usbip_addr, server).await;
            debug!("USB/IP server task completed for keyboard connection {}", connection_id_clone);
        });

        // Wait a moment for server to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        info!(
            "USB keyboard device ready on {} (connection {})",
            usbip_addr, connection_id
        );
        let _ = status_tx.send(format!(
            "USB keyboard ready - run: sudo usbip list -r {} && sudo usbip attach -r {} -b 1-1",
            usbip_addr, usbip_addr
        ));

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
                "Failed to call LLM on keyboard attach for connection {}: {}",
                connection_id, e
            );
        }

        // Keep connection alive - the USB/IP protocol runs independently
        // The handler will process URBs from the client
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
        protocol: &Arc<crate::server::usb::keyboard::UsbKeyboardProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        // Check if already processing
        {
            let conns = connections.lock().await;
            if let Some(conn_data) = conns.get(&connection_id) {
                if conn_data.state != ConnectionState::Idle {
                    debug!(
                        "USB keyboard connection {} already processing, skipping LLM call",
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
            &USB_KEYBOARD_ATTACHED_EVENT,
            serde_json::json!({
                "connection_id": connection_id.to_string(),
            }),
        );

        info!(
            "Calling LLM for USB keyboard attached event on connection {}",
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
                    "USB keyboard LLM call completed for connection {}",
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
                    "LLM call failed for USB keyboard connection {}: {}",
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
