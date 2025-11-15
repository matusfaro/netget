//! OpenAI-compatible API server implementation
//!
//! OpenAI API runs over HTTP. The LLM uses Ollama to generate chat completions
//! and return them in OpenAI-compatible format.

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::openai::actions::{OpenAiProtocol, OPENAI_REQUEST_EVENT};
use crate::state::app_state::AppState;
use crate::{console_error, console_info};

/// OpenAI-compatible API server that delegates to LLM/Ollama
pub struct OpenAiServer;

impl OpenAiServer {
    /// Spawn the OpenAI API server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "OpenAI API server listening on {}", local_addr);

        let protocol = Arc::new(OpenAiProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "OpenAI API connection {} from {}",
                            connection_id, remote_addr
                        );
                        let _ = status_tx
                            .send(format!("[INFO] OpenAI API connection from {}", remote_addr));

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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
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

                            // Create a service that handles OpenAI API requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_openai_request(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    server_id,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                error!("Error serving OpenAI API connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send(format!(
                                "[INFO] OpenAI API connection {} closed",
                                connection_id
                            ));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept OpenAI API connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single OpenAI API request with LLM actions
async fn handle_openai_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OpenAiProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().to_string();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    debug!("OpenAI API request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] OpenAI API {} {}", method, path));

    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read request body: {}", e));
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    json!({
                        "error": {
                            "message": "Failed to read request body",
                            "type": "invalid_request_error"
                        }
                    })
                    .to_string(),
                )))
                .unwrap());
        }
    };

    let body_text = String::from_utf8_lossy(&body_bytes);

    // Create OpenAI request event
    let event = Event::new(
        &OPENAI_REQUEST_EVENT,
        json!({
            "method": method,
            "path": path,
            "body": if body_text.is_empty() { "" } else { body_text.as_ref() }
        }),
    );

    // Call LLM to generate OpenAI response
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
            debug!("LLM OpenAI response received");

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Build HTTP response from action results
            build_openai_response(execution_result.protocol_results, &method, &path, &status_tx)
        }
        Err(e) => {
            error!("LLM call failed: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM call failed: {}", e));

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    json!({
                        "error": {
                            "message": format!("Internal error: {}", e),
                            "type": "server_error",
                            "code": "internal_error"
                        }
                    })
                    .to_string(),
                )))
                .unwrap())
        }
    }
}

/// Build HTTP response from OpenAI action results
fn build_openai_response(
    protocol_results: Vec<ActionResult>,
    method: &str,
    path: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Find openai_response result
    for result in &protocol_results {
        if let ActionResult::Custom { name, data } = result {
            if name == "openai_response" {
                // Extract response data
                let status = data
                    .get("status")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200) as u16;

                let headers = data
                    .get("headers")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let body = data.get("body").and_then(|v| v.as_str()).unwrap_or("{}");

                let _ = status_tx.send(format!("[DEBUG] OpenAI {} {} -> {}", method, path, status));

                // Build response
                let mut response_builder = Response::builder().status(status);

                // Add headers
                for header in headers {
                    if let (Some(name_val), Some(value_val)) = (
                        header.get(0).and_then(|v| v.as_str()),
                        header.get(1).and_then(|v| v.as_str()),
                    ) {
                        response_builder = response_builder.header(name_val, value_val);
                    }
                }

                return Ok(response_builder
                    .body(Full::new(Bytes::from(body.to_string())))
                    .unwrap());
            }
        }
    }

    // No openai_response action found - return error
    error!("No openai_response action in LLM results");
    let _ = status_tx.send("[ERROR] LLM did not return openai_response action".to_string());

    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            json!({
                "error": {
                    "message": "LLM did not return valid response",
                    "type": "server_error",
                    "code": "internal_error"
                }
            })
            .to_string(),
        )))
        .unwrap())
}
