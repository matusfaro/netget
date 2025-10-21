//! SSH server implementation - simplified

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::info;

/// SSH server that forwards sessions to LLM
pub struct SshServer {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl SshServer {
    /// Create a new SSH server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the SSH server
    pub async fn start(self) -> Result<()> {
        // For now, implement a simple TCP listener that reports SSH connections
        // Full SSH implementation with russh would require more complex setup
        let listener = TcpListener::bind(self.addr).await?;
        info!("SSH server listening on {}", listener.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: listener.local_addr()?,
        }))?;

        loop {
            match listener.accept().await {
                Ok((mut stream, peer_addr)) => {
                    let connection_id = ConnectionId::new();
                    let event_tx = self.event_tx.clone();

                    // Send connection event
                    let _ = event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                        connection_id,
                        remote_addr: peer_addr,
                    }));

                    // Spawn task to handle this SSH connection
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buffer = vec![0u8; 1024];

                        // Read SSH client hello
                        match stream.read(&mut buffer).await {
                            Ok(n) if n > 0 => {
                                // SSH typically starts with "SSH-" protocol version
                                let data = String::from_utf8_lossy(&buffer[..n]);
                                let ssh_info = if data.starts_with("SSH-") {
                                    format!("SSH Client Hello: {}", data.trim())
                                } else {
                                    format!("SSH Connection: {} bytes", n)
                                };

                                // Send data received event
                                let _ = event_tx.send(AppEvent::Network(NetworkEvent::DataReceived {
                                    connection_id,
                                    data: Bytes::from(ssh_info),
                                }));
                            }
                            _ => {
                                // Connection closed or error
                                let _ = event_tx.send(AppEvent::Network(NetworkEvent::Disconnected {
                                    connection_id,
                                }));
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept SSH connection: {}", e);
                }
            }
        }
    }
}