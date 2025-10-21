//! UDP server implementation for raw UDP stack

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};

/// UDP server that manages UDP connections
pub struct UdpServer {
    socket: Arc<UdpSocket>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl UdpServer {
    /// Create a new UDP server listening on the specified address
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        let socket = Arc::new(socket);

        // Send listening event with the actual bound address
        let local_addr = socket.local_addr()?;
        event_tx.send(AppEvent::Network(NetworkEvent::Listening { addr: local_addr }))?;

        Ok(Self { socket, event_tx })
    }

    /// Start accepting and handling UDP datagrams
    pub async fn start(self) -> Result<()> {
        let mut buffer = vec![0u8; 65535]; // Maximum UDP datagram size
        let mut connections: HashMap<SocketAddr, ConnectionId> = HashMap::new();
        let socket = self.socket.clone();

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    // Get or create connection ID for this peer
                    let connection_id = connections.entry(peer_addr).or_insert_with(|| {
                        let id = ConnectionId::new();
                        // Send connection event for new peer
                        let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                            connection_id: id,
                            remote_addr: peer_addr,
                        }));
                        id
                    });

                    // Send data received event
                    let data = Bytes::copy_from_slice(&buffer[..n]);
                    let _ = self
                        .event_tx
                        .send(AppEvent::Network(NetworkEvent::DataReceived {
                            connection_id: *connection_id,
                            data,
                        }));
                }
                Err(e) => {
                    tracing::error!("UDP receive error: {}", e);
                    // Continue listening despite errors
                }
            }
        }
    }

    /// Send data to a specific peer
    pub async fn send_to(&self, data: &[u8], addr: SocketAddr) -> Result<()> {
        self.socket.send_to(data, addr).await?;
        Ok(())
    }

    /// Get the local address the server is bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }
}

/// Shared UDP socket for sending responses
pub type SharedUdpSocket = Arc<Mutex<Arc<UdpSocket>>>;

/// Map from connection ID to peer address for UDP responses
pub type UdpPeerMap = Arc<Mutex<HashMap<ConnectionId, SocketAddr>>>;