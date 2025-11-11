//! OpenAPI 3.1 spec-driven HTTP server implementation
//!
//! The LLM provides an OpenAPI specification and generates responses based on validated requests.
//! Supports both spec-compliant and intentionally non-compliant responses for testing/honeypot purposes.

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
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::openapi::actions::OpenApiProtocol;
use crate::state::app_state::AppState;

#[cfg(feature = "openapi")]
use matchit::Router;
#[cfg(feature = "openapi")]
use openapi_rs::model::parse::OpenAPI;

/// Metadata for a matched route
#[cfg(feature = "openapi")]
#[derive(Clone, Debug)]
pub struct RouteMetadata {
    pub operation_id: Option<String>,
    pub method: String,
    pub path_template: String,
    pub operation_json: serde_json::Value, // Pre-serialized operation for LLM
}

/// Result of route matching
#[cfg(feature = "openapi")]
#[derive(Debug)]
pub enum MatchResult {
    /// Route found and matched
    Found {
        metadata: RouteMetadata,
        params: HashMap<String, String>,
    },
    /// Path exists but method not allowed
    MethodNotAllowed { allowed_methods: Vec<String> },
    /// Path not found in spec
    NotFound,
}

/// OpenAPI server state
pub struct OpenApiState {
    /// Raw OpenAPI specification (YAML or JSON)
    pub spec: Option<String>,
    /// Whether the spec has been successfully parsed
    pub spec_valid: bool,
    /// Parsed OpenAPI specification
    #[cfg(feature = "openapi")]
    pub parsed_spec: Option<OpenAPI>,
    /// Route matcher for fast path matching
    #[cfg(feature = "openapi")]
    pub router: Option<Router<RouteMetadata>>,
    /// Whether to ask LLM for invalid requests (404/405/400)
    pub llm_on_invalid: bool,
}

impl OpenApiState {
    pub fn new() -> Self {
        Self {
            spec: None,
            spec_valid: false,
            #[cfg(feature = "openapi")]
            parsed_spec: None,
            #[cfg(feature = "openapi")]
            router: None,
            llm_on_invalid: false, // Default: bypass LLM for errors
        }
    }
}

/// Build matchit router from OpenAPI specification
#[cfg(feature = "openapi")]
fn build_router(spec: &OpenAPI) -> anyhow::Result<Router<RouteMetadata>> {
    let mut router = Router::new();

    for (path_template, path_item) in &spec.paths {
        // Iterate over operations HashMap (keys: "get", "post", etc.)
        for (method_lower, operation) in &path_item.operations {
            let method = method_lower.to_uppercase();

            // Serialize operation to JSON for LLM
            let operation_json = serde_json::to_value(operation)?;

            let metadata = RouteMetadata {
                operation_id: operation.operation_id.clone(),
                method: method.clone(),
                path_template: path_template.clone(),
                operation_json,
            };

            // Use composite key: METHOD:PATH (e.g., "GET:/users/{id}")
            let route_key = format!("{}:{}", method, path_template);
            router.insert(&route_key, metadata)?;

            debug!("Registered OpenAPI route: {} {}", method, path_template);
        }
    }

    Ok(router)
}

/// Match incoming request against router
#[cfg(feature = "openapi")]
fn match_route(
    router: &Router<RouteMetadata>,
    method: &str,
    path: &str,
) -> MatchResult {
    // Try exact method:path match
    let route_key = format!("{}:{}", method, path);

    match router.at(&route_key) {
        Ok(matched) => {
            // Extract path parameters
            let params: HashMap<String, String> = matched
                .params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            MatchResult::Found {
                metadata: matched.value.clone(),
                params,
            }
        }
        Err(_) => {
            // Check if path exists with different method (for 405)
            let allowed_methods = find_allowed_methods(router, path);

            if !allowed_methods.is_empty() {
                MatchResult::MethodNotAllowed { allowed_methods }
            } else {
                MatchResult::NotFound
            }
        }
    }
}

