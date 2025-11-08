//! USB Mass Storage Class (MSC) server implementation
//!
//! This module implements a virtual USB Mass Storage device using the USB/IP protocol.
//! The device uses Bulk-Only Transport (BOT) and SCSI transparent command set to expose
//! a virtual disk that can be mounted by the host operating system.

pub mod actions;

#[cfg(feature = "usb-msc")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-msc")]
use std::collections::HashMap;
#[cfg(feature = "usb-msc")]
use std::net::SocketAddr;
#[cfg(feature = "usb-msc")]
use std::path::PathBuf;
#[cfg(feature = "usb-msc")]
use std::sync::Arc;
#[cfg(feature = "usb-msc")]
use tokio::sync::{mpsc, Mutex};
#[cfg(feature = "usb-msc")]
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "usb-msc")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "usb-msc")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-msc")]
use crate::llm::ActionResult;
#[cfg(feature = "usb-msc")]
use crate::protocol::Event;
#[cfg(feature = "usb-msc")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-msc")]
use crate::server::usb::descriptors::*;
#[cfg(feature = "usb-msc")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-msc")]
use actions::USB_MSC_ATTACHED_EVENT;

/// Connection state for LLM processing
#[cfg(feature = "usb-msc")]
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for USB MSC
#[cfg(feature = "usb-msc")]
struct ConnectionData {
    state: ConnectionState,
    memory: String,
    disk_path: PathBuf,
    write_protect: bool,
    total_sectors: u32,
    bytes_per_sector: u32,
}

/// USB Mass Storage Class server
#[cfg(feature = "usb-msc")]
pub struct UsbMscServer;

#[cfg(feature = "usb-msc")]
impl UsbMscServer {
    /// Spawn the USB MSC server with LLM integration
    ///
    /// This creates a USB/IP server that exports a virtual mass storage device.
    /// The LLM can control the device through actions like mount_disk, eject_disk, etc.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        disk_image: Option<PathBuf>,
    ) -> Result<SocketAddr> {
        // Create and bind TCP server for USB/IP protocol
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("USB Mass Storage server listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "USB Mass Storage server listening on {}",
            local_addr
        ));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(crate::server::usb::msc::UsbMscProtocol::new());

        // Spawn accept loop for USB/IP connections
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "USB/IP connection {} from {} (USB MSC device)",
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
                                "write_protect": true,
                                "total_sectors": 0
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
                        let disk_image_clone = disk_image.clone();

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
                                disk_image_clone,
                            )
                            .await
                            {
                                error!("USB MSC connection {} error: {}", connection_id, e);
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
    /// This creates a USB/IP server that exports a virtual mass storage device.
    ///
    /// **IMPORTANT**: This is a framework implementation. The usbip crate does not
    /// have built-in Mass Storage Class support. Full implementation requires:
    ///
    /// 1. Implementing UsbInterfaceHandler for MSC with BOT protocol
    /// 2. Parsing CBW (Command Block Wrapper) from bulk OUT endpoint
    /// 3. Handling SCSI commands (INQUIRY, READ_CAPACITY, READ(10), WRITE(10), etc.)
    /// 4. Sending data via bulk IN endpoint
    /// 5. Sending CSW (Command Status Wrapper) after each command
    /// 6. Managing virtual disk image file
    ///
    /// See CLAUDE.md for full implementation guide.
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        _stream: tokio::net::TcpStream,
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<crate::server::usb::msc::UsbMscProtocol>,
        server_id: crate::state::ServerId,
        disk_image: Option<PathBuf>,
    ) -> Result<()> {
        info!(
            "USB MSC connection {} from {} - mass storage device initialization",
            connection_id, remote_addr
        );

        // Initialize connection data
        let disk_path = disk_image.unwrap_or_else(|| PathBuf::from("/tmp/netget_msc_disk.img"));
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                memory: String::new(),
                disk_path: disk_path.clone(),
                write_protect: true,  // Start write-protected for safety
                total_sectors: 2048,  // Default: 1MB disk (2048 * 512 bytes)
                bytes_per_sector: 512,
            },
        );

        // TODO: Implement USB/IP MSC device
        //
        // This requires:
        // 1. Creating a custom UsbInterfaceHandler that implements BOT protocol
        // 2. Creating UsbDevice with MSC descriptors (build_msc_config_descriptor)
        // 3. Handling bulk OUT endpoint (CBW parsing + SCSI command dispatch)
        // 4. Handling bulk IN endpoint (data transfer + CSW)
        // 5. Implementing SCSI command handlers
        // 6. Managing virtual disk image file
        //
        // See src/server/usb/msc/CLAUDE.md for detailed implementation guide.

        let _ = status_tx.send(format!(
            "USB MSC device framework ready (SCSI implementation pending) - disk: {}",
            disk_path.display()
        ));

        error!(
            "USB MSC full implementation pending - requires custom UsbInterfaceHandler with BOT/SCSI support"
        );

        Err(anyhow::anyhow!(
            "USB MSC protocol not yet fully implemented - requires BOT/SCSI handler (see CLAUDE.md)"
        ))
    }
}
