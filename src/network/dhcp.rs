//! DHCP server implementation - simplified UDP-based

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::info;

/// DHCP server that forwards requests to LLM
pub struct DhcpServer {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl DhcpServer {
    /// Create a new DHCP server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the DHCP server
    pub async fn start(self) -> Result<()> {
        // DHCP typically uses ports 67 (server) and 68 (client)
        let socket = UdpSocket::bind(self.addr).await?;
        info!("DHCP server listening on {}", socket.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: socket.local_addr()?,
        }))?;

        let mut buffer = vec![0u8; 1500]; // Standard MTU size

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    // Create connection ID for this DHCP transaction
                    let connection_id = ConnectionId::new();

                    // Send connection event
                    let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                        connection_id,
                        remote_addr: peer_addr,
                    }));

                    // Parse basic DHCP info
                    // DHCP has a specific packet format, but for simplicity,
                    // just check for some basic indicators
                    let dhcp_info = if n >= 240 {
                        // Minimal DHCP packet is 240 bytes (without options)
                        let op = buffer[0]; // 1 = request, 2 = reply
                        let _htype = buffer[1]; // Hardware type (1 = Ethernet)
                        let xid = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

                        let op_str = if op == 1 { "REQUEST" } else if op == 2 { "REPLY" } else { "UNKNOWN" };

                        format!("DHCP {}: Transaction ID 0x{:08x}, {} bytes", op_str, xid, n)
                    } else {
                        format!("DHCP packet: {} bytes (too small)", n)
                    };

                    // Send data received event
                    let _ = self
                        .event_tx
                        .send(AppEvent::Network(NetworkEvent::DataReceived {
                            connection_id,
                            data: Bytes::from(dhcp_info),
                        }));
                }
                Err(e) => {
                    tracing::error!("DHCP receive error: {}", e);
                }
            }
        }
    }
}