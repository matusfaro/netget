//! OAuth2 authorization server implementation
//!
//! OAuth2 server implementing RFC 6749 (OAuth 2.0 Authorization Framework).
//! The LLM controls authorization decisions, token generation, and client validation.

pub mod actions;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::{Incoming, Body};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::server::connection::ConnectionId;
use crate::server::oauth2::actions::{OAuth2Protocol, OAUTH2_AUTHORIZE_EVENT, OAUTH2_TOKEN_EVENT, OAUTH2_INTROSPECT_EVENT, OAUTH2_REVOKE_EVENT};
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// OAuth2 authorization server
pub struct OAuth2Server;

impl OAuth2Server {
    /// Spawn the OAuth2 server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("OAuth2 server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] OAuth2 server listening on {}", local_addr));

        let protocol = Arc::new(OAuth2Protocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("OAuth2 connection {} from {}", connection_id, remote_addr);

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        use serde_json::json;
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
                            protocol_info: ProtocolConnectionInfo::new(json!({
                                "recent_requests": []
                            })),
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

                            // Create a service that handles OAuth2 requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_oauth2_request(
                                    req,
                                    connection_id,
                                    server_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving OAuth2 connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("✗ OAuth2 connection {connection_id} closed"));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept OAuth2 connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single OAuth2 request
async fn handle_oauth2_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OAuth2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();

    debug!("OAuth2 request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] OAuth2 {} {}", method, path));

    // Track request in connection info
    app_state.update_connection_stats(server_id, connection_id, None, None, Some(1), None).await;

    // Route the request
    let response = match (method.clone(), path) {
        (Method::GET, "/authorize") | (Method::POST, "/authorize") => {
            handle_authorize_request(req, connection_id, server_id, llm_client, app_state.clone(), status_tx.clone(), protocol).await
        }
        (Method::POST, "/token") => {
            handle_token_request(req, connection_id, server_id, llm_client, app_state.clone(), status_tx.clone(), protocol).await
        }
        (Method::POST, "/introspect") => {
            handle_introspect_request(req, connection_id, server_id, llm_client, app_state.clone(), status_tx.clone(), protocol).await
        }
        (Method::POST, "/revoke") => {
            handle_revoke_request(req, connection_id, server_id, llm_client, app_state.clone(), status_tx.clone(), protocol).await
        }
        _ => {
            debug!("OAuth2: Unknown endpoint {} {}", method, path);
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "invalid_request",
                    "error_description": "Unknown endpoint"
                }).to_string())))
                .unwrap())
        }
    };

    // Update connection stats
    if let Ok(ref resp) = response {
        let body_size = resp.body().size_hint().exact().unwrap_or(0);
        app_state.update_connection_stats(server_id, connection_id, None, Some(body_size), None, Some(1)).await;
    }

    let _ = status_tx.send("__UPDATE_UI__".to_string());
    response
}

