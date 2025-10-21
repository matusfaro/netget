//! TCP server implementation

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info};

use super::connection::ConnectionId;
use crate::events::types::NetworkEvent;

/// TCP server that listens for incoming connections
pub struct TcpServer {
    listener: Option<TcpListener>,
    local_addr: Option<SocketAddr>,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
}

impl TcpServer {
    /// Create a new TCP server (not yet listening)
    pub fn new(event_tx: mpsc::UnboundedSender<NetworkEvent>) -> Self {
        Self {
            listener: None,
            local_addr: None,
            event_tx,
        }
    }

    /// Start listening on the specified address
    pub async fn listen(&mut self, addr: impl Into<SocketAddr>) -> Result<()> {
        let addr = addr.into();
        let listener = TcpListener::bind(addr)
            .await
            .context("Failed to bind TCP listener")?;

        let local_addr = listener.local_addr()?;
        info!("TCP server listening on {}", local_addr);

        self.listener = Some(listener);
        self.local_addr = Some(local_addr);

        // Send listening event
        let _ = self.event_tx.send(NetworkEvent::Listening { addr: local_addr });

        Ok(())
    }

    /// Accept a new connection
    pub async fn accept(&mut self) -> Result<Option<(TcpStream, SocketAddr)>> {
        if let Some(listener) = &self.listener {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("Accepted connection from {}", addr);
                    Ok(Some((stream, addr)))
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    Err(e.into())
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get the local address the server is listening on
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Check if the server is listening
    pub fn is_listening(&self) -> bool {
        self.listener.is_some()
    }

    /// Close the server
    pub fn close(&mut self) {
        if self.listener.is_some() {
            info!("Closing TCP server");
            self.listener = None;
            self.local_addr = None;
        }
    }

    /// Spawn the TCP server for TUI mode with connection management
    pub async fn spawn_tui(
        listen_addr: SocketAddr,
        network_tx: mpsc::UnboundedSender<NetworkEvent>,
        connections: Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>>>>,
        cancellation_tokens: Arc<Mutex<HashMap<ConnectionId, oneshot::Sender<()>>>>,
    ) -> Result<()> {
        // Create and bind TCP server
        let mut tcp_server = TcpServer::new(network_tx.clone());
        tcp_server.listen(listen_addr).await?;

        // Send listening event
        let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match tcp_server.accept().await {
                    Ok(Some((stream, remote_addr))) => {
                        let connection_id = ConnectionId::new();

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));
                        connections.lock().await.insert(connection_id, write_half_arc);

                        // Create cancellation channel for this connection
                        let (cancel_tx, mut cancel_rx) = oneshot::channel();
                        cancellation_tokens.lock().await.insert(connection_id, cancel_tx);

                        // Send connected event
                        let _ = network_tx.send(NetworkEvent::Connected {
                            connection_id,
                            remote_addr,
                        });

                        // Spawn reader task with cancellation
                        let network_tx_inner = network_tx.clone();
                        tokio::spawn(async move {
                            use tokio::io::AsyncReadExt;
                            let mut buffer = vec![0u8; 8192];
                            let mut read_half = read_half;

                            loop {
                                tokio::select! {
                                    // Check for cancellation
                                    _ = &mut cancel_rx => {
                                        // Connection was explicitly closed
                                        let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                        break;
                                    }
                                    // Read data
                                    result = read_half.read(&mut buffer) => {
                                        match result {
                                            Ok(0) => {
                                                let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                                break;
                                            }
                                            Ok(n) => {
                                                let data = bytes::Bytes::copy_from_slice(&buffer[..n]);
                                                let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                                    connection_id,
                                                    data,
                                                });
                                            }
                                            Err(_) => break,
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }
}

/// Handle a TCP connection
pub async fn handle_connection(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    connection_id: ConnectionId,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    debug!("Handling connection {} from {}", connection_id, remote_addr);

    // Send connection established event
    let _ = event_tx.send(NetworkEvent::Connected {
        connection_id,
        remote_addr,
    });

    let mut buffer = vec![0u8; 8192];

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                // Connection closed
                info!("Connection {} closed by remote", connection_id);
                let _ = event_tx.send(NetworkEvent::Disconnected { connection_id });
                break;
            }
            Ok(n) => {
                let data = Bytes::copy_from_slice(&buffer[..n]);
                debug!(
                    "Received {} bytes from connection {}",
                    n, connection_id
                );

                // Send data received event
                let _ = event_tx.send(NetworkEvent::DataReceived {
                    connection_id,
                    data,
                });
            }
            Err(e) => {
                error!("Error reading from connection {}: {}", connection_id, e);
                let _ = event_tx.send(NetworkEvent::Error {
                    connection_id: Some(connection_id),
                    error: e.to_string(),
                });
                break;
            }
        }
    }

    Ok(())
}

/// Send data on a TCP connection
pub async fn send_data(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    stream
        .write_all(data)
        .await
        .context("Failed to write data")?;
    stream.flush().await.context("Failed to flush stream")?;
    Ok(())
}
