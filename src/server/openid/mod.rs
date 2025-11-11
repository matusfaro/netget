//! OpenID Connect server implementation
//!
//! The LLM controls all OpenID Connect endpoints and generates responses including
//! discovery documents, authorization codes, JWT tokens, and user info.

pub mod actions;

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
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::openid::actions::OpenIdProtocol;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// OpenID Connect provider state
pub struct OpenIdState {
    /// Issuer URL
    pub issuer: Option<String>,
    /// Supported OAuth scopes
    pub supported_scopes: Vec<String>,
}

impl OpenIdState {
    pub fn new() -> Self {
        Self {
            issuer: None,
            supported_scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
        }
    }
}

/// Determine OIDC endpoint type from request path
fn classify_endpoint(path: &str) -> &'static str {
    match path {
        "/.well-known/openid-configuration" => "discovery",
        "/authorize" => "authorization",
        "/token" => "token",
        "/userinfo" => "userinfo",
        "/jwks.json" | "/jwks" => "jwks",
        _ => "unknown",
    }
}

/// Parse query string into key-value pairs
fn parse_query_string(query: Option<&str>) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                params.insert(
                    urlencoding::decode(key).unwrap_or_default().to_string(),
                    urlencoding::decode(value).unwrap_or_default().to_string(),
                );
            }
        }
    }
    params
}

/// Parse URL-encoded form data
fn parse_form_data(body: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for pair in body.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(
                urlencoding::decode(key).unwrap_or_default().to_string(),
                urlencoding::decode(value).unwrap_or_default().to_string(),
            );
        }
    }
    params
}

