//! Tor Directory server implementation
pub mod actions;
pub mod authority_keys;
pub mod consensus_signer;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[cfg(feature = "tor")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "tor")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "tor")]
use crate::llm::ActionResult;
#[cfg(feature = "tor")]
use actions::TOR_DIRECTORY_REQUEST_EVENT;
#[cfg(feature = "tor")]
use crate::server::TorDirectoryProtocol;
#[cfg(feature = "tor")]
use crate::protocol::Event;
#[cfg(feature = "tor")]
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Tor Directory server that serves consensus and descriptors
pub struct TorDirectoryServer;

/// Authority keys for the directory (generated on startup)
static AUTHORITY_KEYS: once_cell::sync::Lazy<authority_keys::AuthorityKeys> =
    once_cell::sync::Lazy::new(|| {
        authority_keys::AuthorityKeys::generate()
            .expect("Failed to generate authority keys")
    });

#[cfg(feature = "tor")]
impl TorDirectoryServer {
    /// Spawn Tor Directory server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        // Log authority fingerprints for test configuration
        let v3_ident = AUTHORITY_KEYS.v3_identity_fingerprint();
        let fingerprint = AUTHORITY_KEYS.authority_fingerprint();

        info!("Tor Directory server (action-based) listening on {}", local_addr);
        info!("Authority v3 identity fingerprint: {}", v3_ident);

        console_info!(status_tx, "[INFO] Tor Directory server listening on {}", local_addr);
        console_info!(status_tx, "[INFO] Authority v3 identity fingerprint: {}", v3_ident);
        console_info!(status_tx, "[INFO] Authority fingerprint: {}", fingerprint);

        let protocol = Arc::new(TorDirectoryProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await
                        );
                        console_debug!(status_tx, "[DEBUG] Tor Directory connection {} from {}", connection_id, remote_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let mut session = TorDirectorySession {
                                stream,
                                connection_id,
                                server_id,
                                remote_addr,
                                llm_client: llm_clone.clone(),
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone.clone(),
                            };

                            // Handle Tor Directory HTTP session
                            if let Err(e) = session.handle().await {
                                error!("Tor Directory session error: {}", e);
                                let _ = status_clone.send(format!("[ERROR] Tor Directory session error: {}", e));
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Tor Directory connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(feature = "tor")]
struct TorDirectorySession {
    stream: tokio::net::TcpStream,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<TorDirectoryProtocol>,
}

#[cfg(feature = "tor")]
impl TorDirectorySession {
    async fn handle(&mut self) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (read_half, mut write_half) = tokio::io::split(&mut self.stream);
        let mut reader = BufReader::new(read_half);

        // Read HTTP request line
        let mut request_line = String::new();
        let n = reader.read_line(&mut request_line).await?;
        if n == 0 {
            return Ok(());
        }

        // Parse HTTP request: "GET /tor/status-vote/current/consensus HTTP/1.1"
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            console_debug!(self.status_tx, "[DEBUG] Tor Directory malformed request from {}", self.remote_addr);

            // Send 400 Bad Request
            let error_response = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
            write_half.write_all(error_response).await?;
            write_half.flush().await?;
            return Ok(());
        }

        let method = parts[0];
        let path = parts[1];

        console_debug!(self.status_tx, "[DEBUG] Tor Directory {} {} from {}", method, path, self.remote_addr);

        // Read remaining headers (but we don't need to parse them for now)
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 || line == "\r\n" || line == "\n" {
                break; // End of headers
            }
        }

        // Create Tor Directory request event
        let event = Event::new(&TOR_DIRECTORY_REQUEST_EVENT, serde_json::json!({
            "path": path,
            "client_ip": self.remote_addr.ip().to_string(),
            "method": method,
        }));

        // Get LLM response
        if let Ok(execution_result) = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        ).await {
            for protocol_result in execution_result.protocol_results {
                match protocol_result {
                    ActionResult::Output(data) => {
                        write_half.write_all(&data).await?;
                        write_half.flush().await?;

                        console_debug!(self.status_tx, "[DEBUG] Tor Directory sent {} bytes", data.len());
                    }
                    ActionResult::CloseConnection => {
                        debug!("Tor Directory closing connection");
                        return Ok(());
                    }
                    _ => {}
                }
            }
        } else {
            // If LLM call fails, send 500 error
            debug!("Tor Directory LLM call failed, sending 500");
            let error_response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
            write_half.write_all(error_response).await?;
            write_half.flush().await?;
        }

        Ok(())
    }
}

#[cfg(not(feature = "tor"))]
impl TorDirectoryServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("Tor Directory feature not enabled")
    }
}
