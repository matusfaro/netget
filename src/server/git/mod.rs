//! Git Smart HTTP server implementation
//!
//! Implements Git's Smart HTTP protocol for serving virtual repositories.
//! The LLM controls repository content, reference advertisement, and pack generation.
//!
//! Protocol URLs:
//! - GET  /info/refs?service=git-upload-pack  - Reference discovery
//! - POST /git-upload-pack                     - Pack negotiation and transfer
//!
//! Read-only implementation (no push support yet).

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::server::git::actions::GitProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_info, console_trace, console_warn};

/// Git Smart HTTP server
pub struct GitServer;

impl GitServer {
    /// Spawn the Git server with integrated LLM actions
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
        console_info!(status_tx, "Git server listening on {}", local_addr);

        let protocol = Arc::new(GitProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Git connection {} from {}", connection_id, remote_addr);
                        let _ =
                            status_tx.send(format!("[INFO] Git connection from {}", remote_addr));

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

                            // Create a service that handles Git Smart HTTP requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_git_request(
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
                                error!("Error serving Git connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone
                                .send(format!("[INFO] Git connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Git connection: {}", e);
                        let _ = status_tx
                            .send(format!("[ERROR] Failed to accept Git connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a Git Smart HTTP request
async fn handle_git_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<GitProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();

    debug!("Git request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] Git {} {}", method, path));

    // Parse repository name from path
    // Format: /<repo>/info/refs or /<repo>/git-upload-pack
    let repo_name = parse_repo_name(path);

    // Track repository access
    if let Some(ref name) = repo_name {
        if let Err(e) = track_repo_access(&app_state, server_id, connection_id, name).await {
            error!("Failed to track repository access: {}", e);
        }
    }

    // Route based on path
    match (&method, path) {
        // Reference discovery: GET /info/refs?service=git-upload-pack
        (&Method::GET, p) if p.ends_with("/info/refs") => {
            let query = uri.query().unwrap_or("");
            if query.contains("service=git-upload-pack") {
                handle_info_refs(
                    repo_name,
                    &llm_client,
                    &app_state,
                    &status_tx,
                    &protocol,
                    connection_id,
                    server_id,
                )
                .await
            } else {
                // Dumb HTTP protocol not supported
                Ok(build_error_response(
                    StatusCode::FORBIDDEN,
                    "Dumb HTTP protocol not supported, use Smart HTTP (git-upload-pack service)",
                ))
            }
        }

        // Pack negotiation: POST /git-upload-pack
        (&Method::POST, p) if p.ends_with("/git-upload-pack") => {
            // Read request body (pack negotiation)
            let body_bytes = match req.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    console_error!(status_tx, "Failed to read request body: {}", e);
                    return Ok(build_error_response(
                        StatusCode::BAD_REQUEST,
                        "Failed to read request body",
                    ));
                }
            };

            trace!(
                "Git upload-pack request body ({} bytes): {:?}",
                body_bytes.len(),
                body_bytes
            );
            let _ = status_tx.send(format!(
                "[TRACE] Git upload-pack request: {} bytes",
                body_bytes.len()
            ));

            handle_upload_pack(
                repo_name,
                body_bytes,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                connection_id,
                server_id,
            )
            .await
        }

        // Unsupported endpoint
        _ => Ok(build_error_response(
            StatusCode::NOT_FOUND,
            &format!("Endpoint not found: {} {}", method, path),
        )),
    }
}

/// Parse repository name from URL path
fn parse_repo_name(path: &str) -> Option<String> {
    // Path formats:
    // /repo-name/info/refs
    // /repo-name/git-upload-pack
    // /info/refs (root repository)
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();

    if parts.is_empty() {
        return None;
    }

    // If path is just /info/refs or /git-upload-pack, no repo name
    if parts[0] == "info" || parts[0] == "git-upload-pack" {
        return Some("default".to_string()); // Default repository
    }

    // Otherwise, first part is repo name
    Some(parts[0].to_string())
}

/// Handle GET /info/refs?service=git-upload-pack
async fn handle_info_refs(
    repo_name: Option<String>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<GitProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Git info/refs for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Git info/refs for repo: {}", repo));

    // Get sync actions for reference advertisement
    let sync_actions = protocol.get_sync_actions();

    // Build prompt for LLM
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Git client is requesting references (branches and tags) for repository "{}".

{}

You MUST respond with ONE of these actions:
1. "git_advertise_refs" - Provide list of branches/tags with commit SHAs
2. "git_error" - If repository doesn't exist or access denied

Response format:
{{
  "actions": [
    {{
      "type": "git_advertise_refs",
      "refs": [
        {{"name": "refs/heads/main", "sha": "abc123..."}},
        {{"name": "refs/tags/v1.0", "sha": "def456..."}}
      ],
      "capabilities": ["multi_ack", "side-band-64k", "ofs-delta"]
    }}
  ]
}}

The SHA values should be 40-character hex strings (can be fake for virtual repos).
Provide references for this repository."#,
        repo, actions_desc
    );

    debug!("Calling LLM for Git ref advertisement: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Calling LLM for ref advertisement"));

