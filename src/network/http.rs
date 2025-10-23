//! HTTP server implementation using hyper

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::network::connection::ConnectionId;
use crate::network::HttpProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, NetworkContext, ActionResult, ProtocolActions};
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for HTTP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling HTTP requests. The data contains parsed HTTP request details with method, URI, headers, and body.
Return appropriate HTTP status codes (200, 404, 500, etc.) with headers and body."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "status": 200,  // HTTP status code
  "headers": {"Content-Type": "text/html"},  // Response headers
  "body": "Response body",  // Response body
  "message": null,  // Optional message for user
  "set_memory": null,  // Replace memory
  "append_memory": null  // Append to memory
}"#;

    (context, output_format)
}

/// HTTP server that delegates request handling to LLM
pub struct HttpServer;

impl HttpServer {
    /// Spawn the HTTP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("HTTP server listening on {}", local_addr);

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted HTTP connection {} from {}", connection_id, remote_addr);

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();

                            // Create a service that handles requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_clone.clone();
                                let status_clone = status_for_service.clone();
                                handle_http_request_with_llm(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving HTTP connection: {:?}", err);
                            }

                            let _ = status_tx_clone.send(format!("✗ HTTP connection {} closed", connection_id));
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

    /// Spawn the HTTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("HTTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(HttpProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Accepted HTTP connection {} from {}", connection_id, remote_addr);

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
                            protocol_info: ProtocolConnectionInfo::Http {
                                recent_requests: Vec::new(),
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();
                            let app_state_for_service = app_state_clone.clone();

                            // Create a service that handles requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_http_request_with_llm_actions(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving HTTP connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("✗ HTTP connection {} closed", connection_id));
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
}

/// Handle a single HTTP request with integrated LLM
async fn handle_http_request_with_llm(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details first for logging
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // DEBUG: Log request summary to both file and TUI
    debug!(
        "HTTP request: {} {} ({} bytes) from {:?}",
        method,
        uri,
        body_bytes.len(),
        connection_id
    );
    let _ = status_tx.send(format!(
        "[DEBUG] HTTP request: {} {} ({} bytes)",
        method, uri, body_bytes.len()
    ));

    // TRACE: Log full request details
    trace!("HTTP request headers:");
    for (name, value) in &headers {
        trace!("  {}: {}", name, value);
        let _ = status_tx.send(format!("[TRACE] HTTP header: {}: {}", name, value));
    }
    if !body_bytes.is_empty() {
        if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
            // Try to pretty-print if it's JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_str) {
                let pretty = serde_json::to_string_pretty(&json).unwrap_or(body_str.to_string());
                trace!("HTTP request body (JSON):\n{}", pretty);
                let _ = status_tx.send(format!("[TRACE] HTTP request body (JSON):\n{}", pretty));
            } else {
                trace!("HTTP request body:\n{}", body_str);
                let _ = status_tx.send(format!("[TRACE] HTTP request body:\n{}", body_str));
            }
        } else {
            trace!("HTTP request body (binary): {} bytes", body_bytes.len());
            let _ = status_tx.send(format!("[TRACE] HTTP request body (binary): {} bytes", body_bytes.len()));
        }
    }

    // Build event description for HTTP request
    let headers_text = headers.iter()
        .map(|(k, v)| format!("  {}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");
    let body_text = String::from_utf8_lossy(&body_bytes);

    let event_description = format!(
        r#"HTTP Request:
- Method: {}
- URI: {}
- Headers:
{}
- Body: {}"#,
        method,
        uri,
        if headers_text.is_empty() { "  (none)" } else { &headers_text },
        if body_text.is_empty() { "(empty)" } else { &body_text }
    );

    // Build prompt and call LLM
    let model = app_state.get_ollama_model().await;
    let prompt_config = get_llm_protocol_prompt();

    let prompt = PromptBuilder::build_network_event_prompt(
        &app_state,
        connection_id,
        &event_description,
        prompt_config,
    ).await;

    // Call LLM to generate HTTP response
    match llm_client.generate_http_response(&model, &prompt).await {
        Ok(llm_response) => {
            // Handle memory updates (HTTP response doesn't use ProcessedResponse, handle manually)
            if let Some(set_global) = llm_response.set_memory.clone() {
                if let Some(server_id) = app_state.get_first_server_id().await {
                    app_state.set_memory(server_id, set_global).await;
                }
            }
            if let Some(append_global) = llm_response.append_memory.clone() {
                if let Some(server_id) = app_state.get_first_server_id().await {
                    let current = app_state.get_memory(server_id).await.unwrap_or_default();
                    let new_memory = if current.is_empty() {
                        append_global
                    } else {
                        format!("{}\n{}", current, append_global)
                    };
                    app_state.set_memory(server_id, new_memory).await;
                }
            }

            // Log if requested
            if let Some(log_msg) = &llm_response.log_message {
                info!("{}", log_msg);
            }

            let _ = status_tx.send(format!(
                "→ HTTP {} {} → {} ({} bytes)",
                method, uri, llm_response.status, llm_response.body.len()
            ));

            // DEBUG: Log response summary to both file and TUI
            debug!(
                "HTTP response: {} ({} bytes, {} headers)",
                llm_response.status,
                llm_response.body.len(),
                llm_response.headers.len()
            );
            let _ = status_tx.send(format!(
                "[DEBUG] HTTP response: {} ({} bytes, {} headers)",
                llm_response.status,
                llm_response.body.len(),
                llm_response.headers.len()
            ));

            // TRACE: Log full response details
            trace!("HTTP response headers:");
            for (name, value) in &llm_response.headers {
                trace!("  {}: {}", name, value);
                let _ = status_tx.send(format!("[TRACE] HTTP header: {}: {}", name, value));
            }
            if !llm_response.body.is_empty() {
                // Try to pretty-print if it's JSON
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&llm_response.body) {
                    let pretty = serde_json::to_string_pretty(&json).unwrap_or(llm_response.body.clone());
                    trace!("HTTP response body (JSON):\n{}", pretty);
                    let _ = status_tx.send(format!("[TRACE] HTTP response body (JSON):\n{}", pretty));
                } else {
                    trace!("HTTP response body:\n{}", llm_response.body);
                    let _ = status_tx.send(format!("[TRACE] HTTP response body:\n{}", llm_response.body));
                }
            }

            // Build the HTTP response
            let mut response = Response::builder().status(llm_response.status);

            // Add headers
            for (name, value) in llm_response.headers {
                response = response.header(name, value);
            }

            Ok(response.body(Full::new(Bytes::from(llm_response.body))).unwrap())
        }
        Err(e) => {
            error!("LLM error generating HTTP response: {}", e);
            let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, e));

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap())
        }
    }
}

