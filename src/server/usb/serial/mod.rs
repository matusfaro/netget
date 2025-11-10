//! USB CDC ACM Serial server implementation

pub mod actions;

#[cfg(feature = "usb-serial")]
use anyhow::Result;
#[cfg(feature = "usb-serial")]
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
#[cfg(feature = "usb-serial")]
use tokio::sync::{mpsc, Mutex};
#[cfg(feature = "usb-serial")]
use tracing::{error, info};

#[cfg(feature = "usb-serial")]
use crate::{llm::OllamaClient, server::connection::ConnectionId, state::app_state::AppState};

#[cfg(feature = "usb-serial")]
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

#[cfg(feature = "usb-serial")]
struct ConnectionData {
    state: ConnectionState,
    memory: String,
    line_coding: crate::server::usb::descriptors::LineCoding,
    control_lines: crate::server::usb::descriptors::ControlLineState,
}

#[cfg(feature = "usb-serial")]
pub struct UsbSerialServer;

#[cfg(feature = "usb-serial")]
impl UsbSerialServer {
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("USB Serial server listening on {}", local_addr);
        let _ = status_tx.send(format!("USB Serial server listening on {}", local_addr));

        let connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>> = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(crate::server::usb::serial::actions::UsbSerialProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        info!("USB/IP connection {} from {} (USB serial)", connection_id, remote_addr);

                        use crate::state::server::{ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: stream.local_addr().unwrap_or(local_addr),
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                "state": "WaitingForImport",
                                "baud_rate": 115200
                            })),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Placeholder connection handler
                        tokio::spawn(async move {
                            error!("USB serial connection {} placeholder - full USB/IP integration needed", connection_id);
                        });
                    }
                    Err(e) => error!("Failed to accept USB/IP connection: {}", e),
                }
            }
        });

        Ok(local_addr)
    }
}
