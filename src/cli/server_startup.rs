//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::events::NetworkEvent;
use crate::network::{ConnectionId, HttpServer, TcpServer};
use crate::protocol::BaseStack;
use crate::state::app_state::AppState;

type WriteHalfMap = Arc<Mutex<std::collections::HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;

/// Start a TCP server and spawn accept loop
type CancellationTokenMap = Arc<Mutex<std::collections::HashMap<crate::network::ConnectionId, tokio::sync::oneshot::Sender<()>>>>;

pub async fn start_tcp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
    connections: WriteHalfMap,
    cancellation_tokens: &CancellationTokenMap,
) -> Result<()> {
    // Create and bind TCP server
    let mut tcp_server = TcpServer::new(network_tx.clone());
    tcp_server.listen(listen_addr).await?;

    // Send listening event
    let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

    // Clone for the spawned task
    let cancellation_tokens = cancellation_tokens.clone();

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
                    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
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

/// Start an HTTP server
pub async fn start_http_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    let http_server = HttpServer::new(listen_addr, network_tx.clone()).await?;

    // Send listening event
    let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = http_server.accept_loop().await {
            eprintln!("HTTP server error: {}", e);
        }
    });

    Ok(())
}

/// Check if server needs to be started and start it
pub async fn check_and_start_server(
    state: &AppState,
    network_tx: &mpsc::UnboundedSender<NetworkEvent>,
    connections: &WriteHalfMap,
    cancellation_tokens: &CancellationTokenMap,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    use crate::state::app_state::Mode;

    // Check if we're in server mode and not yet listening
    if state.get_mode().await != Mode::Server {
        return Ok(());
    }

    if state.get_local_addr().await.is_some() {
        // Already listening
        return Ok(());
    }

    // Get port from state (set by OpenServer action)
    let port = state.get_port().await.unwrap_or(1234);
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    // Store the listen address
    state.set_local_addr(Some(listen_addr)).await;

    // Start server based on base stack
    let base_stack = state.get_base_stack().await;
    let msg = format!("Starting {} server on {}", base_stack, listen_addr);
    let _ = status_tx.send(msg.clone());

    match base_stack {
        BaseStack::TcpRaw => {
            start_tcp_server(listen_addr, network_tx.clone(), connections.clone(), cancellation_tokens).await?;
        }
        BaseStack::Http => {
            start_http_server(listen_addr, network_tx.clone()).await?;
        }
        BaseStack::DataLink => {
            let _ = status_tx.send("DataLink server not yet implemented in TUI".to_string());
        }
    }

    Ok(())
}