/// Find which HTTP methods are allowed for a given path
#[cfg(feature = "openapi")]
fn find_allowed_methods(router: &Router<RouteMetadata>, path: &str) -> Vec<String> {
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"];
    let mut allowed = Vec::new();

    for method in &methods {
        let route_key = format!("{}:{}", method, path);
        if router.at(&route_key).is_ok() {
            allowed.push(method.to_string());
        }
    }

    allowed
}

/// Validate request against OpenAPI operation schema
/// Returns Ok(()) if valid, Err(error_message) if invalid
#[cfg(feature = "openapi")]
fn validate_request(
    _operation_json: &serde_json::Value,
    _method: &str,
    _path: &str,
    _headers: &HashMap<String, String>,
    _body: &str,
) -> anyhow::Result<()> {
    // TODO: Implement schema validation
    // For now, just return Ok - validation can be added later
    // This would involve:
    // 1. Validate required parameters are present
    // 2. Validate parameter types match schema
    // 3. Validate request body against schema if present
    // 4. Validate Content-Type header

    Ok(())
}

/// Create immediate 404 Not Found response
#[cfg(feature = "openapi")]
fn immediate_404() -> Response<Full<Bytes>> {
    Response::builder()
        .status(404)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            json!({
                "error": "Not Found",
                "message": "The requested path does not exist in the OpenAPI specification"
            })
            .to_string(),
        )))
        .unwrap()
}

/// Create immediate 405 Method Not Allowed response with Allow header
#[cfg(feature = "openapi")]
fn immediate_405(allowed_methods: Vec<String>) -> Response<Full<Bytes>> {
    let allow_header = allowed_methods.join(", ");

    Response::builder()
        .status(405)
        .header("Content-Type", "application/json")
        .header("Allow", allow_header.clone())
        .body(Full::new(Bytes::from(
            json!({
                "error": "Method Not Allowed",
                "message": format!("This path does not support the requested method"),
                "allowed_methods": allowed_methods
            })
            .to_string(),
        )))
        .unwrap()
}

/// Create immediate 400 Bad Request response for validation errors
#[cfg(feature = "openapi")]
fn immediate_400(error_message: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(400)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            json!({
                "error": "Bad Request",
                "message": error_message
            })
            .to_string(),
        )))
        .unwrap()
}

