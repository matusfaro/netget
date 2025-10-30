//! DynamoDB-compatible server implementation
//!
//! Implements a DynamoDB-compatible HTTP/JSON API on port 8000.
//! The LLM controls all database operations and maintains "virtual" data through conversation context.

pub mod actions;

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

use crate::server::connection::ConnectionId;
use crate::server::DynamoProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::state::app_state::AppState;

/// DynamoDB server that delegates API operations to LLM
pub struct DynamoServer;

impl DynamoServer {
    /// Spawn the DynamoDB server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("DynamoDB server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] DynamoDB server listening on {}", local_addr));

        let protocol = Arc::new(DynamoProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("DynamoDB connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] DynamoDB connection from {}", remote_addr));

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
                            protocol_info: ProtocolConnectionInfo::Dynamo {
                                recent_operations: Vec::new(), // (operation, table, time)
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

                            // Create a service that handles DynamoDB requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_dynamo_request_with_llm(
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
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving DynamoDB connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] DynamoDB connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept DynamoDB connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept DynamoDB connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single DynamoDB request with LLM
async fn handle_dynamo_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<DynamoProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract DynamoDB operation from x-amz-target header
    // Format: "DynamoDB_20120810.GetItem"
    let operation = req.headers()
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split('.').nth(1))
        .unwrap_or("Unknown")
        .to_string();

    // Read JSON body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read DynamoDB request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read DynamoDB request body: {}", e));
            Bytes::new()
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    debug!(
        "DynamoDB request: {} {} operation={} ({} bytes)",
        method,
        uri,
        operation,
        body_bytes.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] DynamoDB {} operation={} ({} bytes)",
        method, operation, body_bytes.len()
    ));

    // Try to extract table name from JSON body
    let table_name = if !body_str.is_empty() {
        serde_json::from_str::<serde_json::Value>(&body_str)
            .ok()
            .and_then(|v| v.get("TableName").and_then(|t| t.as_str()).map(String::from))
    } else {
        None
    };

    trace!("DynamoDB request body: {}", body_str);
    let _ = status_tx.send(format!("[TRACE] DynamoDB request: {}", body_str));

    // Create DynamoDB request event
    let event = crate::protocol::Event::new(
        &actions::DYNAMO_REQUEST_EVENT,
        serde_json::json!({
            "operation": operation,
            "table_name": table_name,
            "request_body": body_str,
        }),
    );

    let llm_result = crate::llm::action_helper::call_llm(
        &llm_client,
        &app_state,
        server_id,
        None, // Connection ID not needed for stateless HTTP
        &event,
        protocol.as_ref(),
    )
    .await;

    // Process action results to build HTTP response
    match llm_result {
        Ok(execution_result) => {
            // Look for DynamoDB-specific response actions
            for result in execution_result.protocol_results {
                match result {
                    ActionResult::DynamoResponse { status, body } => {
                        debug!("DynamoDB response: status={}", status);
                        let _ = status_tx.send(format!("[DEBUG] DynamoDB → {} response", status));
                        trace!("DynamoDB response body: {}", body);
                        let _ = status_tx.send(format!("[TRACE] DynamoDB response: {}", body));

                        // Generate a simple request ID using timestamp
                        let request_id = format!("{:x}", std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos());

                        return Ok(Response::builder()
                            .status(status)
                            .header("Content-Type", "application/x-amz-json-1.0")
                            .header("x-amzn-RequestId", request_id)
                            .body(Full::new(Bytes::from(body)))
                            .unwrap());
                    }
                    _ => {
                        // Other actions don't affect HTTP response
                    }
                }
            }

            // No DynamoDB response found, return default OK with empty response
            debug!("No DynamoDB response from LLM, returning 200 OK with empty object");

            let request_id = format!("{:x}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos());

            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("x-amzn-RequestId", request_id)
                .body(Full::new(Bytes::from("{}")))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error for DynamoDB request: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM error for DynamoDB request: {}", e));

            // Return DynamoDB error format
            let error_response = serde_json::json!({
                "__type": "InternalServerError",
                "message": "Internal server error"
            });

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/x-amz-json-1.0")
                .body(Full::new(Bytes::from(error_response.to_string())))
                .unwrap())
        }
    }
}
