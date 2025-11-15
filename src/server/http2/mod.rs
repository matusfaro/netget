//! HTTP/2 server implementation using hyper and h2
pub mod actions;
pub mod h2_server;
pub mod push;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http2;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::Http2Protocol;
use crate::state::app_state::AppState;
use actions::HTTP2_REQUEST_EVENT;

// Re-export for convenience
pub use h2_server::H2Server;

/// HTTP/2 server that delegates request handling to LLM
pub struct Http2Server;

impl Http2Server {
    /// Spawn the HTTP/2 server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        tls_config: Option<Arc<rustls::ServerConfig>>,
    ) -> anyhow::Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        let protocol_name = if tls_config.is_some() {
            "HTTP/2 (TLS)"
        } else {
            "HTTP/2 (h2c)"
        };
        info!(
            "{} server (action-based) listening on {}",
            protocol_name, local_addr
        );
        let _ = status_tx.send(format!("[INFO] {} server listening on {}", protocol_name, local_addr));

        let protocol = Arc::new(Http2Protocol::new());

        // Create TLS acceptor if TLS is enabled
        let tls_acceptor = tls_config.map(|config| tokio_rustls::TlsAcceptor::from(config));

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "Accepted {} connection {} from {}",
                            protocol_name, connection_id, remote_addr
                        );

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                "recent_requests": []
                            })),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let tls_acceptor_clone = tls_acceptor.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            // Perform TLS handshake if TLS is enabled
                            if let Some(acceptor) = tls_acceptor_clone {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        debug!(
                                            "{} TLS handshake complete with {}",
                                            protocol_name, remote_addr
                                        );
                                        let _ = status_tx_clone.send(format!(
                                            "[DEBUG] {} TLS handshake complete with {}",
                                            protocol_name, remote_addr
                                        ));
                                        let io = TokioIo::new(tls_stream);
                                        Self::serve_connection(
                                            io,
                                            connection_id,
                                            server_id,
                                            llm_client_clone,
                                            app_state_clone.clone(),
                                            status_tx_clone.clone(),
                                            protocol_clone,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        error!("{} TLS handshake failed: {}", protocol_name, e);
                                        let _ = status_tx_clone.send(format!(
                                            "[ERROR] {} TLS handshake failed: {}",
                                            protocol_name, e
                                        ));
                                    }
                                }
                            } else {
                                // No TLS, use plain TCP (h2c)
                                let io = TokioIo::new(stream);
                                Self::serve_connection(
                                    io,
                                    connection_id,
                                    server_id,
                                    llm_client_clone,
                                    app_state_clone.clone(),
                                    status_tx_clone.clone(),
                                    protocol_clone,
                                )
                                .await;
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send(format!(
                                "✗ {} connection {connection_id} closed",
                                protocol_name
                            ));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept HTTP/2 connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Serve an HTTP/2 connection (helper function to avoid code duplication)
    async fn serve_connection<T>(
        io: TokioIo<T>,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<Http2Protocol>,
    ) where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // Clone for service closure
        let status_for_service = status_tx.clone();
        let app_state_for_service = app_state.clone();

        // Create a service that handles requests with LLM
        let service = service_fn(move |req: Request<Incoming>| {
            let llm_clone = llm_client.clone();
            let state_clone = app_state_for_service.clone();
            let status_clone = status_for_service.clone();
            let protocol_clone = protocol.clone();
            handle_http2_request_with_llm_actions(
                req,
                connection_id,
                server_id,
                llm_clone,
                state_clone,
                status_clone,
                protocol_clone,
            )
        });

        // Serve HTTP/2 on this connection
        if let Err(err) = http2::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(io, service)
            .await
        {
            error!("Error serving HTTP/2 connection: {:?}", err);
        }
    }
}

/// Handle a single HTTP/2 request with integrated LLM actions
async fn handle_http2_request_with_llm_actions(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<Http2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Use shared request extraction logic
    let request_data =
        crate::server::http_common::handler::extract_request_data(req, "HTTP/2", &status_tx).await;

    // Create HTTP/2 request event (includes version field)
    let body_text = String::from_utf8_lossy(&request_data.body_bytes);
    let event = Event::new(
        &HTTP2_REQUEST_EVENT,
        serde_json::json!({
            "method": request_data.method,
            "uri": request_data.uri,
            "version": request_data.version,
            "headers": request_data.headers,
            "body": if body_text.is_empty() { "" } else { body_text.as_ref() }
        }),
    );

    // Call LLM to generate HTTP/2 response
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    )
    .await
    {
        Ok(execution_result) => {
            debug!("LLM HTTP/2 response received");

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Use shared response building logic
            crate::server::http_common::handler::build_response(
                execution_result.protocol_results,
                "HTTP/2",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
        Err(e) => {
            // Use shared error response building
            crate::server::http_common::handler::build_error_response(
                e,
                "HTTP/2",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
    }
}
