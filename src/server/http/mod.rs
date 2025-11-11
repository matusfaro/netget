//! HTTP server implementation using hyper
pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::HttpProtocol;
use crate::state::app_state::AppState;
use actions::HTTP_REQUEST_EVENT;

/// HTTP server that delegates request handling to LLM
pub struct HttpServer;

impl HttpServer {
    /// Spawn the HTTP server with integrated LLM actions
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
            "HTTPS"
        } else {
            "HTTP"
        };
        info!(
            "{} server (action-based) listening on {}",
            protocol_name, local_addr
        );

        let protocol = Arc::new(HttpProtocol::new());

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
                                // No TLS, use plain TCP
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
                        error!("Failed to accept HTTP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Serve an HTTP connection (helper function to avoid code duplication)
    async fn serve_connection<T>(
        io: TokioIo<T>,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<HttpProtocol>,
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
            handle_http_request_with_llm_actions(
                req,
                connection_id,
                server_id,
                llm_clone,
                state_clone,
                status_clone,
                protocol_clone,
            )
        });

        // Serve HTTP/1 on this connection with upgrade support
        if let Err(err) = http1::Builder::new()
            .serve_connection(io, service)
            .with_upgrades()
            .await
        {
            error!("Error serving HTTP connection: {:?}", err);
        }
    }
}

/// Handle a single HTTP request with integrated LLM actions
async fn handle_http_request_with_llm_actions(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<HttpProtocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Check for HTTP/2 upgrade request (h2c) - only when http2 feature is enabled
    #[cfg(feature = "http2")]
    {
        if let Some(upgrade_header) = req.headers().get(hyper::header::UPGRADE) {
            if let Ok(upgrade_value) = upgrade_header.to_str() {
                if upgrade_value.contains("h2c") {
                    info!(
                        "HTTP/2 upgrade (h2c) request detected from connection {}",
                        connection_id
                    );
                    let _ = status_tx.send(format!("[INFO] HTTP/2 upgrade (h2c) requested"));

                    // Check for HTTP2-Settings header (required for h2c upgrade)
                    if req.headers().get("HTTP2-Settings").is_none() {
                        let response = Response::builder()
                            .status(400) // Bad Request
                            .body(Full::new(Bytes::from(
                                "HTTP/2 upgrade requires HTTP2-Settings header",
                            )))
                            .unwrap();
                        return Ok(response);
                    }

                    // Spawn task to handle upgrade after 101 response
                    let llm_clone = llm_client.clone();
                    let app_state_clone = app_state.clone();
                    let status_tx_clone = status_tx.clone();
                    let protocol_clone = protocol.clone();

                    tokio::spawn(async move {
                        // Wait for upgrade to complete
                        match hyper::upgrade::on(req).await {
                            Ok(upgraded) => {
                                info!("HTTP/2 upgrade successful for connection {}", connection_id);
                                let _ = status_tx_clone.send(format!(
                                    "[INFO] Upgraded connection {} to HTTP/2",
                                    connection_id
                                ));

                                // Perform h2 handshake on the upgraded connection
                                use hyper_util::rt::TokioIo;
                                let io = TokioIo::new(upgraded);

                                // Use h2 server to handle the upgraded connection
                                if let Err(e) = handle_upgraded_h2c_connection(
                                    io,
                                    connection_id,
                                    server_id,
                                    llm_clone,
                                    app_state_clone,
                                    status_tx_clone,
                                    protocol_clone,
                                )
                                .await
                                {
                                    error!("Error handling upgraded h2c connection: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("HTTP/2 upgrade failed: {}", e);
                                let _ = status_tx_clone
                                    .send(format!("[ERROR] HTTP/2 upgrade failed: {}", e));
                            }
                        }
                    });

                    // Return 101 Switching Protocols
                    let response = Response::builder()
                        .status(101) // 101 Switching Protocols
                        .header(hyper::header::UPGRADE, "h2c")
                        .header(hyper::header::CONNECTION, "Upgrade")
                        .body(Full::new(Bytes::new()))
                        .unwrap();

                    return Ok(response);
                }
            }
        }
    }

    // If http2 feature is not enabled, reject upgrade requests
    #[cfg(not(feature = "http2"))]
    {
        if let Some(upgrade_header) = req.headers().get(hyper::header::UPGRADE) {
            if let Ok(upgrade_value) = upgrade_header.to_str() {
                if upgrade_value.contains("h2c") {
                    info!("HTTP/2 upgrade requested but http2 feature not enabled");
                    let _ = status_tx.send(
                        "[INFO] HTTP/2 upgrade not supported (http2 feature disabled)".to_string(),
                    );

                    let response = Response::builder()
                        .status(501) // Not Implemented
                        .body(Full::new(Bytes::from(
                            "HTTP/2 upgrade not supported. Server built without http2 feature.",
                        )))
                        .unwrap();

                    return Ok(response);
                }
            }
        }
    }

    // Use shared request extraction logic
    let request_data =
        crate::server::http_common::handler::extract_request_data(req, "HTTP", &status_tx).await;

    // Create HTTP request event (no version field for HTTP/1.1)
    let body_text = String::from_utf8_lossy(&request_data.body_bytes);
    let event = Event::new(
        &HTTP_REQUEST_EVENT,
        serde_json::json!({
            "method": request_data.method,
            "uri": request_data.uri,
            "headers": request_data.headers,
            "body": if body_text.is_empty() { "" } else { body_text.as_ref() }
        }),
    );

    // Call LLM to generate HTTP response
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
            debug!("LLM HTTP response received");

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Use shared response building logic
            crate::server::http_common::handler::build_response(
                execution_result.protocol_results,
                "HTTP",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
        Err(e) => {
            // Use shared error response building
            crate::server::http_common::handler::build_error_response(
                e,
                "HTTP",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
    }
}

/// Handle an upgraded h2c connection (only available with http2 feature)
#[cfg(feature = "http2")]
async fn handle_upgraded_h2c_connection<T>(
    io: T,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    _protocol: Arc<HttpProtocol>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use crate::server::Http2Protocol;
    use h2::server;

    info!("Starting h2c connection for {}", connection_id);

    // Perform h2 server handshake
    let mut h2_conn = server::handshake(io).await?;

    let protocol = Arc::new(Http2Protocol::new());

    // Accept requests on the h2 connection
    loop {
        match h2_conn.accept().await {
            Some(result) => {
                let (request, send_response) = result?;

                let llm_clone = llm_client.clone();
                let app_state_clone = app_state.clone();
                let status_tx_clone = status_tx.clone();
                let protocol_clone = protocol.clone();

                // Spawn task to handle this HTTP/2 request
                tokio::spawn(async move {
                    if let Err(e) = crate::server::http2::h2_server::handle_h2_request(
                        request,
                        send_response,
                        connection_id,
                        server_id,
                        llm_clone,
                        app_state_clone,
                        status_tx_clone,
                        protocol_clone,
                    )
                    .await
                    {
                        error!("Error handling h2c request: {}", e);
                    }
                });
            }
            None => {
                // Connection closed
                info!("H2C connection {} closed", connection_id);
                break;
            }
        }
    }

    Ok(())
}