/// Handle LLM response and process actions
async fn handle_llm_response(
    execution_result: crate::llm::actions::executor::ExecutionResult,
    status_tx: mpsc::UnboundedSender<String>,
    method: String,
    path: String,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("LLM OpenID response received");

    // Display messages
    for msg in execution_result.messages {
        let _ = status_tx.send(msg);
    }

    // Default response
    let mut status_code = 200;
    let mut response_headers = HashMap::new();
    let mut response_body = String::new();
    let mut redirect_location: Option<String> = None;

    // Process protocol results
    for protocol_result in execution_result.protocol_results {
        match protocol_result {
            crate::llm::actions::protocol_trait::ActionResult::Custom { name, data } => {
                match name.as_str() {
                    "send_discovery_document" => {
                        // Build OpenID Connect discovery document
                        let mut discovery = json!({
                            "issuer": data["issuer"],
                            "authorization_endpoint": data["authorization_endpoint"],
                            "token_endpoint": data["token_endpoint"],
                            "userinfo_endpoint": data["userinfo_endpoint"],
                            "jwks_uri": data["jwks_uri"],
                            "response_types_supported": data.get("supported_response_types").cloned().unwrap_or(json!(["code", "id_token", "token id_token"])),
                            "subject_types_supported": ["public"],
                            "id_token_signing_alg_values_supported": ["RS256"],
                        });

                        if let Some(scopes) = data.get("supported_scopes") {
                            discovery["scopes_supported"] = scopes.clone();
                        }

                        response_body = serde_json::to_string_pretty(&discovery)
                            .unwrap_or_else(|_| discovery.to_string());
                        response_headers.insert("content-type".to_string(), "application/json".to_string());
                        status_code = 200;
                    }
                    "send_authorization_response" => {
                        // Build redirect URL with query parameters
                        let redirect_uri = data["redirect_uri"].as_str().unwrap_or("");
                        let mut redirect_url = redirect_uri.to_string();
                        let mut params = Vec::new();

                        if let Some(code) = data.get("code").and_then(|v| v.as_str()) {
                            params.push(format!("code={}", urlencoding::encode(code)));
                        }
                        if let Some(state) = data.get("state").and_then(|v| v.as_str()) {
                            params.push(format!("state={}", urlencoding::encode(state)));
                        }
                        if let Some(error) = data.get("error").and_then(|v| v.as_str()) {
                            params.push(format!("error={}", urlencoding::encode(error)));
                        }
                        if let Some(error_desc) = data.get("error_description").and_then(|v| v.as_str()) {
                            params.push(format!("error_description={}", urlencoding::encode(error_desc)));
                        }

                        if !params.is_empty() {
                            let separator = if redirect_url.contains('?') { "&" } else { "?" };
                            redirect_url = format!("{}{}{}", redirect_url, separator, params.join("&"));
                        }

                        redirect_location = Some(redirect_url.clone());
                        status_code = 302;
                        response_headers.insert("location".to_string(), redirect_url);
                    }
                    "send_token_response" => {
                        // Build OAuth token response
                        let mut token_response = json!({
                            "access_token": data["access_token"],
                            "token_type": data.get("token_type").cloned().unwrap_or(json!("Bearer")),
                        });

                        if let Some(id_token) = data.get("id_token") {
                            token_response["id_token"] = id_token.clone();
                        }
                        if let Some(refresh_token) = data.get("refresh_token") {
                            token_response["refresh_token"] = refresh_token.clone();
                        }
                        if let Some(expires_in) = data.get("expires_in") {
                            token_response["expires_in"] = expires_in.clone();
                        }
                        if let Some(scope) = data.get("scope") {
                            token_response["scope"] = scope.clone();
                        }

                        response_body = token_response.to_string();
                        response_headers.insert("content-type".to_string(), "application/json".to_string());
                        response_headers.insert("cache-control".to_string(), "no-store".to_string());
                        response_headers.insert("pragma".to_string(), "no-cache".to_string());
                        status_code = 200;
                    }
                    "send_userinfo_response" => {
                        // Build userinfo response
                        let mut userinfo = json!({
                            "sub": data["sub"],
                        });

                        if let Some(name) = data.get("name") {
                            userinfo["name"] = name.clone();
                        }
                        if let Some(email) = data.get("email") {
                            userinfo["email"] = email.clone();
                        }
                        if let Some(email_verified) = data.get("email_verified") {
                            userinfo["email_verified"] = email_verified.clone();
                        }
                        if let Some(picture) = data.get("picture") {
                            userinfo["picture"] = picture.clone();
                        }
                        if let Some(additional_claims) = data.get("additional_claims").and_then(|v| v.as_object()) {
                            for (k, v) in additional_claims {
                                userinfo[k] = v.clone();
                            }
                        }

                        response_body = userinfo.to_string();
                        response_headers.insert("content-type".to_string(), "application/json".to_string());
                        status_code = 200;
                    }
                    "send_jwks_response" => {
                        // Build JWKS response
                        let jwks = json!({
                            "keys": data.get("keys").cloned().unwrap_or(json!([]))
                        });

                        response_body = jwks.to_string();
                        response_headers.insert("content-type".to_string(), "application/json".to_string());
                        status_code = 200;
                    }
                    "send_error_response" => {
                        // Build OAuth error response
                        let error_response = json!({
                            "error": data["error"],
                            "error_description": data.get("error_description").cloned().unwrap_or(json!("")),
                        });

                        response_body = error_response.to_string();
                        response_headers.insert("content-type".to_string(), "application/json".to_string());
                        status_code = data.get("status_code").and_then(|v| v.as_u64()).unwrap_or(400) as u16;
                    }
                    _ => {
                        debug!("Unknown custom action: {}", name);
                    }
                }
            }
            _ => {}
        }
    }

    // Ensure we always have valid response
    if response_body.is_empty() && redirect_location.is_none() {
        response_body = json!({
            "error": "server_error",
            "error_description": "OpenID server received request but LLM did not generate a response"
        }).to_string();
        response_headers.insert("content-type".to_string(), "application/json".to_string());
        status_code = 500;
    }

    console_info!(status_tx, "→ OpenID {} {} → {} ({} bytes{})");

    // Build the HTTP response
    let mut response = Response::builder().status(status_code);

    // Add headers
    for (name, value) in response_headers {
        response = response.header(name, value);
    }

    Ok(response.body(Full::new(Bytes::from(response_body))).unwrap())
}

/// OpenID Connect server that uses LLM to handle all endpoints
pub struct OpenIdServer;

