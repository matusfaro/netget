//! SAML Service Provider (IDP) server implementation
//!
//! This module implements a SAML 2.0 Service Provider that authenticates users
//! and generates signed SAML assertions. The LLM controls authentication decisions,
//! user attributes, and assertion generation.

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::server::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::SamlSpProtocol;
use crate::state::app_state::AppState;
use actions::SAML_SP_REQUEST_EVENT;

/// SAML SP server that delegates authentication and assertion generation to LLM
pub struct SamlSpServer;

impl SamlSpServer {
    /// Spawn the SAML SP server with LLM-controlled authentication
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        // Dual logging: tracing macro + status_tx
        info!("SAML SP server listening on {}", local_addr);
        let _ = status_tx.send(format!("SAML SP server listening on {}", local_addr));

        let protocol = Arc::new(SamlSpProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);

                        // Dual logging
                        info!("Accepted SAML SP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("SAML SP connection {} from {}", connection_id, remote_addr));

                        let status_tx_for_task = status_tx.clone();

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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let protocol_clone = protocol.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let status_tx = status_tx_for_task;
                            let io = TokioIo::new(stream);

                            // Clone for service_fn closure
                            let llm_for_service = llm_client_clone.clone();
                            let state_for_service = app_state_clone.clone();
                            let status_for_service = status_tx.clone();
                            let protocol_for_service = protocol_clone.clone();

                            // Create a service that handles SAML SP requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_for_service.clone();
                                let state_clone = state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_for_service.clone();
                                handle_saml_sp_request(
                                    req,
                                    connection_id,
                                    server_id,
                                    remote_addr,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                )
                            });

                            // Serve the connection
                            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving SAML SP connection {}: {}", connection_id, e);
                            }

                            // Remove connection when done
                            debug!("SAML SP connection {} closed", connection_id);
                            app_state_clone.remove_connection_from_server(server_id, connection_id).await;
                            let _ = status_tx.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SAML SP connection: {}", e);
                        let _ = status_tx.send(format!("Failed to accept SAML SP connection: {}", e));
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a SAML SP request with LLM decision making
async fn handle_saml_sp_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<SamlSpProtocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(|q| q.to_string());

    // Dual logging: DEBUG level for request summaries
    debug!("SAML SP {} {} from {}", method, path, remote_addr);
    let _ = status_tx.send(format!("SAML SP {} {}", method, path));

    // Extract headers
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(e) => {
            error!("Failed to read SAML SP request body: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Failed to read request body")))
                .unwrap());
        }
    };

    // TRACE level for full payloads
    if !body_bytes.is_empty() {
        trace!("SAML SP request body: {} bytes", body_bytes.len());
        if let Ok(body_str) = String::from_utf8(body_bytes.clone()) {
            trace!("SAML SP request body content: {}", body_str);
        }
    }

    // Update connection stats
    app_state.update_connection_stats(
        server_id,
        connection_id,
        Some(body_bytes.len() as u64),
        None,
        None,
        None,
    ).await;

    // Build event for LLM
    let event = Event::new(
        &SAML_SP_REQUEST_EVENT,
        serde_json::json!({
            "method": method.to_string(),
            "path": path,
            "query": query,
            "headers": headers,
            "body": if body_bytes.is_empty() {
                serde_json::Value::Null
            } else if let Ok(body_str) = String::from_utf8(body_bytes.clone()) {
                serde_json::Value::String(body_str)
            } else {
                // For binary data, use base64
                serde_json::Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &body_bytes))
            },
            "client_ip": remote_addr.ip().to_string(),
        })
    );

    // Call LLM for decision
    debug!("Calling LLM for SAML SP request decision");
    let action_result = call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await;

    // Execute actions and build response
    let response = match action_result {
        Ok(result) => {
            if result.protocol_results.is_empty() {
                warn!("LLM returned no actions for SAML SP request");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from("No response generated")))
                    .unwrap()
            } else {
                // Parse HTTP response from protocol results
                use crate::llm::actions::protocol_trait::ActionResult;

                let mut status_code = 200u16;
                let mut response_headers = std::collections::HashMap::new();
                let mut response_body = String::new();

                for protocol_result in result.protocol_results {
                    if let ActionResult::Output(output_data) = protocol_result {
                        // Parse JSON response data
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

                let mut response = Response::builder().status(status_code);
                for (name, value) in response_headers {
                    response = response.header(name, value);
                }
                response.body(Full::new(Bytes::from(response_body))).unwrap()
            }
        }
        Err(e) => {
            error!("LLM error for SAML SP request: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from(format!("LLM error: {}", e))))
                .unwrap()
        }
    };

    // Update bytes sent
    let response_size = response.body().size_hint().exact().unwrap_or(0);
    app_state.update_connection_stats(
        server_id,
        connection_id,
        None,
        Some(response_size),
        None,
        None,
    ).await;

    debug!("SAML SP response: {} ({} bytes)", response.status(), response_size);

    Ok(response)
}
