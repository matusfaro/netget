//! JSON-RPC 2.0 server implementation
//!
//! JSON-RPC runs over HTTP POST. The LLM controls all RPC method calls and responses.
//! Supports single requests, batch requests, and notifications.

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::{ActionResult, Server};
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::jsonrpc::actions::{JsonRpcProtocol, JSONRPC_METHOD_CALL_EVENT};
use crate::state::app_state::AppState;
use crate::{console_error, console_info};

/// JSON-RPC 2.0 standard error codes
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const INTERNAL_ERROR: i32 = -32603;

/// JSON-RPC 2.0 server that delegates to LLM
pub struct JsonRpcServer;

impl JsonRpcServer {
    /// Spawn the JSON-RPC server with integrated LLM actions
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
        console_info!(status_tx, "JSON-RPC server listening on {}", local_addr);

        let protocol = Arc::new(JsonRpcProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("JSON-RPC connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx
                            .send(format!("[INFO] JSON-RPC connection from {}", remote_addr));

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

                            // Create a service that handles JSON-RPC requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_jsonrpc_request(
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
                                error!("Error serving JSON-RPC connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send(format!(
                                "[INFO] JSON-RPC connection {} closed",
                                connection_id
                            ));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept JSON-RPC connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single JSON-RPC request
async fn handle_jsonrpc_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<JsonRpcProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();

    debug!("JSON-RPC request: {} {}", method, uri.path());
    let _ = status_tx.send(format!("[DEBUG] JSON-RPC {} {}", method, uri.path()));

    // JSON-RPC requires POST method
    if method != Method::POST {
        return Ok(build_error_response(
            INVALID_REQUEST,
            "JSON-RPC requires POST method",
            None,
            None,
        ));
    }

    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            console_error!(status_tx, "Failed to read request body: {}", e);
            return Ok(build_error_response(
                INVALID_REQUEST,
                "Failed to read request body",
                None,
                None,
            ));
        }
    };

    // Parse JSON
    let request_value: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            console_error!(status_tx, "Failed to parse JSON: {}", e);
            return Ok(build_error_response(PARSE_ERROR, "Parse error", None, None));
        }
    };

    trace!(
        "JSON-RPC request body: {}",
        serde_json::to_string_pretty(&request_value).unwrap_or_default()
    );
    let _ = status_tx.send(format!(
        "[TRACE] JSON-RPC request: {}",
        serde_json::to_string_pretty(&request_value).unwrap_or_default()
    ));

    // Check if it's a batch request (array) or single request (object)
    match request_value {
        Value::Array(requests) if !requests.is_empty() => {
            // Batch request
            debug!(
                "Processing batch JSON-RPC request with {} items",
                requests.len()
            );
            let _ = status_tx.send(format!(
                "[DEBUG] Batch JSON-RPC request with {} items",
                requests.len()
            ));

            let mut responses = Vec::new();
            for request in requests {
                if let Some(response) = process_single_request(
                    request,
                    connection_id,
                    &llm_client,
                    &app_state,
                    &status_tx,
                    &protocol,
                    server_id,
                )
                .await
                {
                    responses.push(response);
                }
            }

            // Return batch response
            let response_json = json!(responses);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(response_json.to_string())))
                .unwrap())
        }
        Value::Object(_) => {
            // Single request
            if let Some(response) = process_single_request(
                request_value,
                connection_id,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                server_id,
            )
            .await
            {
                let response_json = response;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(response_json.to_string())))
                    .unwrap())
            } else {
                // Notification (no response)
                Ok(Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(Full::new(Bytes::new()))
                    .unwrap())
            }
        }
        Value::Array(_) => {
            // Empty batch request
            Ok(build_error_response(
                INVALID_REQUEST,
                "Empty batch request",
                None,
                None,
            ))
        }
        _ => {
            // Invalid request type
            Ok(build_error_response(
                INVALID_REQUEST,
                "Request must be an object or array",
                None,
                None,
            ))
        }
    }
}