/// Handle LLM response and process actions
#[cfg(feature = "openapi")]
async fn handle_llm_response(
    execution_result: crate::llm::actions::executor::ExecutionResult,
    status_tx: mpsc::UnboundedSender<String>,
    openapi_state: Arc<RwLock<OpenApiState>>,
    method: String,
    path: String,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("LLM OpenAPI response received");

    // Display messages
    for msg in execution_result.messages {
        let _ = status_tx.send(msg);
    }

    // Default response
    let mut status_code = 200;
    let mut response_headers = HashMap::new();
    let mut response_body = String::new();
    let mut spec_to_load: Option<String> = None;

    // Process protocol results
    for protocol_result in execution_result.protocol_results {
        match protocol_result {
            ActionResult::Custom { name, data } => {
                match name.as_str() {
                    "send_openapi_response" => {
                        // Extract response details
                        if let Some(status) = data.get("status_code").and_then(|v| v.as_u64()) {
                            status_code = status as u16;
                        }
                        if let Some(headers_obj) = data.get("headers").and_then(|v| v.as_object()) {
                            for (k, v) in headers_obj {
                                if let Some(v_str) = v.as_str() {
                                    response_headers.insert(k.clone(), v_str.to_string());
                                }
                            }
                        }
                        if let Some(body) = data.get("body").and_then(|v| v.as_str()) {
                            response_body = body.to_string();
                        }
                    }
                    "send_validation_error" => {
                        // Extract error details
                        if let Some(status) = data.get("status_code").and_then(|v| v.as_u64()) {
                            status_code = status as u16;
                        }
                        if let Some(message) = data.get("message").and_then(|v| v.as_str()) {
                            response_body = json!({
                                "error": message
                            }).to_string();
                            response_headers.insert("content-type".to_string(), "application/json".to_string());
                        }
                    }
                    "load_openapi_spec" | "reload_spec" => {
                        // LLM provided OpenAPI spec
                        if let Some(spec) = data.get("spec").and_then(|v| v.as_str()) {
                            spec_to_load = Some(spec.to_string());
                        }
                    }
                    "configure_error_handling" => {
                        // LLM configured error handling
                        if let Some(llm_on_invalid) = data.get("llm_on_invalid").and_then(|v| v.as_bool()) {
                            let mut state = openapi_state.write().await;
                            state.llm_on_invalid = llm_on_invalid;
                            console_info!(status_tx, "[INFO] OpenAPI llm_on_invalid: {}", llm_on_invalid);
                        }
                    }
                    _ => {
                        debug!("Unknown custom action: {}", name);
                    }
                }
            }
            ActionResult::Output(output_data) => {
                // Legacy fallback for non-action responses
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
            _ => {}
        }
    }

    // Load spec if provided
    if let Some(spec) = spec_to_load {
        let mut state = openapi_state.write().await;
        // Try to parse spec
        match openapi_rs::model::parse::OpenAPI::yaml(&spec) {
            Ok(parsed_spec) => {
                console_info!(status_tx, "[INFO] OpenAPI spec loaded: {} bytes", spec.len());

                // Build router from parsed spec
                match build_router(&parsed_spec) {
                    Ok(router) => {
                        let route_count = parsed_spec.paths.len();
                        console_info!(status_tx, "[INFO] Built OpenAPI router with {} routes", route_count);

                        state.spec = Some(spec);
                        state.spec_valid = true;
                        state.parsed_spec = Some(parsed_spec);
                        state.router = Some(router);
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to build router: {}", e);
                        state.spec = Some(spec);
                        state.spec_valid = false;
                    }
                }
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] Failed to parse OpenAPI spec: {}", e);
                state.spec = Some(spec);
                state.spec_valid = false;
            }
        }
    }

    // Ensure we always have valid JSON in response if body is empty
    if response_body.is_empty() {
        response_body = json!({
            "message": "OpenAPI server received request but LLM did not generate a response",
            "method": method,
            "path": path
        }).to_string();
        response_headers.insert("content-type".to_string(), "application/json".to_string());
    }

    console_info!(status_tx, "→ OpenAPI {} {} → {} ({} bytes)");

    // Build the HTTP response
    let mut response = Response::builder().status(status_code);

    // Add headers
    for (name, value) in response_headers {
        response = response.header(name, value);
    }

    Ok(response.body(Full::new(Bytes::from(response_body))).unwrap())
}

/// OpenAPI server that uses LLM to handle spec-driven requests
pub struct OpenApiServer;

impl OpenApiServer {
    /// Spawn OpenAPI server with LLM actions
    ///
    /// ## Startup Parameters
    ///
    /// The LLM can provide OpenAPI specification during server initialization via `startup_params`:
    ///
    /// **Option 1: Inline spec (string)**
    /// ```json
    /// {
    ///   "spec": "openapi: 3.1.0\ninfo:\n  title: My API\n..."
    /// }
    /// ```
    ///
    /// **Option 2: Spec file path**
    /// ```json
    /// {
    ///   "spec_file": "/path/to/openapi.yaml"
    /// }
    /// ```
    ///
    /// When a spec is provided via startup_params:
    /// - The spec is immediately parsed and validated
    /// - Route matching is configured automatically
    /// - Invalid requests (404/405) are rejected without asking LLM (unless `llm_on_invalid` is enabled)
    /// - Matched requests receive only the relevant operation spec, not the full spec
    ///
    /// If no spec is provided, the server starts in "dynamic mode" where the LLM can load the spec
    /// later using the `reload_spec` action.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> anyhow::Result<SocketAddr> {
        // Create shared OpenAPI state
        let openapi_state = Arc::new(RwLock::new(OpenApiState::new()));
        let protocol = Arc::new(OpenApiProtocol::new());

