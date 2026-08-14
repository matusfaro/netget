//! DNS-over-TLS (DoT) server implementation
//!
//! Implements RFC 7858 DNS-over-TLS protocol using hickory-dns and rustls.
//! The LLM controls DNS responses while NetGet handles the TLS transport layer.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
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
use tracing::error;

/// DNS-over-TLS server
pub struct DotServer;

impl DotServer {
    /// Spawn the DoT server.
    ///
    /// The listener is bound here, *before* the accept loop is spawned, so that
    /// a bind failure (port in use, permission denied) is returned to the
    /// caller instead of being swallowed by the background task - otherwise the
    /// server would be reported as `Running` while nothing is listening.
    /// Binding here also means the returned address carries the real port when
    /// the caller asked for port 0.
    pub async fn spawn(
        bind_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        // Generate TLS configuration (use default self-signed cert)
        let tls_config = crate::server::tls_cert_manager::generate_default_tls_config()
            .context("Failed to generate TLS configuration")?;

        Log::new(Some(&status_tx)).info(format!("Starting DoT server on {}", bind_addr));

        let listener = TcpListener::bind(bind_addr)
            .await
            .context("Failed to bind DoT TCP listener")?;

        // Actual bound address (important for port 0 dynamic allocation)
        let local_addr = listener
            .local_addr()
            .context("Failed to get DoT listener local address")?;

        Log::new(Some(&status_tx)).info(format!("DoT server listening on {}", local_addr));

        let task_registrar = app_state.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = Self::run(
                listener, tls_config, llm_client, app_state, server_id, status_tx,
            )
            .await
            {
                error!("DoT server error: {}", e);
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar.register_server_task(server_id, handle).await;

        Ok(local_addr)
    }

    /// Run the DoT accept loop on an already-bound listener
    async fn run(
        listener: TcpListener,
        tls_config: Arc<rustls::ServerConfig>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let acceptor = TlsAcceptor::from(tls_config);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    Log::new(Some(&status_tx))
                        .debug(format!("DoT TCP connection from {}", peer_addr));

                    let acceptor = acceptor.clone();
                    let llm_client = llm_client.clone();
                    let app_state = app_state.clone();
                    let status_tx = status_tx.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            stream, peer_addr, acceptor, llm_client, app_state, server_id,
                            status_tx,
                        )
                        .await
                        {
                            error!("DoT connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    Log::new(Some(&status_tx))
                        .warn(format!("Failed to accept DoT TCP connection: {}", e));
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

        Log::new(Some(&status_tx)).debug(format!("DoT TLS handshake complete with {}", peer_addr));

        Log::new(Some(&status_tx)).info(format!("DoT connection from {}", peer_addr));

        // Handle DNS queries over TLS
        loop {
            // Read length-prefixed DNS message (2-byte big-endian length)
            let mut len_buf = [0u8; 2];
            match tls_stream.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Log::new(Some(&status_tx))
                        .debug(format!("DoT connection from {} closed", peer_addr));
                    break;
                }
                Err(e) => {
                    Log::new(Some(&status_tx))
                        .error(format!("Failed to read DoT length prefix: {}", e));
                    break;
                }
            }

            let dns_len = u16::from_be_bytes(len_buf) as usize;

            if dns_len == 0 || dns_len > 65535 {
                Log::new(Some(&status_tx))
                    .warn(format!("Invalid DoT DNS message length: {}", dns_len));
                break;
            }

            // Read DNS message
            let mut dns_buf = vec![0u8; dns_len];
            if let Err(e) = tls_stream.read_exact(&mut dns_buf).await {
                Log::new(Some(&status_tx)).error(format!("Failed to read DoT DNS message: {}", e));
                break;
            }

            Log::new(Some(&status_tx))
                .debug(format!("DoT received {} bytes from {}", dns_len, peer_addr));

            // Parse DNS query
            let dns_message = match DnsMessage::from_vec(&dns_buf) {
                Ok(msg) => msg,
                Err(e) => {
                    Log::new(Some(&status_tx))
                        .warn(format!("Failed to parse DoT DNS message: {}", e));
                    continue;
                }
            };

            // Extract query information
            let queries = dns_message.queries();
            if queries.is_empty() {
                Log::new(Some(&status_tx)).warn("DoT DNS message has no queries");
                continue;
            }

            let query = &queries[0];
            let domain = query.name().to_utf8();
            let query_type = format!("{:?}", query.query_type());
            let query_id = dns_message.id();

            Log::new(Some(&status_tx)).info(format!(
                "DoT query: {} {} (ID: {})",
                domain, query_type, query_id
            ));

            Log::new(Some(&status_tx))
                .trace(format!("DoT DNS query hex: {}", hex::encode(&dns_buf)));

            // Create event for LLM
            let event = Event::new(
                &DOT_QUERY_EVENT,
                json!({
                    "query_id": query_id,
                    "domain": domain,
                    "query_type": query_type,
                    "peer_addr": peer_addr.to_string(),
                }),
            );

            // Get protocol actions
            let protocol = Arc::new(DotProtocol::new());

            Log::new(Some(&status_tx))
                .debug(format!("DoT calling LLM for query from {}", peer_addr));

            // Call LLM
            match call_llm(
                &llm_client,
                &app_state,
                server_id,
                None,
                &event,
                protocol.as_ref(),
            )
            .await
            {
                Ok(execution_result) => {
                    // Display messages from LLM
                    for message in &execution_result.messages {
                        Log::new(Some(&status_tx)).info(format!("{}", message));
                    }

                    Log::new(Some(&status_tx)).debug(format!(
                        "DoT got {} protocol results",
                        execution_result.protocol_results.len()
                    ));

                    // Execute actions from LLM response
                    for protocol_result in &execution_result.protocol_results {
                        use crate::llm::actions::protocol_trait::ActionResult;
                        match protocol_result {
                            ActionResult::Output(bytes) => {
                                // DNS action returned binary response directly
                                // Send length-prefixed response
                                let len = bytes.len() as u16;
                                let mut response = len.to_be_bytes().to_vec();
                                response.extend_from_slice(bytes);

                                if let Err(e) = tls_stream.write_all(&response).await {
                                    Log::new(Some(&status_tx))
                                        .error(format!("Failed to send DoT response: {}", e));
                                } else {
                                    let log = Log::new(Some(&status_tx));
                                    log.debug(format!("DoT sent {} bytes", bytes.len()));
                                    log.trace(format!("DoT response hex: {}", hex::encode(bytes)));
                                }
                            }
                            ActionResult::Custom { data, .. } => {
                                if let Some(output_data) =
                                    data.get("output_data").and_then(|v| v.as_str())
                                {
                                    // Decode hex DNS response
                                    if let Ok(response_bytes) = hex::decode(output_data) {
                                        // Send length-prefixed response
                                        let len = response_bytes.len() as u16;
                                        let mut response = len.to_be_bytes().to_vec();
                                        response.extend_from_slice(&response_bytes);

                                        if let Err(e) = tls_stream.write_all(&response).await {
                                            Log::new(Some(&status_tx)).error(format!(
                                                "Failed to send DoT response: {}",
                                                e
                                            ));
                                        } else {
                                            let log = Log::new(Some(&status_tx));
                                            log.debug(format!(
                                                "DoT sent {} bytes",
                                                response_bytes.len()
                                            ));
                                            log.trace(format!("DoT response hex: {}", output_data));
                                        }
                                    }
                                }
                            }
                            ActionResult::CloseConnection => {
                                Log::new(Some(&status_tx)).info(format!(
                                    "DoT connection from {} closed by LLM",
                                    peer_addr
                                ));
                                return Ok(());
                            }
                            ActionResult::NoAction => {
                                // Ignore query - don't send response
                                Log::new(Some(&status_tx)).debug("DoT query ignored by LLM");
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    // DoT is DNS: the client is a stub resolver blocked on an answer, and
                    // `continue` gave it nothing until its own timeout expired - the same
                    // defect plain DNS had, and worse here, because a TLS connection is
                    // expensive enough that resolvers pin one and serialise queries over it.
                    //
                    // SERVFAIL makes the resolver move on to another server at once. The
                    // query id and question section must be echoed or a stub resolver
                    // discards the packet as unsolicited and we are back to silence, so this
                    // reuses the DNS server's own builder rather than synthesising a header.
                    let overloaded = crate::llm::is_overload_error(&e);
                    Log::new(Some(&status_tx)).warn(format!(
                        "DoT answering SERVFAIL to {} (overload={}): {}",
                        peer_addr, overloaded, e
                    ));
                    match crate::server::dns::actions::build_servfail(&dns_message) {
                        Ok(packet) => {
                            // RFC 7858 uses the DNS-over-TCP framing: a two-byte length
                            // prefix in front of every message.
                            let mut framed = (packet.len() as u16).to_be_bytes().to_vec();
                            framed.extend_from_slice(&packet);
                            if let Err(send_err) = tls_stream.write_all(&framed).await {
                                Log::new(Some(&status_tx)).error(format!(
                                    "DoT failed to send SERVFAIL to {}: {}",
                                    peer_addr, send_err
                                ));
                            }
                        }
                        Err(build_err) => {
                            Log::new(Some(&status_tx)).error(format!(
                                "DoT failed to build SERVFAIL for {}: {}",
                                peer_addr, build_err
                            ));
                        }
                    }
                    continue;
                }
            }
        }

        // Connection closed
        Log::new(Some(&status_tx)).info(format!("DoT connection from {} closed", peer_addr));

        Ok(())
    }
}
