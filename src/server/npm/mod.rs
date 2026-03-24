//! NPM registry server implementation
//!
//! NPM registry runs over HTTP. The LLM controls package metadata, tarballs,
//! listings, and search results.

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::EventType;
use crate::server::connection::ConnectionId;
use crate::server::npm::actions::NpmProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_info};

/// NPM registry server that delegates to LLM
pub struct NpmServer;

impl NpmServer {
    /// Spawn the NPM registry server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "NPM registry server listening on {}", local_addr);

        let protocol = Arc::new(NpmProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("NPM connection {} from {}", connection_id, remote_addr);
                        let _ =
                            status_tx.send(format!("[INFO] NPM connection from {}", remote_addr));

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

                            // Create a service that handles NPM registry requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_npm_request(
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
                                error!("Error serving NPM connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone
                                .send(format!("[INFO] NPM connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept NPM connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single NPM registry request
async fn handle_npm_request(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<NpmProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();
    let query = uri.query().unwrap_or("");

    debug!("NPM request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] NPM {} {}", method, path));

    // Only handle GET requests
    if method != Method::GET {
        let response = json!({
            "error": "Method not allowed"
        });
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(response.to_string())))
            .unwrap());
    }

    // Route the request
    let (event_type, description) = if path == "/-/all" {
        ("NPM_LIST_REQUEST", "NPM package list request".to_string())
    } else if path.starts_with("/-/v1/search") {
        (
            "NPM_SEARCH_REQUEST",
            format!("NPM package search: {}", query),
        )
    } else if path.contains("/-/") {
        // Tarball request: /{package}/-/{tarball}.tgz
        let parts: Vec<&str> = path.split("/-/").collect();
        let package_name = parts.get(0).unwrap_or(&"").trim_start_matches('/');
        let tarball_name = parts.get(1).unwrap_or(&"");
        (
            "NPM_TARBALL_REQUEST",
            format!(
                "NPM tarball request: package={}, tarball={}",
                package_name, tarball_name
            ),
        )
    } else {
        // Package metadata request: /{package}
        let package_name = path.trim_start_matches('/');
        (
            "NPM_PACKAGE_REQUEST",
            format!("NPM package metadata request: {}", package_name),
        )
    };

    trace!("NPM event: {}: {}", event_type, &description);

    // Verify server exists
    if app_state.get_instruction(server_id).await.is_none() {
        error!("Server {} not found", server_id);
        let response = json!({
            "error": "Server not found"
        });
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(response.to_string())))
            .unwrap());
    }

    // Build NPM event - use the static event type references
    let event_type_static: &'static EventType = match &event_type[..] {
        "NPM_PACKAGE_REQUEST" => &actions::NPM_PACKAGE_REQUEST,
        "NPM_TARBALL_REQUEST" => &actions::NPM_TARBALL_REQUEST,
        "NPM_LIST_REQUEST" => &actions::NPM_LIST_REQUEST,
        "NPM_SEARCH_REQUEST" => &actions::NPM_SEARCH_REQUEST,
        _ => {
            error!("Unknown NPM event type: {}", event_type);
            let error_response = json!({
                "error": format!("Internal error: unknown event type '{}'", event_type)
            });
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(error_response.to_string())))
                .unwrap());
        }
    };

    let event = crate::protocol::Event::new(
        event_type_static,
        json!({
            "method": method.as_str(),
            "path": path,
            "query": query,
            "description": description,
        }),
    );

    console_debug!(
        status_tx,
        "Calling LLM for NPM request: {} {}",
        method,
        path
    );

    // Call LLM
    let llm_result = call_llm(
        &llm_client,
        &app_state,
        server_id,
        None,
        &event,
        protocol.as_ref(),
    )
    .await;

    // Process LLM result
    match llm_result {
        Ok(execution_result) => {
            // Look for NPM-specific response actions
            for result in execution_result.protocol_results {
                // Try to process this action result as NPM response
                let response = process_npm_action_result(result, &status_tx).await;
                // Return the first successful response
                return response;
            }

            // No NPM actions found, return error
            let error_response = json!({
                "error": "No NPM action returned"
            });
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(error_response.to_string())))
                .unwrap())
        }
        Err(e) => {
            console_error!(status_tx, "LLM call failed: {}", e);
            let error_response = json!({
                "error": format!("LLM error: {}", e)
            });
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(error_response.to_string())))
                .unwrap())
        }
    }
}

/// Process LLM action result and build HTTP response
async fn process_npm_action_result(
    action_result: crate::llm::ActionResult,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    use crate::llm::ActionResult;

    match action_result {
        ActionResult::Custom { name, data } => {
            match name.as_str() {
                "npm_package_metadata" => {
                    let metadata = data
                        .get("metadata")
                        .context("Missing metadata in npm_package_metadata")
                        .unwrap();

                    debug!("NPM package metadata response");
                    let _ = status_tx.send("[DEBUG] Sending NPM package metadata".to_string());
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(metadata.to_string())))
                        .unwrap())
                }
                "npm_package_tarball" => {
                    let tarball_data = data
                        .get("tarball_data")
                        .and_then(|v| v.as_str())
                        .context("Missing tarball_data in npm_package_tarball")
                        .unwrap();

                    // Decode base64
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(tarball_data)
                        .unwrap_or_default();

                    debug!("NPM package tarball response: {} bytes", decoded.len());
                    let _ = status_tx.send(format!(
                        "[DEBUG] Sending NPM tarball: {} bytes",
                        decoded.len()
                    ));
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/octet-stream")
                        .body(Full::new(Bytes::from(decoded)))
                        .unwrap())
                }
                "npm_package_list" => {
                    let packages = data
                        .get("packages")
                        .context("Missing packages in npm_package_list")
                        .unwrap();

                    debug!("NPM package list response");
                    let _ = status_tx.send("[DEBUG] Sending NPM package list".to_string());
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(packages.to_string())))
                        .unwrap())
                }
                "npm_package_search" => {
                    let results = data
                        .get("results")
                        .context("Missing results in npm_package_search")
                        .unwrap();

                    debug!("NPM package search response");
                    let _ = status_tx.send("[DEBUG] Sending NPM search results".to_string());
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(results.to_string())))
                        .unwrap())
                }
                "npm_error" => {
                    let error_message = data
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    let status_code = data
                        .get("status_code")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(500) as u16;

                    debug!("NPM error: {} ({})", error_message, status_code);
                    let _ = status_tx.send(format!("[DEBUG] NPM error: {}", error_message));
                    let error_response = json!({
                        "error": error_message
                    });
                    Ok(Response::builder()
                        .status(
                            StatusCode::from_u16(status_code)
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        )
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(error_response.to_string())))
                        .unwrap())
                }
                _ => {
                    error!("Unknown NPM action: {}", name);
                    let error_response = json!({
                        "error": "Unknown NPM action"
                    });
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(error_response.to_string())))
                        .unwrap())
                }
            }
        }
        _ => {
            error!("Unexpected action result type for NPM request");
            let error_response = json!({
                "error": "Internal server error"
            });
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(error_response.to_string())))
                .unwrap())
        }
    }
}
