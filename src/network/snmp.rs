//! SNMP agent implementation - simplified UDP-based

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::info;

/// SNMP agent that forwards requests to LLM
pub struct SnmpAgent {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl SnmpAgent {
    /// Create a new SNMP agent
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the SNMP agent
    pub async fn start(self) -> Result<()> {
        // SNMP typically uses port 161
        let socket = UdpSocket::bind(self.addr).await?;
        info!("SNMP agent listening on {}", socket.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: socket.local_addr()?,
        }))?;

        let mut buffer = vec![0u8; 65535]; // SNMP messages can be large

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    // Create connection ID for this SNMP request
                    let connection_id = ConnectionId::new();

                    // Send connection event
                    let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                        connection_id,
                        remote_addr: peer_addr,
                    }));

                    // Parse basic SNMP info from the packet
                    // SNMP packets use BER encoding, complex to parse without library
                    // For now, just report the packet size and let the LLM handle it
                    let snmp_info = format!("SNMP Request: {} bytes from {}", n, peer_addr);

                    // Send data received event
                    let _ = self
                        .event_tx
                        .send(AppEvent::Network(NetworkEvent::DataReceived {
                            connection_id,
                            data: Bytes::from(snmp_info),
                        }));
                }
                Err(e) => {
                    tracing::error!("SNMP receive error: {}", e);
                }
            }
        }
    }
}