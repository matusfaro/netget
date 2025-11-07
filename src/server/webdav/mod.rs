//! WebDAV server implementation using dav-server
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::server::WebDavProtocol;
use crate::state::app_state::AppState;

// WebDAV types
use dav_server::{fakels::FakeLs, memfs::MemFs, DavHandler};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;

/// WebDAV server that provides LLM-controlled file operations
pub struct WebDavServer;

impl WebDavServer {
    /// Spawn WebDAV server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        info!("WebDAV server (action-based) starting on {}", listen_addr);

        let _protocol = Arc::new(WebDavProtocol::new());

        // Create in-memory filesystem (LLM will control file operations)
        let memfs = MemFs::new();
        let dav_server = DavHandler::builder()
            .filesystem(memfs)
            .locksystem(FakeLs::new())
            .build_handler();

        let dav_server = Arc::new(dav_server);

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(listen_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind WebDAV listener: {}", e);
                    let _ = status_tx.send(format!("✗ WebDAV bind failed: {}", e));
                    return;
                }
            };

            let local_addr = listener.local_addr().unwrap();
            info!("WebDAV server listening on {}", local_addr);
            let _ = status_tx.send(format!("→ WebDAV server listening on {}", local_addr));

            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new();
                        debug!("WebDAV connection {} from {}", connection_id, peer_addr);
                        let _ =
                            status_tx.send(format!("[DEBUG] WebDAV connection from {}", peer_addr));

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let dav_clone = dav_server.clone();
                        let app_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        // Handle WebDAV connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Create service that uses DavHandler
                            let service = service_fn(move |req| {
                                let dav = dav_clone.clone();
                                async move { Ok::<_, std::convert::Infallible>(dav.handle(req).await) }
                            });

                            // Serve HTTP/1 WebDAV requests
                            if let Err(err) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                error!("WebDAV connection error: {:?}", err);
                            }

                            // Mark connection as closed
                            app_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone
                                .send(format!("✗ WebDAV connection {} closed", connection_id));
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept WebDAV connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(listen_addr)
    }
}