impl OpenIdServer {
    /// Spawn OpenID Connect server with LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> anyhow::Result<SocketAddr> {
        // Create shared OpenID state
        let openid_state = Arc::new(RwLock::new(OpenIdState::new()));
        let protocol = Arc::new(OpenIdProtocol::new());

        // Configure from startup params if provided
        if let Some(ref params) = startup_params {
            let mut state = openid_state.write().await;

            if let Some(issuer) = params.get_optional_string("issuer") {
                console_info!(status_tx, "[INFO] OpenID issuer: {}", issuer);
                state.issuer = Some(issuer);
            }

            if let Some(scopes_array) = params.get_optional_array("supported_scopes") {
                let scopes: Vec<String> = scopes_array
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if !scopes.is_empty() {
                    console_info!(status_tx, "[INFO] OpenID scopes: {:?}", scopes);
                    state.supported_scopes = scopes;
                }
            }
        }

        console_info!(status_tx, "[INFO] Starting OpenID Connect server...");

        // Start HTTP server
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "[INFO] OpenID server listening on {}", local_addr);

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        console_info!(status_tx, "[INFO] OpenID connection from {}", remote_addr);

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
                                "endpoint": None::<String>,
                                "authenticated": false,
                            })),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        console_info!(status_tx, "__UPDATE_UI__");

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let openid_state_clone = openid_state.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();
                            let app_state_for_service = app_state_clone.clone();
                            let openid_state_for_service = openid_state_clone.clone();

                            // Create a service that handles OpenID requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                let openid_state_clone = openid_state_for_service.clone();
                                handle_openid_request(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    openid_state_clone,
                                    server_id,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving OpenID connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] OpenID connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to accept OpenID connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single OpenID Connect request
async fn handle_openid_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OpenIdProtocol>,
    _openid_state: Arc<RwLock<OpenIdState>>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details (before consuming body)
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let query_str = req.uri().query().map(|s| s.to_string());

    // Classify endpoint
    let endpoint_type = classify_endpoint(&path);

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
    let body_text = String::from_utf8_lossy(&body_bytes).to_string();

    // Parse query parameters
    let query_params = parse_query_string(query_str.as_deref());

    // Parse form data if Content-Type is application/x-www-form-urlencoded
    let form_data = if headers.get("content-type")
        .map(|v| v.contains("application/x-www-form-urlencoded"))
        .unwrap_or(false)
    {
        parse_form_data(&body_text)
    } else {
        HashMap::new()
    };

    // DEBUG: Log request summary
    debug!(
        "OpenID request: {} {} (endpoint: {}, {} bytes) from {:?}",
        method,
        path,
        endpoint_type,
        body_bytes.len(),
        connection_id
    );
    console_debug!(status_tx, "[DEBUG] OpenID {} {} ({})");

    // TRACE: Log full request details
    trace!("OpenID request headers:");
    for (name, value) in &headers {
        trace!("  {}: {}", name, value);
    }
    if !body_bytes.is_empty() {
        trace!("OpenID request body: {}", body_text);
    }
    if !query_params.is_empty() {
        trace!("OpenID query params: {:?}", query_params);
    }
    if !form_data.is_empty() {
        trace!("OpenID form data: {:?}", form_data);
    }

    // Create event for LLM
    let event = Event::new(
        &*crate::server::openid::actions::OPENID_REQUEST_EVENT,
        json!({
            "method": method,
            "path": path,
            "query_params": query_params,
            "headers": headers,
            "body": if body_text.is_empty() { "" } else { &body_text },
            "form_data": form_data,
            "endpoint_type": endpoint_type,
        }),
    );

    // Call LLM to handle request
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await {
        Ok(execution_result) => {
            handle_llm_response(
                execution_result,
                status_tx,
                method,
                path,
            ).await
        }
        Err(e) => {
            console_error!(status_tx, "✗ LLM error for {} {}: {}", method, path, e);

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "server_error",
                    "error_description": "Failed to generate response"
                }).to_string())))
                .unwrap())
        }
    }
}