        // Check if spec is provided via startup_params (REQUIRED)
        let spec_loaded = if let Some(ref params) = startup_params {
            // Extract required spec parameter
            let spec_content = if let Some(spec_str) = params.get_optional_string("spec") {
                // Spec provided (LLM must read file and provide content)
                console_info!(status_tx, "[INFO] OpenAPI spec provided ({} bytes)", spec_str.len());
                Some(spec_str)
            } else {
                // spec parameter is required
                let msg = "OpenAPI server requires 'spec' parameter in startup_params. LLM must read the spec file and provide content.";
                console_error!(status_tx, "[ERROR] {}", msg);
                return Err(anyhow::anyhow!(msg));
            };

            // If we have spec content, parse and build router
            if let Some(spec) = spec_content {
                let mut state = openapi_state.write().await;
                state.spec = Some(spec.clone());

                match openapi_rs::model::parse::OpenAPI::yaml(&spec) {
                    Ok(parsed) => {
                        match build_router(&parsed) {
                            Ok(router) => {
                                let route_count = parsed.paths.len();
                                console_info!(status_tx, "[INFO] Built OpenAPI router with {} routes", route_count);
                                state.parsed_spec = Some(parsed);
                                state.router = Some(router);
                                state.spec_valid = true;
                                true
                            }
                            Err(e) => {
                                let msg = format!("Failed to build router: {}", e);
                                console_error!(status_tx, "[ERROR] {}", msg);
                                state.spec_valid = false;
                                return Err(anyhow::anyhow!(msg));
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to parse OpenAPI spec: {}", e);
                        console_error!(status_tx, "[ERROR] {}", msg);
                        state.spec_valid = false;
                        return Err(anyhow::anyhow!(msg));
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if !spec_loaded {
            console_info!(status_tx, "[INFO] Starting OpenAPI server...");
        } else {
            console_info!(status_tx, "[INFO] Starting OpenAPI server with pre-loaded spec...");
        }

        // Start HTTP server
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "[INFO] OpenAPI server listening on {}", local_addr);

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        console_info!(status_tx, "[INFO] OpenAPI connection from {}", remote_addr);

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
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
                        let openapi_state_clone = openapi_state.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();
                            let app_state_for_service = app_state_clone.clone();
                            let openapi_state_for_service = openapi_state_clone.clone();

                            // Create a service that handles OpenAPI requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                let openapi_state_clone = openapi_state_for_service.clone();
                                handle_openapi_request(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    openapi_state_clone,
                                    server_id,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving OpenAPI connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] OpenAPI connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to accept OpenAPI connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single OpenAPI request
async fn handle_openapi_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OpenApiProtocol>,
    openapi_state: Arc<RwLock<OpenApiState>>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let path = req.uri().path().to_string();

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

    // DEBUG: Log request summary
    debug!(
        "OpenAPI request: {} {} ({} bytes) from {:?}",
        method,
        path,
        body_bytes.len(),
        connection_id
    );
    console_debug!(status_tx, "[DEBUG] OpenAPI {} {} ({} bytes)");

    // TRACE: Log full request details
    trace!("OpenAPI request headers:");
    for (name, value) in &headers {
        console_trace!(status_tx, "[TRACE] OpenAPI header: {}: {}", name, value);
    }
    if !body_bytes.is_empty() {
        if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
            // Try to pretty-print if it's JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_str) {
                let pretty = serde_json::to_string_pretty(&json).unwrap_or(body_str.to_string());
                console_trace!(status_tx, "[TRACE] OpenAPI request body (JSON):\r\n{}", pretty.replace('\n', "\r\n"));
            } else {
                console_trace!(status_tx, "[TRACE] OpenAPI request body:\r\n{}", body_str.replace('\n', "\r\n"));
            }
        } else {
            console_trace!(status_tx, "[TRACE] OpenAPI request body (binary): {} bytes", body_bytes.len());
        }
    }

    // Get current spec info and perform route matching
    let (spec_info, match_result, llm_on_invalid) = {
        let state = openapi_state.read().await;
        let spec_info = json!({
            "spec_loaded": state.spec.is_some(),
            "spec_valid": state.spec_valid
        });

        #[cfg(feature = "openapi")]
        {
            let match_result = if let Some(router) = state.router.as_ref() {
                Some(match_route(router, &method, &path))
            } else {
                None
            };
            (spec_info, match_result, state.llm_on_invalid)
        }
        #[cfg(not(feature = "openapi"))]
        {
            (spec_info, None, false)
        }
    };

    // Handle route matching results
    #[cfg(feature = "openapi")]
    if let Some(match_result) = match_result {
        match match_result {
            MatchResult::Found { metadata, params } => {
                // Validate request if llm_on_invalid is false
                if !llm_on_invalid {
                    let body_text = String::from_utf8_lossy(&body_bytes);
                    if let Err(e) = validate_request(&metadata.operation_json, &method, &path, &headers, &body_text) {
                        console_warn!(status_tx, "[WARN] OpenAPI validation error: {}", e);
                        return Ok(immediate_400(e.to_string()));
                    }
                }

                // Create event with matched route information
                let body_text = String::from_utf8_lossy(&body_bytes);
                let event = Event::new(&*crate::server::openapi::actions::OPENAPI_REQUEST_EVENT, serde_json::json!({
                    "method": method,
                    "path": path,
                    "uri": uri,
                    "headers": headers,
                    "body": if body_text.is_empty() { "" } else { body_text.as_ref() },
                    "spec_info": spec_info,
                    "matched_route": {
                        "operation_id": metadata.operation_id,
                        "path_template": metadata.path_template,
                        "path_params": params,
                        "operation": metadata.operation_json,
                    }
                }));

                // Call LLM with matched route context
                match call_llm(
                    &llm_client,
                    &app_state,
                    server_id,
                    Some(connection_id),
                    &event,
                    protocol.as_ref(),
                ).await {
                    Ok(execution_result) => {
                        return handle_llm_response(
                            execution_result,
                            status_tx,
                            openapi_state,
                            method,
                            path,
                        ).await;
                    }
                    Err(e) => {
                        console_error!(status_tx, "✗ LLM error for {} {}: {}", method, path, e);
                        return Ok(Response::builder()
                            .status(500)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(json!({
                                "error": "Internal Server Error",
                                "message": "Failed to generate response"
                            }).to_string())))
                            .unwrap());
                    }
                }
            }
            MatchResult::MethodNotAllowed { allowed_methods } => {
                if llm_on_invalid {
                    // Let LLM handle 405 error
                    debug!("OpenAPI 405 Method Not Allowed (LLM will handle): {} {}", method, path);
                } else {
                    // Immediate 405 response
                    console_info!(status_tx, "[INFO] OpenAPI 405: {} {} (allowed: {})", method, path, allowed_methods.join(", "));
                    return Ok(immediate_405(allowed_methods));
                }
            }
            MatchResult::NotFound => {
                if llm_on_invalid {
                    // Let LLM handle 404 error
                    debug!("OpenAPI 404 Not Found (LLM will handle): {} {}", method, path);
                } else {
                    // Immediate 404 response
                    console_info!(status_tx, "[INFO] OpenAPI 404: {} {}", method, path);
                    return Ok(immediate_404());
                }
            }
        }
    }

    // If no router or llm_on_invalid is true for errors, call LLM with basic info
    let body_text = String::from_utf8_lossy(&body_bytes);
    let event = Event::new(&*crate::server::openapi::actions::OPENAPI_REQUEST_EVENT, serde_json::json!({
        "method": method,
        "path": path,
        "uri": uri,
        "headers": headers,
        "body": if body_text.is_empty() { "" } else { body_text.as_ref() },
        "spec_info": spec_info
    }));

    // Call LLM to handle request
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await {
        #[cfg(feature = "openapi")]
        Ok(execution_result) => {
            handle_llm_response(
                execution_result,
                status_tx,
                openapi_state,
                method,
                path,
            ).await
        }
        #[cfg(not(feature = "openapi"))]
        Ok(execution_result) => {
            // Fallback for when openapi feature is disabled
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "message": "OpenAPI feature not enabled"
                }).to_string())))
                .unwrap())
        }
        Err(e) => {
            console_error!(status_tx, "✗ LLM error for {} {}: {}", method, path, e);

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "Internal Server Error",
                    "message": "Failed to generate response"
                }).to_string())))
                .unwrap())
        }
    }
}
