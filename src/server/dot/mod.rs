//! DNS-over-TLS (DoT) server implementation
//!
//! Implements RFC 7858 DNS-over-TLS protocol using hickory-dns and rustls.
//! The LLM controls DNS responses while NetGet handles the TLS transport layer.

pub mod actions;

use crate::protocol::Event;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::action_helper::call_llm;
use crate::server::DotProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use actions::DOT_QUERY_EVENT;
use anyhow::{Context, Result};
use hickory_proto::op::Message as DnsMessage;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, trace, warn};

/// DNS-over-TLS server
pub struct DotServer {
    bind_addr: SocketAddr,
}

impl DotServer {
    /// Create a new DoT server
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self { bind_addr }
    }

    /// Spawn the DoT server
    pub async fn spawn(
        bind_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let server = Self::new(bind_addr);

        // Generate TLS configuration (use default self-signed cert)
        let tls_config = crate::server::tls_cert_manager::generate_default_tls_config()
            .context("Failed to generate TLS configuration")?;

        console_info!(status_tx, "[INFO] Starting DoT server on {}", bind_addr);

        let handle = tokio::spawn(async move {
            if let Err(e) = server.run(tls_config, llm_client, app_state, server_id, status_tx).await {
                error!("DoT server error: {}", e);
            }
        });

        Ok(handle)
    }

    /// Run the DoT server
    async fn run(
        self,
        tls_config: Arc<rustls::ServerConfig>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.bind_addr)
            .await
            .context("Failed to bind DoT TCP listener")?;

        let acceptor = TlsAcceptor::from(tls_config);

        console_info!(status_tx, "[INFO] DoT server listening on {}", self.bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    console_debug!(status_tx, "[DEBUG] DoT TCP connection from {}", peer_addr);

                    let acceptor = acceptor.clone();
                    let llm_client = llm_client.clone();
                    let app_state = app_state.clone();
                    let status_tx = status_tx.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            stream,
                            peer_addr,
                            acceptor,
                            llm_client,
                            app_state,
                            server_id,
                            status_tx,
                        )
                        .await
                        {
                            error!("DoT connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    console_warn!(status_tx, "[WARN] Failed to accept DoT TCP connection: {}", e);
                }
            }
        }
    }

    /// Handle a single DoT connection
    async fn handle_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        acceptor: TlsAcceptor,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Perform TLS handshake
        let mut tls_stream = acceptor
            .accept(stream)
            .await
            .context("TLS handshake failed")?;

        console_debug!(status_tx, "[DEBUG] DoT TLS handshake complete with {}", peer_addr);

        console_info!(status_tx, "[INFO] DoT connection from {}", peer_addr);

        // Handle DNS queries over TLS
        loop {
            // Read length-prefixed DNS message (2-byte big-endian length)
            let mut len_buf = [0u8; 2];
            match tls_stream.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    console_debug!(status_tx, "[DEBUG] DoT connection from {} closed", peer_addr);
                    break;
                }
                Err(e) => {
                    console_error!(status_tx, "[ERROR] Failed to read DoT length prefix: {}", e);
                    break;
                }
            }

            let dns_len = u16::from_be_bytes(len_buf) as usize;

            if dns_len == 0 || dns_len > 65535 {
                console_warn!(status_tx, "[WARN] Invalid DoT DNS message length: {}", dns_len);
                break;
            }

            // Read DNS message
            let mut dns_buf = vec![0u8; dns_len];
            if let Err(e) = tls_stream.read_exact(&mut dns_buf).await {
                console_error!(status_tx, "[ERROR] Failed to read DoT DNS message: {}", e);
                break;
            }

            console_debug!(status_tx, "[DEBUG] DoT received {} bytes from {}", dns_len, peer_addr);

            // Parse DNS query
            let dns_message = match DnsMessage::from_vec(&dns_buf) {
                Ok(msg) => msg,
                Err(e) => {
                    console_error!(status_tx, "[ERROR] Failed to parse DoT DNS message: {}", e);
                    continue;
                }
            };

            // Extract query information
            let queries = dns_message.queries();
            if queries.is_empty() {
                console_warn!(status_tx, "[WARN] DoT DNS message has no queries");
                continue;
            }

            let query = &queries[0];
            let domain = query.name().to_utf8();
            let query_type = format!("{:?}", query.query_type());
            let query_id = dns_message.id();

            console_info!(status_tx, "[INFO] DoT query: {} {} (ID: {})", domain, query_type, query_id);

            console_trace!(status_tx, "[TRACE] DoT DNS query hex: {}", hex::encode(&dns_buf));

            // Create event for LLM
            let event = Event::new(&DOT_QUERY_EVENT, json!({
                "query_id": query_id,
                "domain": domain,
                "query_type": query_type,
                "peer_addr": peer_addr.to_string(),
            }));

            // Get protocol actions
            let protocol = Arc::new(DotProtocol::new());

            console_debug!(status_tx, "[DEBUG] DoT calling LLM for query from {}", peer_addr);

            // Call LLM
            match call_llm(
                &llm_client,
                &app_state,
                server_id,
                None,
                &event,
                protocol.as_ref(),
            ).await {
                Ok(execution_result) => {
                    // Display messages from LLM
                    for message in &execution_result.messages {
                        console_info!(status_tx, "[INFO] {}", message);
                    }

                    console_debug!(status_tx, "[DEBUG] DoT got {} protocol results", execution_result.protocol_results.len());

                    // Execute actions from LLM response
                    for protocol_result in &execution_result.protocol_results {
                        use crate::llm::actions::protocol_trait::ActionResult;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                        match protocol_result {
                            ActionResult::Output(bytes) => {
                                // DNS action returned binary response directly
                                // Send length-prefixed response
                                let len = bytes.len() as u16;
                                let mut response = len.to_be_bytes().to_vec();
                                response.extend_from_slice(bytes);

                                if let Err(e) = tls_stream.write_all(&response).await {
                                    console_error!(status_tx, "[ERROR] Failed to send DoT response: {}", e);
                                } else {
                                    console_debug!(status_tx, "[DEBUG] DoT sent {} bytes", bytes.len());

                                    console_trace!(status_tx, "[TRACE] DoT response hex: {}", hex::encode(bytes));
                                }
                            }
                            ActionResult::Custom { data, .. } => {
                                if let Some(output_data) = data.get("output_data").and_then(|v| v.as_str()) {
                                    // Decode hex DNS response
                                    if let Ok(response_bytes) = hex::decode(output_data) {
                                        // Send length-prefixed response
                                        let len = response_bytes.len() as u16;
                                        let mut response = len.to_be_bytes().to_vec();
                                        response.extend_from_slice(&response_bytes);

                                        if let Err(e) = tls_stream.write_all(&response).await {
                                            console_error!(status_tx, "[ERROR] Failed to send DoT response: {}", e);
                                        } else {
                                            console_debug!(status_tx, "[DEBUG] DoT sent {} bytes", response_bytes.len());

                                            console_trace!(status_tx, "[TRACE] DoT response hex: {}", output_data);
                                        }
                                    }
                                }
                            }
                            ActionResult::CloseConnection => {
                                console_info!(status_tx, "[INFO] DoT connection from {} closed by LLM", peer_addr);
                                return Ok(());
                            }
                            ActionResult::NoAction => {
                                // Ignore query - don't send response
                                console_debug!(status_tx, "[DEBUG] DoT query ignored by LLM");
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "[ERROR] DoT LLM call failed: {}", e);
                    continue;
                }
            }
        }

        // Connection closed
        console_info!(status_tx, "[INFO] DoT connection from {} closed", peer_addr);

        Ok(())
    }
}