/// Handle a single HTTP request with integrated LLM actions
async fn handle_http_request_with_llm_actions(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<HttpProtocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details first for logging
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // DEBUG: Log request summary to both file and TUI
    debug!(
        "HTTP request (action-based): {} {} ({} bytes) from {:?}",
        method,
        uri,
        body_bytes.len(),
        connection_id
    );
    let _ = status_tx.send(format!(
        "[DEBUG] HTTP request: {} {} ({} bytes)",
        method, uri, body_bytes.len()
    ));

    // TRACE: Log full request details
    trace!("HTTP request headers:");
    for (name, value) in &headers {
        trace!("  {}: {}", name, value);
        let _ = status_tx.send(format!("[TRACE] HTTP header: {}: {}", name, value));
    }
    if !body_bytes.is_empty() {
        if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
            // Try to pretty-print if it's JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_str) {
                let pretty = serde_json::to_string_pretty(&json).unwrap_or(body_str.to_string());
                trace!("HTTP request body (JSON):\n{}", pretty);
                let _ = status_tx.send(format!("[TRACE] HTTP request body (JSON):\n{}", pretty));
            } else {
                trace!("HTTP request body:\n{}", body_str);
                let _ = status_tx.send(format!("[TRACE] HTTP request body:\n{}", body_str));
            }
        } else {
            trace!("HTTP request body (binary): {} bytes", body_bytes.len());
            let _ = status_tx.send(format!("[TRACE] HTTP request body (binary): {} bytes", body_bytes.len()));
        }
    }

    // Build event description for HTTP request
    let headers_text = headers.iter()
        .map(|(k, v)| format!("  {}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");
    let body_text = String::from_utf8_lossy(&body_bytes);

    let event_description = format!(
        r#"HTTP Request:
- Method: {}
- URI: {}
- Headers:
{}
- Body: {}"#,
        method,
        uri,
        if headers_text.is_empty() { "  (none)" } else { &headers_text },
        if body_text.is_empty() { "(empty)" } else { &body_text }
    );

    // Create network context
    let context = NetworkContext::HttpRequest {
        connection_id,
        method: method.clone(),
        uri: uri.clone(),
        headers: headers.clone(),
        status_tx: status_tx.clone(),
    };

    // Get protocol sync actions
    let protocol_actions = protocol.get_sync_actions(&context);

    // Build prompt and call LLM
    let model = app_state.get_ollama_model().await;

    let prompt = PromptBuilder::build_network_event_action_prompt(
        &app_state,
        &event_description,
        protocol_actions,
    ).await;

    // Call LLM to generate HTTP response
    match llm_client.generate(&model, &prompt).await {
        Ok(llm_output) => {
            debug!("LLM HTTP response: {}", llm_output);

            // Parse action response
            match ActionResponse::from_str(&llm_output) {
                Ok(action_response) => {
                    // Execute actions
                    match execute_actions(
                        action_response.actions,
                        &app_state,
                        Some(protocol.as_ref()),
                        Some(&context),
                    ).await {
                        Ok(result) => {
                            // Display messages
                            for msg in result.messages {
                                let _ = status_tx.send(msg);
                            }

                            // Extract HTTP response from protocol results
                            // Default response in case nothing was produced
                            let mut status_code = 200;
                            let mut response_headers = HashMap::new();
                            let mut response_body = String::new();

                            for protocol_result in result.protocol_results {
                                if let ActionResult::Output(output_data) = protocol_result {
                                    // Parse the output as JSON containing HTTP response fields
                                    if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&output_data) {
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

                            let _ = status_tx.send(format!(
                                "→ HTTP {} {} → {} ({} bytes)",
                                method, uri, status_code, response_body.len()
                            ));

                            // Build the HTTP response
                            let mut response = Response::builder().status(status_code);

                            // Add headers
                            for (name, value) in response_headers {
                                response = response.header(name, value);
                            }

                            Ok(response.body(Full::new(Bytes::from(response_body))).unwrap())
                        }
                        Err(e) => {
                            error!("Failed to execute actions: {}", e);
                            let _ = status_tx.send(format!("✗ Action execution error: {}", e));

                            Ok(Response::builder()
                                .status(500)
                                .body(Full::new(Bytes::from("Internal Server Error")))
                                .unwrap())
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse action response: {}", e);
                    let _ = status_tx.send(format!("✗ Parse error: {}", e));

                    Ok(Response::builder()
                        .status(500)
                        .body(Full::new(Bytes::from("Internal Server Error")))
                        .unwrap())
                }
            }
        }
        Err(e) => {
            error!("LLM error generating HTTP response: {}", e);
            let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, e));

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap())
        }
    }
}
