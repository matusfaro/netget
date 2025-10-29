//! IPP (Internet Printing Protocol) server implementation
//!
//! IPP runs over HTTP on port 631. The LLM controls printer attributes,
//! job handling, and responses.

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
use crate::network::IppProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::state::app_state::AppState;

/// IPP server that delegates request handling to LLM
pub struct IppServer;

impl IppServer {
    /// Spawn the IPP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("IPP server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] IPP server listening on {}", local_addr));

        let protocol = Arc::new(IppProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("IPP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] IPP connection from {}", remote_addr));

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
                            protocol_info: ProtocolConnectionInfo::Ipp {
                                recent_jobs: Vec::new(),
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

                            // Create a service that handles IPP requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_ipp_request_with_llm(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    server_id,
                                )
                            });

                            // Serve HTTP/1 on this connection (IPP uses HTTP)
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving IPP connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] IPP connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IPP connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept IPP connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single IPP request with LLM
async fn handle_ipp_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<IppProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body (IPP operation data)
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read IPP request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read IPP request body: {}", e));
            Bytes::new()
        }
    };

    debug!(
        "IPP request: {} {} ({} bytes)",
        method,
        uri,
        body_bytes.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] IPP {} {} ({} bytes)",
        method, uri, body_bytes.len()
    ));

    // Parse IPP request if body is present
    let operation_name = if !body_bytes.is_empty() {
        parse_ipp_operation(&body_bytes).unwrap_or_else(|| "Unknown".to_string())
    } else {
        "Empty".to_string()
    };

    trace!("IPP operation: {}", operation_name);

    // Create IPP request event
    let event = crate::protocol::Event::new(
        &crate::network::ipp_actions::IPP_REQUEST_EVENT,
        serde_json::json!({
            "method": method,
            "uri": uri,
            "operation": operation_name,
        }),
    );

    let llm_result = crate::llm::action_helper::call_llm(
        &llm_client,
        &app_state,
        server_id,
        None, // TODO: Add connection_id when available
        &event,
        protocol.as_ref(),
    )
    .await;

    // Process action results to build HTTP response
    match llm_result {
        Ok(execution_result) => {
            // Look for IPP-specific response actions
            for result in execution_result.protocol_results {
                match result {
                    ActionResult::IppResponse { status, body } => {
                        debug!("IPP response: status={}", status);
                        let _ = status_tx.send(format!("[DEBUG] IPP → {} response", status));

                        return Ok(Response::builder()
                            .status(status)
                            .header("Content-Type", "application/ipp")
                            .body(Full::new(Bytes::from(body)))
                            .unwrap());
                    }
                    _ => {
                        // Other actions don't affect HTTP response
                    }
                }
            }

            // No IPP response found, return default OK
            debug!("No IPP response from LLM, returning 200 OK");
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "application/ipp")
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error for IPP request: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM error for IPP request: {}", e));

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap())
        }
    }
}

/// Parse IPP operation from raw bytes to extract operation name
fn parse_ipp_operation(body: &[u8]) -> Option<String> {
    // IPP format: version(2) + operation-id(2) + request-id(4) + attributes
    if body.len() < 8 {
        return None;
    }

    // Extract operation ID (bytes 2-3, big endian)
    let operation_id = u16::from_be_bytes([body[2], body[3]]);

    // Map common operation IDs to names
    let name = match operation_id {
        0x0002 => "Print-Job",
        0x0003 => "Print-URI",
        0x0004 => "Validate-Job",
        0x0005 => "Create-Job",
        0x0006 => "Send-Document",
        0x0007 => "Send-URI",
        0x0008 => "Cancel-Job",
        0x0009 => "Get-Job-Attributes",
        0x000A => "Get-Jobs",
        0x000B => "Get-Printer-Attributes",
        0x000C => "Hold-Job",
        0x000D => "Release-Job",
        0x000E => "Restart-Job",
        0x000F => "Pause-Printer",
        0x0010 => "Resume-Printer",
        0x0011 => "Purge-Jobs",
        _ => return Some(format!("Operation-0x{:04X}", operation_id)),
    };

    Some(name.to_string())
}
