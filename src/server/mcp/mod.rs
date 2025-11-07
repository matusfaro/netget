//! MCP (Model Context Protocol) server implementation
//!
//! This module implements an MCP server that allows LLM to control all server capabilities.
//! MCP is built on JSON-RPC 2.0 and provides a standardized way for LLM applications
//! to access external resources, tools, and prompts.
//!
//! Key features:
//! - JSON-RPC 2.0 over HTTP/SSE transport
//! - Full LLM control over resources, tools, and prompts
//! - Session-based state management
//! - Three-phase initialization (initialize → response → initialized)
//! - Support for resource subscriptions, tool execution, and prompt templates

pub mod actions;
pub mod jsonrpc;
pub mod session;

use anyhow::Result;
use axum::{
    extract::{Json, State as AxumState},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use jsonrpc::{ErrorCode, JsonRpcError, JsonRpcMessage, JsonRpcResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

#[cfg(feature = "mcp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "mcp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "mcp")]
use crate::protocol::Event;
#[cfg(feature = "mcp")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "mcp")]
use crate::server::McpProtocol;
#[cfg(feature = "mcp")]
use crate::state::app_state::AppState;
#[cfg(feature = "mcp")]
use crate::state::server::{ConnectionStatus, ProtocolConnectionInfo, ServerId};
#[cfg(feature = "mcp")]
use actions::{
    MCP_INITIALIZE_EVENT, MCP_RESOURCES_LIST_EVENT, MCP_RESOURCES_READ_EVENT,
    MCP_TOOLS_LIST_EVENT, MCP_TOOLS_CALL_EVENT, MCP_PROMPTS_LIST_EVENT, MCP_PROMPTS_GET_EVENT,
};
#[cfg(feature = "mcp")]
use session::McpSession;

/// MCP server shared state
#[derive(Clone)]
pub struct McpServerState {
    /// LLM client for generating responses
    pub llm_client: OllamaClient,
    /// Application state
    pub app_state: Arc<AppState>,
    /// Status message sender
    pub status_tx: mpsc::UnboundedSender<String>,
    /// Server ID for tracking connections
    pub server_id: ServerId,
    /// Protocol implementation
    pub protocol: Arc<McpProtocol>,
    /// Active sessions (keyed by session ID)
    pub sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSession>>>>>,
    /// Local address the server is bound to
    pub local_addr: SocketAddr,
}

/// MCP server that handles Model Context Protocol over HTTP
pub struct McpServer;

#[cfg(feature = "mcp")]
impl McpServer {
    /// Spawn MCP server with Axum on HTTP (default port 8000)
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        let listener = tokio::net::TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        info!("MCP server (JSON-RPC 2.0) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] MCP server listening on {}", local_addr));

        let protocol = Arc::new(McpProtocol::new());
        let sessions = Arc::new(Mutex::new(HashMap::new()));

        let server_state = McpServerState {
            llm_client,
            app_state,
            status_tx: status_tx.clone(),
            server_id,
            protocol,
            sessions,
            local_addr,
        };

        // Build Axum router
        let app = Router::new()
            .route("/", post(handle_jsonrpc))
            .with_state(server_state);

        // Spawn server
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("MCP server error: {}", e);
                let _ = status_tx.send(format!("[ERROR] MCP server error: {}", e));
            }
        });

        Ok(local_addr)
    }
}

