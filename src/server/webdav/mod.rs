//! WebDAV server built on `dav-server`.
//!
//! **This protocol is `DevelopmentState::Incomplete` and hidden from the LLM.** Every
//! request is answered by `dav_server::memfs::MemFs` — a real read/write filesystem living
//! in this process. The `OllamaClient` handed to `spawn_with_llm_actions` is dropped, no
//! event is ever raised, and the server instruction the user wrote is read by nobody. A
//! client PUTs a file and GETs it back because MemFs stored it, not because the model said
//! anything.
//!
//! That is a direct violation of the rule that protocols must not implement storage: the
//! LLM is supposed to supply every file, directory listing and property. See
//! `src/server/webdav/CLAUDE.md` and `src/server/webdav/actions.rs` for the full account,
//! and `src/server/nfs/` for what an LLM-backed filesystem looks like when it is done
//! properly.
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

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
        info!("WebDAV server starting on {}", listen_addr);
        let _ = status_tx.send(
            "[WARN] WebDAV is Incomplete: requests are served from an in-process in-memory \
             filesystem, the LLM is never consulted and the server instruction is ignored"
                .to_string(),
        );
        warn!(
            "WebDAV is Incomplete: MemFs answers every request, the LLM is never consulted \
             and the server instruction is ignored"
        );

        let _protocol = Arc::new(WebDavProtocol::new());

        // In-process read/write filesystem. This is the storage a protocol must not have; it
        // exists because there is no LLM-backed DavFileSystem yet. See the module docs.
        let memfs = MemFs::new();
        let dav_server = DavHandler::builder()
            .filesystem(memfs)
            .locksystem(FakeLs::new())
            .build_handler();

        let dav_server = Arc::new(dav_server);

        // Bind before spawning. Binding inside the accept task made every failure invisible:
        // the task logged and returned while spawn_with_llm_actions still answered Ok, so the
        // server sat in Running with no socket. It also meant port 0 was reported back to the
        // caller verbatim instead of the port the kernel actually chose.
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr)
            .await
            .with_context(|| format!("Failed to bind WebDAV listener on {}", listen_addr))?;
        let local_addr = listener.local_addr()?;

        info!("WebDAV server listening on {}", local_addr);
        let _ = status_tx.send(format!("→ WebDAV server listening on {}", local_addr));

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
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

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}
