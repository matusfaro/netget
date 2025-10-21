//! DNS server implementation - simplified UDP-based

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::info;

/// DNS server that forwards queries to LLM
pub struct DnsServer {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl DnsServer {
    /// Create a new DNS server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the DNS server
    pub async fn start(self) -> Result<()> {
        // DNS typically uses port 53
        let socket = UdpSocket::bind(self.addr).await?;
        info!("DNS server listening on {}", socket.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: socket.local_addr()?,
        }))?;

        let mut buffer = vec![0u8; 512]; // Standard DNS packet size

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    // Create connection ID for this DNS query
                    let connection_id = ConnectionId::new();

                    // Send connection event
                    let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                        connection_id,
                        remote_addr: peer_addr,
                    }));

                    // Parse DNS query header to get basic info
                    let query_info = if n >= 12 {
                        // DNS header is at least 12 bytes
                        let id = u16::from_be_bytes([buffer[0], buffer[1]]);
                        let flags = u16::from_be_bytes([buffer[2], buffer[3]]);
                        let qd_count = u16::from_be_bytes([buffer[4], buffer[5]]);
                        let an_count = u16::from_be_bytes([buffer[6], buffer[7]]);

                        format!(
                            "DNS Query: ID={}, Flags=0x{:04x}, Questions={}, Answers={}",
                            id, flags, qd_count, an_count
                        )
                    } else {
                        format!("DNS Query: {} bytes (too small for valid DNS)", n)
                    };

                    // Send data received event
                    let _ = self
                        .event_tx
                        .send(AppEvent::Network(NetworkEvent::DataReceived {
                            connection_id,
                            data: Bytes::from(query_info),
                        }));
                }
                Err(e) => {
                    tracing::error!("DNS receive error: {}", e);
                }
            }
        }
    }
}