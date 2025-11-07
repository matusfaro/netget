//! HTTP/2 server implementation using h2 crate directly for full server push support

use bytes::Bytes;
use h2::server::{self, SendResponse};
use http::{Request, Response, StatusCode};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::Http2Protocol;
use crate::state::app_state::AppState;

use super::actions::HTTP2_REQUEST_EVENT;
use super::push::{PendingPush, PushManager};

/// HTTP/2 server with full server push support
pub struct H2Server;

impl H2Server {
    /// Spawn HTTP/2 server using h2 crate directly
    pub async fn spawn_with_push_support(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        tls_config: Option<Arc<rustls::ServerConfig>>,
    ) -> anyhow::Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        let protocol_name = if tls_config.is_some() { "HTTP/2 (TLS, h2 with push)" } else { "HTTP/2 (h2c with push)" };
        info!("{} server listening on {}", protocol_name, local_addr);

        let protocol = Arc::new(Http2Protocol::new());

        // Create TLS acceptor if TLS is enabled
        let tls_acceptor = tls_config.map(|config| {
            tokio_rustls::TlsAcceptor::from(config)
        });

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((tcp_stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = tcp_stream.local_addr().unwrap_or(local_addr);
                        info!("Accepted {} connection {} from {}", protocol_name, connection_id, remote_addr);

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
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
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let tls_acceptor_clone = tls_acceptor.clone();

                        // Spawn task to handle this connection
                        tokio::spawn(async move {
                            // Perform TLS handshake if TLS is enabled
                            let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = if let Some(acceptor) = tls_acceptor_clone {
                                match acceptor.accept(tcp_stream).await {
                                    Ok(tls_stream) => {
                                        debug!("{} TLS handshake complete with {}", protocol_name, remote_addr);
                                        let _ = status_tx_clone.send(format!("[DEBUG] {} TLS handshake complete with {}", protocol_name, remote_addr));
                                        handle_h2_connection(
                                            tls_stream,
                                            connection_id,
                                            server_id,
                                            llm_client_clone,
                                            app_state_clone.clone(),
                                            status_tx_clone.clone(),
                                            protocol_clone,
                                        ).await
                                    }
                                    Err(e) => {
                                        error!("{} TLS handshake failed: {}", protocol_name, e);
                                        let _ = status_tx_clone.send(format!("[ERROR] {} TLS handshake failed: {}", protocol_name, e));
                                        Err(Box::new(e))
                                    }
                                }
                            } else {
                                // No TLS, use plain TCP (h2c)
                                handle_h2_connection(
                                    tcp_stream,
                                    connection_id,
                                    server_id,
                                    llm_client_clone,
                                    app_state_clone.clone(),
                                    status_tx_clone.clone(),
                                    protocol_clone,
                                ).await
                            };

                            if let Err(e) = result {
                                error!("{} connection error: {}", protocol_name, e);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("✗ {} connection {connection_id} closed", protocol_name));
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
}

/// Handle a single HTTP/2 connection with full server push support
async fn handle_h2_connection<T>(
    tcp_stream: T,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<Http2Protocol>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Create h2 connection
    let mut h2_conn = server::handshake(tcp_stream).await?;
    debug!("HTTP/2 handshake complete for connection {}", connection_id);

    // Handle incoming requests
    while let Some(result) = h2_conn.accept().await {
        let (request, send_response) = result?;

        let llm_clone = llm_client.clone();
        let app_state_clone = app_state.clone();
        let status_clone = status_tx.clone();
        let protocol_clone = protocol.clone();

        // Spawn task for each request (stream)
        tokio::spawn(async move {
            if let Err(e) = handle_h2_request(
                request,
                send_response,
                connection_id,
                server_id,
                llm_clone,
                app_state_clone,
                status_clone,
                protocol_clone,
            ).await {
                error!("Error handling HTTP/2 request: {}", e);
            }
        });
    }

    Ok(())
}

/// Handle a single HTTP/2 request with server push support
pub async fn handle_h2_request(
    request: Request<h2::RecvStream>,
    mut send_response: SendResponse<Bytes>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<Http2Protocol>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Extract request metadata
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    let version = format!("{:?}", request.version());

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in request.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read request body from h2::RecvStream
    let mut body_stream = request.into_body();
    let mut body_bytes = Vec::new();

    loop {
        match body_stream.data().await {
            Some(Ok(chunk)) => {
                body_bytes.extend_from_slice(&chunk);
                // Release flow control capacity for this chunk
                let _ = body_stream.flow_control().release_capacity(chunk.len());
            }
            Some(Err(e)) => {
                warn!("Error reading request body: {}", e);
                let _ = status_tx.send(format!("[WARN] Error reading body: {}", e));
                break;
            }
            None => {
                // End of stream
                break;
            }
        }
    }

    // Log request
    debug!(
        "HTTP/2 request: {} {} {} ({} bytes) from {:?}",
        method, uri, version, body_bytes.len(), connection_id
    );
    let _ = status_tx.send(format!(
        "[DEBUG] HTTP/2 request: {} {} {} ({} bytes)",
        method, uri, version, body_bytes.len()
    ));

    // Create event for LLM
    let body_text = String::from_utf8_lossy(&body_bytes);
    let event = Event::new(&HTTP2_REQUEST_EVENT, serde_json::json!({
        "method": method,
        "uri": uri,
        "version": version,
        "headers": headers,
        "body": if body_text.is_empty() { "" } else { body_text.as_ref() }
    }));

    // Create push manager for this request
    let push_manager = Arc::new(Mutex::new(PushManager::new()));
    let _push_manager_clone = push_manager.clone();

    // Store push manager in app state for action access
    // (This would require extending AppState, for now we'll use a simpler approach)

    // Call LLM to generate response
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await {
        Ok(execution_result) => {
            debug!("LLM HTTP/2 response received");

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Extract response and pushes from protocol results
            let mut status_code = 200;
            let mut response_headers = HashMap::new();
            let mut response_body = String::new();
            let mut pushes = Vec::new();

            for protocol_result in execution_result.protocol_results {
                match protocol_result {
                    ActionResult::Output(output_data) => {
                        // Parse JSON output
                        if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&output_data) {
                            // Check if this is a push directive
                            if json_value.get("_push_directive").and_then(|v| v.as_bool()).unwrap_or(false) {
                                // This is a push request
                                let push = PendingPush {
                                    path: json_value.get("path")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("/").to_string(),
                                    method: json_value.get("method")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("GET").to_string(),
                                    status: json_value.get("status")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(200) as u16,
                                    headers: json_value.get("headers")
                                        .and_then(|v| v.as_object())
                                        .map(|obj| obj.iter()
                                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                            .collect())
                                        .unwrap_or_default(),
                                    body: json_value.get("body")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("").as_bytes().to_vec(),
                                };
                                pushes.push(push);
                            } else {
                                // This is the main HTTP response
                                if let Some(status) = json_value.get("status").and_then(|v| v.as_u64()) {
                                    status_code = status as u16;
                                }
                                if let Some(headers_obj) = json_value.get("headers").and_then(|v| v.as_object()) {
                                    for (k, v) in headers_obj {
                                        if let Some(v_str) = v.as_str() {
                                            response_headers.insert(k.clone(), v_str.to_string());
                                        }
                                    }
                                }
                                if let Some(body) = json_value.get("body").and_then(|v| v.as_str()) {
                                    response_body = body.to_string();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Execute server pushes BEFORE sending main response
            for push in pushes {
                debug!("Executing server push for {}", push.path);
                let push_body_len = push.body.len();

                // Create push promise request
                let mut push_request = http::Request::builder()
                    .method(push.method.as_str())
                    .uri(&push.path);

                for (name, value) in &push.headers {
                    push_request = push_request.header(name, value);
                }

                if let Ok(push_req) = push_request.body(()) {
                    // Send push promise
                    match send_response.push_request(push_req) {
                        Ok(mut push_stream) => {
                            // Send push response
                            let mut push_response = http::Response::builder()
                                .status(push.status);

                            for (name, value) in &push.headers {
                                push_response = push_response.header(name, value);
                            }

                            if let Ok(push_resp) = push_response.body(()) {
                                match push_stream.send_response(push_resp, false) {
                                    Ok(mut stream) => {
                                        if let Err(e) = stream.send_data(Bytes::from(push.body), true) {
                                            warn!("Failed to send push body for {}: {}", push.path, e);
                                        } else {
                                            debug!("Successfully pushed {}", push.path);
                                            let _ = status_tx.send(format!("⬆ Pushed {} ({} bytes)", push.path, push_body_len));
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to send push response for {}: {}", push.path, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Client rejected push for {}: {}", push.path, e);
                        }
                    }
                }
            }

            // Send main response
            let _ = status_tx.send(format!(
                "→ HTTP/2 {} {} → {} ({} bytes)",
                method, uri, status_code, response_body.len()
            ));

            let mut response = Response::builder().status(status_code);
            for (name, value) in response_headers {
                response = response.header(name, value);
            }

            let response = response.body(())?;
            let mut stream = send_response.send_response(response, false)?;
            stream.send_data(Bytes::from(response_body), true)?;
        }
        Err(e) => {
            error!("LLM error generating HTTP/2 response: {}", e);
            let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, e));

            let response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(())?;
            let mut stream = send_response.send_response(response, false)?;
            stream.send_data(Bytes::from("Internal Server Error"), true)?;
        }
    }

    Ok(())
}