    // Call LLM with retry
    let model_str = match crate::llm::ensure_model_selected(model).await {
        Ok(m) => m,
        Err(e) => {
            console_error!(status_tx, "Failed to select model: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Model selection failed: {}", e),
            ));
        }
    };
    let llm_response = match llm_client
        .generate_with_retry(
            &model_str,
            &prompt,
            r#"[{"type": "git_advertise_refs", ...}]"#,
        )
        .await
    {
        Ok(response) => response,
        Err(e) => {
            console_error!(status_tx, "LLM call failed: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Git refs: {}", llm_response);
    let _ = status_tx.send(format!("[TRACE] LLM response: {}", llm_response));

    // Parse LLM response as actions
    let actions_result: Value = match serde_json::from_str(&llm_response) {
        Ok(v) => v,
        Err(e) => {
            console_error!(status_tx, "Failed to parse LLM response: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid LLM response",
            ));
        }
    };

    let actions = actions_result
        .get("actions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            error!("LLM response missing 'actions' array");
            build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid LLM response format",
            )
        });

    let actions = match actions {
        Ok(a) => a,
        Err(response) => return Ok(response),
    };

    // Execute the first action
    if let Some(action) = actions.first() {
        match protocol.execute_action(action.clone()) {
            Ok(ActionResult::Custom { name, data }) if name == "git_refs_response" => {
                // Build pkt-line format response
                let response_body = build_refs_response(&data);

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header(
                        "Content-Type",
                        "application/x-git-upload-pack-advertisement",
                    )
                    .header("Cache-Control", "no-cache")
                    .body(Full::new(Bytes::from(response_body)))
                    .unwrap())
            }
            Ok(ActionResult::Custom { name, data }) if name == "git_error_response" => {
                let message = data
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Error");
                let code = data.get("code").and_then(|v| v.as_u64()).unwrap_or(500) as u16;

                Ok(build_error_response(
                    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    message,
                ))
            }
            Ok(_) => {
                error!("LLM returned unexpected action type");
                Ok(build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Unexpected action type",
                ))
            }
            Err(e) => {
                error!("Failed to execute action: {}", e);
                Ok(build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Action execution failed: {}", e),
                ))
            }
        }
    } else {
        error!("No actions in LLM response");
        Ok(build_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "No actions in response",
        ))
    }
}

