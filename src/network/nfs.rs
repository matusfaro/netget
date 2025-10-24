//! NFS server implementation using nfsserve

use crate::network::connection::ConnectionId;
use anyhow::{Result, Context};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, ProtocolActions};
use crate::network::NfsProtocol;
use crate::state::app_state::AppState;

/// NFS server that provides LLM-controlled file system
pub struct NfsServer;

impl NfsServer {
    /// Spawn NFS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        info!("NFS server (action-based) starting on {}", listen_addr);
        let _ = status_tx.send(format!("[INFO] NFS server starting on {}", listen_addr));

        let protocol = Arc::new(NfsProtocol::new());

        // Note: Full NFS implementation requires implementing nfsserve::vfs::NFSFileSystem trait
        // For now, we create a placeholder that can be extended with LLM-controlled filesystem

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(listen_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind NFS listener: {}", e);
                    let _ = status_tx.send(format!("✗ NFS bind failed: {}", e));
                    return;
                }
            };

            let local_addr = listener.local_addr().unwrap();
            info!("NFS server listening on {}", local_addr);
            let _ = status_tx.send(format!("→ NFS server listening on {}", local_addr));

            // NFS typically uses both TCP (port 2049) and UDP, plus RPC portmapper
            // For simplicity, we'll handle TCP connections here
            // Full implementation would require:
            // 1. RPC portmapper registration
            // 2. MOUNT protocol (port 20048)
            // 3. NFS protocol (port 2049)
            // 4. Implementing NFSFileSystem trait with LLM-backed operations

            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new();
                        debug!("NFS connection {} from {}", connection_id, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] NFS connection from {}", peer_addr));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Nfs {
                                mounted_paths: Vec::new(),
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let app_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        // Handle NFS connection
                        tokio::spawn(async move {
                            debug!("NFS connection {} established", connection_id);
                            let _ = status_clone.send(format!("→ NFS connection {}", connection_id));

                            // TODO: Implement NFS RPC protocol handling
                            // This requires:
                            // 1. XDR decoding of RPC messages
                            // 2. NFS procedure call handling (LOOKUP, READ, WRITE, etc.)
                            // 3. LLM integration for file system operations
                            // 4. XDR encoding of responses

                            // For now, just hold the connection
                            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                            // Mark connection as closed
                            app_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_clone.send(format!("✗ NFS connection {} closed", connection_id));
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept NFS connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(listen_addr)
    }
}

// Future enhancement: Implement NFSFileSystem trait for LLM-backed filesystem
// use nfsserve::vfs::NFSFileSystem;
//
// struct LlmNfsFileSystem {
//     llm_client: OllamaClient,
//     app_state: Arc<AppState>,
// }
//
// impl NFSFileSystem for LlmNfsFileSystem {
//     // Implement required methods: lookup, getattr, read, write, etc.
//     // Each method would consult the LLM for how to respond
// }
