//! AWS SQS (Simple Queue Service) compatible server implementation
//!
//! Implements an SQS-compatible HTTP/JSON API on port 9324.
//! The LLM controls all queue operations and maintains "virtual" queues through conversation context.

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
use crate::server::SqsProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// SQS server that delegates queue operations to LLM
pub struct SqsServer;

impl SqsServer {
    /// Spawn the SQS server with integrated LLM actions
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
        console_info!(status_tx, "[INFO] SQS server listening on {}", local_addr);

        let protocol = Arc::new(SqsProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        console_info!(status_tx, "[INFO] SQS connection from {}", remote_addr);

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
                        console_info!(status_tx, "__UPDATE_UI__");

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

                            // Create a service that handles SQS requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_sqs_request_with_llm(
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
                                error!("Error serving SQS connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] SQS connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to accept SQS connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single SQS request with LLM
async fn handle_sqs_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<SqsProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract SQS operation from x-amz-target header
    // Format: "AmazonSQS.SendMessage", "AmazonSQS.ReceiveMessage", etc.
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
            console_error!(status_tx, "[ERROR] Failed to read SQS request body: {}", e);
            Bytes::new()
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    debug!(
        "SQS request: {} {} operation={} ({} bytes)",
        method,
        uri,
        operation,
        body_bytes.len()
    );
    console_debug!(status_tx, "[DEBUG] SQS {} operation={} ({} bytes)");

    // Try to extract queue URL from JSON body
    let queue_url = if !body_str.is_empty() {
        serde_json::from_str::<serde_json::Value>(&body_str)
            .ok()
            .and_then(|v| v.get("QueueUrl").and_then(|q| q.as_str()).map(String::from))
    } else {
        None
    };

    console_trace!(status_tx, "[TRACE] SQS request: {}", body_str);

    // Create SQS request event
    let event = crate::protocol::Event::new(
        &actions::SQS_REQUEST_EVENT,
        serde_json::json!({
            "operation": operation,
            "queue_url": queue_url,
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
            // Look for SQS-specific response actions
            for result in execution_result.protocol_results {
                match result {
                    ActionResult::Custom { name, data } => {
                        if name == "sqs_response" {
                            let status = data.get("status")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(200) as u16;
                            let body = data.get("body")
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");

                            console_debug!(status_tx, "[DEBUG] SQS → {} response", status);
                            console_trace!(status_tx, "[TRACE] SQS response: {}", body);

                            // Generate a simple request ID using timestamp
                            let request_id = format!("{:x}", std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_nanos());

                            return Ok(Response::builder()
                                .status(status)
                                .header("Content-Type", "application/x-amz-json-1.0")
                                .header("x-amzn-RequestId", request_id)
                                .body(Full::new(Bytes::from(body.to_string())))
                                .unwrap());
                        }
                    }
                    _ => {
                        // Other actions don't affect HTTP response
                    }
                }
            }

            // No SQS response action found, return empty success
            console_debug!(status_tx, "[DEBUG] SQS → 200 response (default)");

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
            console_error!(status_tx, "[ERROR] LLM execution failed: {}", e);

            let request_id = format!("{:x}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos());

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("x-amzn-RequestId", request_id)
                .body(Full::new(Bytes::from(
                    r#"{"__type":"InternalFailure","message":"Internal server error"}"#
                )))
                .unwrap())
        }
    }
}