/// Handle POST /git-upload-pack
async fn handle_upload_pack(
    repo_name: Option<String>,
    body: Bytes,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<GitProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Git upload-pack for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Git upload-pack for repo: {}", repo));

    // For MVP, we'll just send a simple pack response
    // In a full implementation, we'd parse the want/have negotiation from body

    let sync_actions = protocol.get_sync_actions();
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Git client is requesting a pack file for repository "{}".

Pack negotiation data ({} bytes received).

{}

You MUST respond with ONE of these actions:
1. "git_send_pack" - Send pack file data (base64-encoded)
2. "git_error" - If repository doesn't exist or error occurred

For MVP, you can send a minimal pack file. In production, this would contain
the actual Git objects requested by the client.

Response format:
{{
  "actions": [
    {{
      "type": "git_send_pack",
      "pack_data": "<base64 encoded pack file>"
    }}
  ]
}}

Generate a pack file response."#,
        repo,
        body.len(),
        actions_desc
    );

    debug!("Calling LLM for Git pack generation: {}", repo);
    let _ = status_tx.send("[DEBUG] Calling LLM for pack generation".to_string());

    // Call LLM with retry
    let model_str = match crate::llm::ensure_model_selected(model).await {
        Ok(m) => m,
        Err(e) => {
            console_error!(status_tx, "Failed to select model: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Model selection failed: {}", e),
            ));
        }
    };
    let llm_response = match llm_client
        .generate_with_retry(&model_str, &prompt, r#"[{"type": "git_send_pack", ...}]"#)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            console_error!(status_tx, "LLM call failed: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Git pack: {}", llm_response);
    let _ = status_tx.send(format!("[TRACE] LLM response for pack generation"));

    // Parse and execute actions (similar to handle_info_refs)
    let actions_result: Value = match serde_json::from_str(&llm_response) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to parse LLM response: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid LLM response",
            ));
        }
    };

    let actions = match actions_result.get("actions").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid LLM response format",
            ))
        }
    };

    if let Some(action) = actions.first() {
        match protocol.execute_action(action.clone()) {
            Ok(ActionResult::Custom { name, data }) if name == "git_pack_response" => {
                let pack_data = data.get("pack_data").and_then(|v| v.as_str()).unwrap_or("");

                // Decode base64 pack data
                let pack_bytes = match base64::engine::general_purpose::STANDARD.decode(pack_data) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Failed to decode pack data: {}", e);
                        return Ok(build_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Invalid pack data",
                        ));
                    }
                };

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/x-git-upload-pack-result")
                    .header("Cache-Control", "no-cache")
                    .body(Full::new(Bytes::from(pack_bytes)))
                    .unwrap())
            }
            Ok(ActionResult::Custom { name, data }) if name == "git_error_response" => {
                let message = data
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Error");
                let code = data.get("code").and_then(|v| v.as_u64()).unwrap_or(500) as u16;

                Ok(build_error_response(
                    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    message,
                ))
            }
            Ok(_) => Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unexpected action type",
            )),
            Err(e) => Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Action execution failed: {}", e),
            )),
        }
    } else {
        Ok(build_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "No actions in response",
        ))
    }
}

/// Build Git pkt-line format refs response
fn build_refs_response(data: &Value) -> Vec<u8> {
    let mut response = Vec::new();

    // Service announcement
    let service_line = "# service=git-upload-pack\n";
    response
        .extend_from_slice(format!("{:04x}{}", service_line.len() + 4, service_line).as_bytes());
    response.extend_from_slice(b"0000"); // Flush packet

    // Get refs from data
    let refs = data.get("refs").and_then(|v| v.as_array());
    let capabilities = data
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "multi_ack side-band-64k ofs-delta".to_string());

    if let Some(refs_array) = refs {
        for (idx, ref_obj) in refs_array.iter().enumerate() {
            let name = ref_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("refs/heads/main");
            let default_sha = "0".repeat(40);
            let sha = ref_obj
                .get("sha")
                .and_then(|v| v.as_str())
                .unwrap_or(&default_sha);

            let ref_line = if idx == 0 {
                // First ref includes capabilities
                format!("{} {}\0{}\n", sha, name, capabilities)
            } else {
                format!("{} {}\n", sha, name)
            };

            let pkt_line = format!("{:04x}{}", ref_line.len() + 4, ref_line);
            response.extend_from_slice(pkt_line.as_bytes());
        }
    }

    // Flush packet
    response.extend_from_slice(b"0000");

    response
}

/// Track repository access in connection state
async fn track_repo_access(
    app_state: &Arc<AppState>,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    repo_name: &str,
) -> anyhow::Result<()> {
    app_state
        .with_server_mut(server_id, |server| {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    let mut recent_repos: Vec<String> = obj
                        .get("recent_repos")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    if !recent_repos.contains(&repo_name.to_string()) {
                        recent_repos.push(repo_name.to_string());
                        // Keep only last 10 repos
                        if recent_repos.len() > 10 {
                            recent_repos.remove(0);
                        }
                    }
                    obj.insert(
                        "recent_repos".to_string(),
                        serde_json::to_value(&recent_repos).unwrap_or(serde_json::json!([])),
                    );
                }
            }
        })
        .await;

    Ok(())
}

/// Build an HTTP error response
fn build_error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .body(Full::new(Bytes::from(format!("Error: {}\n", message))))
        .unwrap()
}