/// Process a single JSON-RPC request
/// Returns None for notifications (no id field)
async fn process_single_request(
    request: Value,
    connection_id: ConnectionId,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<JsonRpcProtocol>,
    server_id: crate::state::ServerId,
) -> Option<Value> {
    // Extract fields
    let jsonrpc_version = request.get("jsonrpc").and_then(|v| v.as_str());
    let method = request.get("method").and_then(|v| v.as_str());
    let params = request.get("params").cloned();
    let id = request.get("id").cloned();

    // Check if it's a notification (no id field or id is null)
    let is_notification = id.is_none() || id.as_ref().map(|v| v.is_null()).unwrap_or(false);

    // Validate JSON-RPC version
    if jsonrpc_version != Some("2.0") {
        if !is_notification {
            return Some(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": INVALID_REQUEST,
                    "message": "Invalid JSON-RPC version, must be '2.0'"
                },
                "id": id
            }));
        } else {
            return None; // Notifications never return errors
        }
    }

    // Validate method
    let method = match method {
        Some(m) if !m.is_empty() => m,
        _ => {
            if !is_notification {
                return Some(json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": INVALID_REQUEST,
                        "message": "Missing or invalid 'method' field"
                    },
                    "id": id
                }));
            } else {
                return None; // Notifications never return errors
            }
        }
    };

    debug!(
        "JSON-RPC method call: method={}, is_notification={}",
        method, is_notification
    );
    let _ = status_tx.send(format!(
        "[DEBUG] JSON-RPC method={}, notification={}",
        method, is_notification
    ));

    // Track method in connection state
    if let Err(e) = track_method_call(app_state, server_id, connection_id, method).await {
        error!("Failed to track method call: {}", e);
    }

    // Call LLM with method details
    let response_value = match call_llm_for_method(
        method,
        params.as_ref(),
        id.clone(),
        llm_client,
        app_state,
        status_tx,
        protocol,
        connection_id,
        server_id,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            console_error!(status_tx, "LLM call failed: {}", e);
            if !is_notification {
                return Some(json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": INTERNAL_ERROR,
                        "message": format!("Internal error: {}", e)
                    },
                    "id": id
                }));
            } else {
                return None; // Notifications never return errors
            }
        }
    };

    // Return response (or None for notifications)
    if !is_notification {
        Some(response_value)
    } else {
        trace!("Notification processed, no response sent");
        None
    }
}

/// Call LLM to handle the JSON-RPC method call
async fn call_llm_for_method(
    method: &str,
    params: Option<&Value>,
    request_id: Option<Value>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<JsonRpcProtocol>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
) -> anyhow::Result<Value> {
    // Create JSON-RPC method call event
    let event_data = if let Some(params_val) = params {
        json!({
            "method": method,
            "params": params_val,
            "id": request_id
        })
    } else {
        json!({
            "method": method,
            "id": request_id
        })
    };

    let event = Event::new(&JSONRPC_METHOD_CALL_EVENT, event_data);

    debug!("Calling LLM for JSON-RPC method: {}", method);
    let _ = status_tx.send(format!("[DEBUG] Calling LLM for method: {}", method));

    // Call LLM with event
    let llm_result = call_llm(
        llm_client,
        app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    )
    .await?;

    trace!("LLM actions for JSON-RPC: {:?}", llm_result.raw_actions.len());

    // Execute the first action
    if let Some(action) = llm_result.raw_actions.first() {
        // Clone the action so we can modify it
        let mut action = action.clone();

        // Auto-fill the id if not provided by LLM
        let action_type = action.get("type").and_then(|v| v.as_str());
        if action_type == Some("jsonrpc_success") || action_type == Some("jsonrpc_error") {
            let has_id = action.get("id").map(|v| !v.is_null()).unwrap_or(false);

            if !has_id {
                if let Some(action_obj) = action.as_object_mut() {
                    if let Some(req_id) = &request_id {
                        debug!("Auto-filling id field with request id: {:?}", req_id);
                        action_obj.insert("id".to_string(), req_id.clone());
                    }
                }
            }
        }

        match protocol.execute_action(action) {
            Ok(ActionResult::Custom { name, data }) if name == "jsonrpc_response" => {
                return Ok(data);
            }
            Ok(_) => {
                return Err(anyhow::anyhow!("LLM returned non-JSON-RPC action"));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to execute action: {}", e));
            }
        }
    }

    // If no response found, return a default error
    Ok(json!({
        "jsonrpc": "2.0",
        "error": {
            "code": INTERNAL_ERROR,
            "message": "LLM did not generate a valid JSON-RPC response"
        },
        "id": request_id
    }))
}

/// Track method call in connection state
async fn track_method_call(
    app_state: &Arc<AppState>,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    method: &str,
) -> anyhow::Result<()> {
    app_state
        .with_server_mut(server_id, |server| {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    let mut recent_methods: Vec<String> = obj
                        .get("recent_methods")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    recent_methods.push(method.to_string());
                    // Keep only last 10 methods
                    if recent_methods.len() > 10 {
                        recent_methods.remove(0);
                    }
                    obj.insert(
                        "recent_methods".to_string(),
                        serde_json::to_value(&recent_methods).unwrap_or(serde_json::json!([])),
                    );
                }
            }
        })
        .await;

    Ok(())
}

/// Build a JSON-RPC error response
fn build_error_response(
    code: i32,
    message: &str,
    data: Option<Value>,
    id: Option<Value>,
) -> Response<Full<Bytes>> {
    let mut error = json!({
        "code": code,
        "message": message
    });

    if let Some(data_val) = data {
        error
            .as_object_mut()
            .unwrap()
            .insert("data".to_string(), data_val);
    }

    let response = json!({
        "jsonrpc": "2.0",
        "error": error,
        "id": id.unwrap_or(Value::Null)
    });

    Response::builder()
        .status(StatusCode::OK) // JSON-RPC always returns 200 OK
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap()
}