/// Handle incoming JSON-RPC 2.0 requests
#[cfg(feature = "mcp")]
async fn handle_jsonrpc(
    AxumState(state): AxumState<McpServerState>,
    Json(payload): Json<Value>,
) -> Response {
    trace!("MCP received JSON-RPC request: {}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()));

    let _ = state.status_tx.send(format!(
        "[TRACE] MCP received: {}",
        serde_json::to_string(&payload).unwrap_or_default()
    ));

    // Parse JSON-RPC message
    let message = match JsonRpcMessage::from_value(payload.clone()) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse JSON-RPC message: {:?}", e);
            let response = JsonRpcResponse::error(None, e);
            return Json(response).into_response();
        }
    };

    // Handle based on message type
    match message {
        JsonRpcMessage::Request(req) => {
            let request_id = req.id.clone();
            let method = req.method.clone();

            debug!("MCP request: method={}, id={:?}", method, request_id);

            // Route to appropriate handler
            let result = match method.as_str() {
                "initialize" => handle_initialize(&state, req.params, &payload).await,
                "ping" => handle_ping(),
                "resources/list" => handle_resources_list(&state, &payload).await,
                "resources/read" => handle_resources_read(&state, req.params, &payload).await,
                "resources/subscribe" => handle_resources_subscribe(&state, req.params, &payload).await,
                "resources/unsubscribe" => handle_resources_unsubscribe(&state, req.params).await,
                "resources/templates/list" => handle_resources_templates_list(&state, &payload).await,
                "tools/list" => handle_tools_list(&state, &payload).await,
                "tools/call" => handle_tools_call(&state, req.params, &payload).await,
                "prompts/list" => handle_prompts_list(&state, &payload).await,
                "prompts/get" => handle_prompts_get(&state, req.params, &payload).await,
                "logging/setLevel" => handle_logging_set_level(&state, req.params).await,
                "completion/complete" => handle_completion(&state, req.params, &payload).await,
                _ => Err(JsonRpcError::new(ErrorCode::MethodNotFound)),
            };

            let response = match result {
                Ok(value) => {
                    trace!("MCP response success: {}",
                        serde_json::to_string_pretty(&value).unwrap_or_default());
                    JsonRpcResponse::success(request_id, value)
                }
                Err(e) => {
                    error!("MCP error: code={}, message={}", e.code, e.message);
                    let _ = state.status_tx.send(format!(
                        "[ERROR] MCP error: {}",
                        e.message
                    ));
                    JsonRpcResponse::error(request_id, e)
                }
            };

            Json(response).into_response()
        }
        JsonRpcMessage::Notification(notif) => {
            let method = notif.method.clone();
            debug!("MCP notification: method={}", method);

            // Handle notifications (no response)
            match method.as_str() {
                "notifications/initialized" => {
                    handle_initialized(&state).await;
                }
                "notifications/cancelled" => {
                    handle_cancelled(&state, notif.params).await;
                }
                "notifications/progress" => {
                    handle_progress(&state, notif.params).await;
                }
                _ => {
                    debug!("Unknown MCP notification: {}", method);
                }
            }

            // Notifications don't return responses
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

/// Handle initialize request - LLM declares capabilities
#[cfg(feature = "mcp")]
async fn handle_initialize(
    state: &McpServerState,
    params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    info!("MCP initialize request");

    // Extract client info from params
    let client_info = params
        .as_ref()
        .and_then(|p| p.get("clientInfo"))
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    debug!("MCP client: {}", client_info);
    let _ = state.status_tx.send(format!("→ MCP client initializing: {}", client_info));

    // Create connection for tracking
    let connection_id = ConnectionId::new();
    let session_id = uuid::Uuid::new_v4().to_string();

    // Track in app_state
    state.app_state.add_connection_to_server(
        state.server_id,
        crate::state::ConnectionState {
            id: connection_id,
            remote_addr: state.local_addr, // HTTP doesn't have clear remote addr
            local_addr: state.local_addr,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: std::time::Instant::now(),
            status: ConnectionStatus::Active,
            status_changed_at: std::time::Instant::now(),
            protocol_info: ProtocolConnectionInfo::empty(),
        },
    ).await;

    // Create session
    let session = McpSession::new(session_id.clone(), connection_id);
    state.sessions.lock().await.insert(session_id, Arc::new(Mutex::new(session)));

    // Create event for LLM
    let event = Event::new(&MCP_INITIALIZE_EVENT, serde_json::json!({
        "method": "initialize",
        "client_info": client_info,
        "protocol_version": params.as_ref().and_then(|p| p.get("protocolVersion")).and_then(|v| v.as_str()).unwrap_or("unknown"),
        "capabilities": params.as_ref().and_then(|p| p.get("capabilities")),
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    debug!("MCP calling LLM for initialize request");
    let _ = state.status_tx.send("[DEBUG] MCP calling LLM for initialize request".to_string());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Display messages from LLM
    for message in &execution_result.messages {
        info!("{}", message);
        let _ = state.status_tx.send(format!("[INFO] {}", message));
    }

    debug!("MCP got {} protocol results", execution_result.protocol_results.len());
    let _ = state.status_tx.send(format!("[DEBUG] MCP got {} protocol results", execution_result.protocol_results.len()));

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_initialize" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default response if LLM doesn't provide one
    Ok(serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "resources": {},
            "tools": {},
            "prompts": {}
        },
        "serverInfo": {
            "name": "netget-mcp",
            "version": "0.1.0"
        }
    }))
}

/// Handle ping request - simple health check
#[cfg(feature = "mcp")]
fn handle_ping() -> Result<Value, JsonRpcError> {
    Ok(serde_json::json!({}))
}

/// Handle resources/list request - LLM returns available resources
#[cfg(feature = "mcp")]
async fn handle_resources_list(
    state: &McpServerState,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    debug!("MCP resources/list");
    let _ = state.status_tx.send("[DEBUG] MCP resources/list request".to_string());

    // Create event for LLM
    let event = Event::new(&MCP_RESOURCES_LIST_EVENT, serde_json::json!({
        "method": "resources/list",
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_resources_list" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: empty resources list
    Ok(serde_json::json!({"resources": []}))
}

/// Handle resources/read request - LLM returns resource content
#[cfg(feature = "mcp")]
async fn handle_resources_read(
    state: &McpServerState,
    params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    let uri = params
        .as_ref()
        .and_then(|p| p.get("uri"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| JsonRpcError::new(ErrorCode::InvalidParams))?;

    debug!("MCP resources/read: uri={}", uri);
    let _ = state.status_tx.send(format!("[DEBUG] MCP resources/read: {}", uri));

    // Create event for LLM
    let event = Event::new(&MCP_RESOURCES_READ_EVENT, serde_json::json!({
        "method": "resources/read",
        "uri": uri,
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_resources_read" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: resource not found
    Err(JsonRpcError::custom(
        ErrorCode::InternalError,
        format!("Resource not found: {}", uri),
    ))
}

/// Handle resources/subscribe request
#[cfg(feature = "mcp")]
async fn handle_resources_subscribe(
    _state: &McpServerState,
    params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    let uri = params
        .as_ref()
        .and_then(|p| p.get("uri"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| JsonRpcError::new(ErrorCode::InvalidParams))?;

    debug!("MCP resources/subscribe: uri={}", uri);

    // TODO: Add LLM integration for subscription management
    Ok(serde_json::json!({}))
}

/// Handle resources/unsubscribe request
#[cfg(feature = "mcp")]
async fn handle_resources_unsubscribe(
    _state: &McpServerState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let uri = params
        .as_ref()
        .and_then(|p| p.get("uri"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| JsonRpcError::new(ErrorCode::InvalidParams))?;

    debug!("MCP resources/unsubscribe: uri={}", uri);
    Ok(serde_json::json!({}))
}

/// Handle resources/templates/list request
#[cfg(feature = "mcp")]
async fn handle_resources_templates_list(
    _state: &McpServerState,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    debug!("MCP resources/templates/list");

    // TODO: Add LLM integration
    Ok(serde_json::json!({
        "resourceTemplates": []
    }))
}

/// Handle tools/list request - LLM returns available tools
#[cfg(feature = "mcp")]
async fn handle_tools_list(
    state: &McpServerState,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    debug!("MCP tools/list");
    let _ = state.status_tx.send("[DEBUG] MCP tools/list request".to_string());

    // Create event for LLM
    let event = Event::new(&MCP_TOOLS_LIST_EVENT, serde_json::json!({
        "method": "tools/list",
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_tools_list" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: empty tools list
    Ok(serde_json::json!({"tools": []}))
}

/// Handle tools/call request - LLM executes tool
#[cfg(feature = "mcp")]
async fn handle_tools_call(
    state: &McpServerState,
    params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    let tool_name = params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| JsonRpcError::new(ErrorCode::InvalidParams))?;

    let tool_arguments = params
        .as_ref()
        .and_then(|p| p.get("arguments"));

    debug!("MCP tools/call: name={}", tool_name);
    let _ = state.status_tx.send(format!("[DEBUG] MCP tools/call: {}", tool_name));

    // Create event for LLM
    let event = Event::new(&MCP_TOOLS_CALL_EVENT, serde_json::json!({
        "method": "tools/call",
        "name": tool_name,
        "arguments": tool_arguments,
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_tools_call" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: tool execution failed
    Err(JsonRpcError::custom(
        ErrorCode::InternalError,
        format!("Tool execution failed: {}", tool_name),
    ))
}

/// Handle prompts/list request - LLM returns available prompts
#[cfg(feature = "mcp")]
async fn handle_prompts_list(
    state: &McpServerState,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    debug!("MCP prompts/list");
    let _ = state.status_tx.send("[DEBUG] MCP prompts/list request".to_string());

    // Create event for LLM
    let event = Event::new(&MCP_PROMPTS_LIST_EVENT, serde_json::json!({
        "method": "prompts/list",
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_prompts_list" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: empty prompts list
    Ok(serde_json::json!({"prompts": []}))
}

/// Handle prompts/get request - LLM returns prompt template
#[cfg(feature = "mcp")]
async fn handle_prompts_get(
    state: &McpServerState,
    params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    let prompt_name = params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| JsonRpcError::new(ErrorCode::InvalidParams))?;

    let prompt_arguments = params
        .as_ref()
        .and_then(|p| p.get("arguments"));

    debug!("MCP prompts/get: name={}", prompt_name);
    let _ = state.status_tx.send(format!("[DEBUG] MCP prompts/get: {}", prompt_name));

    // Create event for LLM
    let event = Event::new(&MCP_PROMPTS_GET_EVENT, serde_json::json!({
        "method": "prompts/get",
        "name": prompt_name,
        "arguments": prompt_arguments,
    }));

    // Get protocol actions
    let protocol = Arc::new(McpProtocol::new());

    // Call LLM with action system
    let execution_result = match call_llm(
        &state.llm_client,
        &state.app_state,
        state.server_id,
        None,
        &event,
        protocol.as_ref(),
    ).await {
        Ok(result) => result,
        Err(e) => {
            error!("MCP LLM call failed: {}", e);
            let _ = state.status_tx.send(format!("[ERROR] MCP LLM call failed: {}", e));
            return Err(JsonRpcError {
                code: ErrorCode::InternalError as i32,
                message: "Internal server error".to_string(),
                data: Some(serde_json::json!({"error": e.to_string()})),
            });
        }
    };

    // Process action results
    for protocol_result in &execution_result.protocol_results {
        use crate::llm::actions::protocol_trait::ActionResult;
        if let ActionResult::Custom { name, data } = protocol_result {
            if name == "mcp_prompts_get" {
                if let Some(response) = data.get("response") {
                    return Ok(response.clone());
                }
            }
        }
    }

    // Default: prompt not found
    Err(JsonRpcError::custom(
        ErrorCode::InternalError,
        format!("Prompt not found: {}", prompt_name),
    ))
}

/// Handle logging/setLevel request
#[cfg(feature = "mcp")]
async fn handle_logging_set_level(
    state: &McpServerState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let level = params
        .as_ref()
        .and_then(|p| p.get("level"))
        .and_then(|l| l.as_str())
        .unwrap_or("info");

    debug!("MCP logging/setLevel: level={}", level);
    let _ = state.status_tx.send(format!("[INFO] MCP log level set to: {}", level));

    Ok(serde_json::json!({}))
}

/// Handle completion/complete request - LLM provides completions
#[cfg(feature = "mcp")]
async fn handle_completion(
    _state: &McpServerState,
    _params: Option<Value>,
    _full_request: &Value,
) -> Result<Value, JsonRpcError> {
    debug!("MCP completion/complete");

    // TODO: Add LLM integration
    Ok(serde_json::json!({
        "completion": {
            "values": [],
            "total": 0,
            "hasMore": false
        }
    }))
}

/// Handle initialized notification
#[cfg(feature = "mcp")]
async fn handle_initialized(state: &McpServerState) {
    info!("MCP client initialized");
    let _ = state.status_tx.send("[INFO] MCP client initialized".to_string());
}

/// Handle cancelled notification
#[cfg(feature = "mcp")]
async fn handle_cancelled(state: &McpServerState, params: Option<Value>) {
    if let Some(req_id) = params.as_ref().and_then(|p| p.get("requestId")) {
        debug!("MCP operation cancelled: {:?}", req_id);
        let _ = state.status_tx.send(format!("[DEBUG] MCP cancelled: {:?}", req_id));
    }
}

/// Handle progress notification
#[cfg(feature = "mcp")]
async fn handle_progress(_state: &McpServerState, params: Option<Value>) {
    if let Some(progress) = params {
        trace!("MCP progress: {}", progress);
    }
}