/// Handle /authorize endpoint (RFC 6749 Section 3.1)
async fn handle_authorize_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OAuth2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let uri = req.uri().clone();
    let method = req.method().clone();

    // Parse query parameters (GET) or form body (POST)
    let params = if method == Method::GET {
        parse_query_params(uri.query().unwrap_or(""))
    } else {
        // Read body for POST
        match req.into_body().collect().await {
            Ok(body) => {
                let body_bytes = body.to_bytes();
                let body_str = String::from_utf8_lossy(&body_bytes);
                parse_query_params(&body_str)
            }
            Err(_) => HashMap::new(),
        }
    };

    debug!("OAuth2 authorize request: {:?}", params);
    let _ = status_tx.send(format!("[DEBUG] OAuth2 authorize: response_type={:?}, client_id={:?}",
        params.get("response_type"), params.get("client_id")));

    // Create LLM event
    let event = Event::new(&OAUTH2_AUTHORIZE_EVENT, json!({
        "response_type": params.get("response_type").cloned().unwrap_or_default(),
        "client_id": params.get("client_id").cloned().unwrap_or_default(),
        "redirect_uri": params.get("redirect_uri").cloned().unwrap_or_default(),
        "scope": params.get("scope").cloned().unwrap_or_default(),
        "state": params.get("state").cloned().unwrap_or_default(),
    }));

    // Call LLM
    match call_llm(&llm_client, &app_state, server_id, Some(connection_id), &event, &*protocol).await {
        Ok(_) => {
            // LLM should have returned oauth2_authorize_response action
            // Extract response from action results
            // For now, return a default authorization code response
            let code = "AUTH_CODE_123"; // LLM should generate this
            let state = params.get("state").cloned().unwrap_or_default();
            let redirect_uri = params.get("redirect_uri").cloned().unwrap_or("urn:ietf:wg:oauth:2.0:oob".to_string());

            let location = if redirect_uri.contains('?') {
                format!("{}&code={}&state={}", redirect_uri, code, state)
            } else {
                format!("{}?code={}&state={}", redirect_uri, code, state)
            };

            info!("OAuth2 authorization approved, redirecting to {}", location);
            Ok(Response::builder()
                .status(StatusCode::FOUND)
                .header("Location", location)
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Err(e) => {
            error!("OAuth2 authorization error: {}", e);
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "server_error",
                    "error_description": format!("{}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle /token endpoint (RFC 6749 Section 3.2)
async fn handle_token_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OAuth2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Parse form body
    let body_bytes = match req.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "invalid_request",
                    "error_description": "Failed to read request body"
                }).to_string())))
                .unwrap());
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes);
    let params = parse_query_params(&body_str);

    debug!("OAuth2 token request: {:?}", params);
    let _ = status_tx.send(format!("[DEBUG] OAuth2 token: grant_type={:?}, client_id={:?}",
        params.get("grant_type"), params.get("client_id")));

    // Create LLM event
    let event = Event::new(&OAUTH2_TOKEN_EVENT, json!({
        "grant_type": params.get("grant_type").cloned().unwrap_or_default(),
        "code": params.get("code").cloned().unwrap_or_default(),
        "redirect_uri": params.get("redirect_uri").cloned().unwrap_or_default(),
        "client_id": params.get("client_id").cloned().unwrap_or_default(),
        "client_secret": params.get("client_secret").cloned().unwrap_or_default(),
        "refresh_token": params.get("refresh_token").cloned().unwrap_or_default(),
        "username": params.get("username").cloned().unwrap_or_default(),
        "password": params.get("password").cloned().unwrap_or_default(),
        "scope": params.get("scope").cloned().unwrap_or_default(),
    }));

    // Call LLM
    match call_llm(&llm_client, &app_state, server_id, Some(connection_id), &event, &*protocol).await {
        Ok(_) => {
            // LLM should have returned oauth2_token_response action
            // For now, return a default token response
            info!("OAuth2 token issued");
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Cache-Control", "no-store")
                .header("Pragma", "no-cache")
                .body(Full::new(Bytes::from(json!({
                    "access_token": "ACCESS_TOKEN_123",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "refresh_token": "REFRESH_TOKEN_123",
                    "scope": params.get("scope").cloned().unwrap_or("".to_string())
                }).to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("OAuth2 token error: {}", e);
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "invalid_grant",
                    "error_description": format!("{}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle /introspect endpoint (RFC 7662)
async fn handle_introspect_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OAuth2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Parse form body
    let body_bytes = match req.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "active": false
                }).to_string())))
                .unwrap());
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes);
    let params = parse_query_params(&body_str);

    debug!("OAuth2 introspect request: token={:?}", params.get("token"));

    // Create LLM event
    let event = Event::new(&OAUTH2_INTROSPECT_EVENT, json!({
        "token": params.get("token").cloned().unwrap_or_default(),
        "token_type_hint": params.get("token_type_hint").cloned().unwrap_or_default(),
    }));

    // Call LLM
    match call_llm(&llm_client, &app_state, server_id, Some(connection_id), &event, &*protocol).await {
        Ok(_) => {
            // LLM should have returned oauth2_introspect_response action
            // For now, return a default active response
            info!("OAuth2 token introspected");
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "active": true,
                    "scope": "read write",
                    "client_id": "client123",
                    "token_type": "Bearer",
                    "exp": 1234567890,
                }).to_string())))
                .unwrap())
        }
        Err(_) => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "active": false
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle /revoke endpoint (RFC 7009)
async fn handle_revoke_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OAuth2Protocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Parse form body
    let body_bytes = match req.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .unwrap());
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes);
    let params = parse_query_params(&body_str);

    debug!("OAuth2 revoke request: token={:?}", params.get("token"));

    // Create LLM event
    let event = Event::new(&OAUTH2_REVOKE_EVENT, json!({
        "token": params.get("token").cloned().unwrap_or_default(),
        "token_type_hint": params.get("token_type_hint").cloned().unwrap_or_default(),
    }));

    // Call LLM
    let _ = call_llm(&llm_client, &app_state, server_id, Some(connection_id), &event, &*protocol).await;

    info!("OAuth2 token revoked");
    // RFC 7009: The authorization server responds with HTTP status code 200
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Full::new(Bytes::new()))
        .unwrap())
}

/// Parse URL-encoded query parameters
fn parse_query_params(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(
                urlencoding::decode(key).unwrap_or_default().to_string(),
                urlencoding::decode(value).unwrap_or_default().to_string(),
            );
        }
    }
    params
}
