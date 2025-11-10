//! USB Mass Storage Class (MSC) server implementation
//!
//! This module implements a virtual USB Mass Storage device using the USB/IP protocol.
//! The device uses Bulk-Only Transport (BOT) and SCSI transparent command set to expose
//! a virtual disk that can be mounted by the host operating system.

pub mod actions;

// Re-export protocol struct for registration
#[cfg(feature = "usb-msc")]
pub use actions::UsbMscProtocol;

#[cfg(feature = "usb-msc")]
mod disk;

#[cfg(feature = "usb-msc")]
mod handler;

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
use tokio::sync::{mpsc, Mutex, RwLock};
#[cfg(feature = "usb-msc")]
use tracing::{debug, error, info, warn};

#[cfg(feature = "usb-msc")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "usb-msc")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-msc")]
use crate::protocol::Event;
#[cfg(feature = "usb-msc")]
use crate::server::connection::ConnectionId;
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
#[derive(Clone)]
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
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
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
        let disk_path = disk_image.unwrap_or_else(|| PathBuf::from("./tmp/netget_msc_disk.img"));
        let disk_size_mb = 10; // Default 10MB disk
        let write_protect = true; // Start write-protected for safety

        // Create or open disk image
        let disk_image_obj = Arc::new(RwLock::new(
            disk::DiskImage::open_or_create(&disk_path, disk_size_mb)
                .context("Failed to create disk image")?,
        ));

        let total_sectors = disk_image_obj.read().await.total_sectors();
        let bytes_per_sector = disk_image_obj.read().await.bytes_per_sector();

        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                memory: String::new(),
                disk_path: disk_path.clone(),
                write_protect,
                total_sectors,
                bytes_per_sector,
            },
        );

        info!(
            "USB MSC device: disk={}, size={}MB, sectors={}, write_protect={}",
            disk_path.display(),
            disk_size_mb,
            total_sectors,
            write_protect
        );

        // Create MSC handler with BOT protocol and SCSI support
        let handler = Arc::new(std::sync::Mutex::new(
            Box::new(handler::UsbMscHandler::new(
                disk_image_obj.clone(),
                write_protect,
            )) as Box<dyn usbip::UsbInterfaceHandler + Send>,
        ));

        // Store handler in protocol for LLM action execution
        protocol.set_handler(connection_id, handler.clone()).await;

        // Create USB device with MSC interface
        let device = usbip::UsbDevice::new(0) // Bus 0
            .with_interface(
                0x08, // Mass Storage Class
                0x06, // SCSI Transparent Command Set
                0x50, // Bulk-Only Transport
                "NetGet Virtual Disk",
                vec![
                    usbip::UsbEndpoint {
                        address: 0x81,         // EP1 IN (Bulk)
                        attributes: 0x02,      // Bulk transfer
                        max_packet_size: 512,  // 512 bytes
                        interval: 0,           // Not used for bulk
                    },
                    usbip::UsbEndpoint {
                        address: 0x02,         // EP2 OUT (Bulk)
                        attributes: 0x02,      // Bulk transfer
                        max_packet_size: 512,  // 512 bytes
                        interval: 0,           // Not used for bulk
                    },
                ],
                handler.clone(),
            );

        // Get local address for USB/IP server
        let usbip_addr = remote_addr; // Use same address as TCP connection

        // Create and spawn USB/IP server (not wrapped in Arc - usbip::server takes ownership)
        let server = usbip::UsbIpServer::new_simulated(vec![device]);
        tokio::spawn(async move {
            usbip::server(usbip_addr, server).await;
            debug!("USB/IP server task completed for MSC connection");
        });

        info!(
            "USB MSC device exported via USB/IP on {} (connection {})",
            usbip_addr, connection_id
        );
        let _ = status_tx.send(format!(
            "USB MSC device ready: {} ({} MB, {} sectors)",
            disk_path.display(),
            disk_size_mb,
            total_sectors
        ));

        // Call LLM to notify device attached
        Self::call_llm_on_attach(
            connection_id,
            remote_addr,
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            connections.clone(),
            protocol.clone(),
            server_id,
        )
        .await?;

        // Keep connection alive (USB/IP server runs in background)
        tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;

        Ok(())
    }

    /// Call LLM when device is attached
    async fn call_llm_on_attach(
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<crate::server::usb::msc::UsbMscProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        info!(
            "USB MSC device attached for connection {} from {}",
            connection_id, remote_addr
        );

        // Create event for device attachment
        let event = Event::new(
            &USB_MSC_ATTACHED_EVENT,
            serde_json::json!({
                "connection_id": connection_id.to_string(),
                "remote_addr": remote_addr.to_string(),
            }),
        );

        // Get connection data to check state
        let conn_data = {
            let conns = connections.lock().await;
            conns.get(&connection_id).cloned()
        };

        if let Some(mut conn_data) = conn_data {
            // Check if already processing
            if conn_data.state != ConnectionState::Idle {
                debug!(
                    "Connection {} not idle (state: {:?}), skipping LLM call",
                    connection_id, conn_data.state
                );
                return Ok(());
            }

            // Mark as processing
            conn_data.state = ConnectionState::Processing;
            connections.lock().await.insert(connection_id, conn_data.clone());

            // Call LLM
            match call_llm(
                &llm_client,
                &app_state,
                server_id,
                Some(connection_id),
                &event,
                protocol.as_ref(),
            )
            .await
            {
                Ok(_execution_result) => {
                    // Actions have already been executed by call_llm
                    info!(
                        "USB MSC LLM call completed for connection {}",
                        connection_id
                    );

                    // Mark as idle
                    conn_data.state = ConnectionState::Idle;
                    connections.lock().await.insert(connection_id, conn_data);

                    Ok(())
                }
                Err(e) => {
                    error!("LLM call failed for connection {}: {}", connection_id, e);
                    // Mark as idle even on error
                    conn_data.state = ConnectionState::Idle;
                    connections.lock().await.insert(connection_id, conn_data);
                    Err(e)
                }
            }
        } else {
            warn!("Connection {} not found", connection_id);
            Ok(())
        }
    }
}
