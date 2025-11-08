//! Mercurial HTTP server implementation
//!
//! Implements Mercurial's HTTP wire protocol for serving virtual repositories.
//! The LLM controls repository content, capabilities, and bundle generation.
//!
//! Protocol URLs:
//! - GET  /?cmd=capabilities       - Server capabilities
//! - GET  /?cmd=heads              - Repository heads
//! - GET  /?cmd=branchmap          - Branch mappings
//! - GET  /?cmd=listkeys           - List keys (bookmarks, tags, etc.)
//! - POST /?cmd=getbundle          - Bundle retrieval (clone/pull)
//! - POST /?cmd=unbundle           - Bundle upload (push)
//!
//! Read-only implementation (no push support yet).

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
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::server::mercurial::actions::MercurialProtocol;
use crate::state::app_state::AppState;

/// Mercurial HTTP server
pub struct MercurialServer;

impl MercurialServer {
    /// Spawn the Mercurial server with integrated LLM actions
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
        info!("Mercurial server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] Mercurial server listening on {}", local_addr));

        let protocol = Arc::new(MercurialProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Mercurial connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx
                            .send(format!("[INFO] Mercurial connection from {}", remote_addr));

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

                            // Create a service that handles Mercurial HTTP requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_mercurial_request(
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
                                error!("Error serving Mercurial connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send(format!(
                                "[INFO] Mercurial connection {} closed",
                                connection_id
                            ));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Mercurial connection: {}", e);
                        let _ = status_tx
                            .send(format!("[ERROR] Failed to accept Mercurial connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a Mercurial HTTP request
async fn handle_mercurial_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<MercurialProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();
    let query = uri.query().unwrap_or("");

    debug!("Mercurial request: {} {}?{}", method, path, query);
    let _ = status_tx.send(format!("[DEBUG] Mercurial {} {}?{}", method, path, query));

    // Parse query parameters
    let params: HashMap<String, String> = parse_query_params(query);
    let cmd = params.get("cmd").map(|s| s.as_str()).unwrap_or("");

    // Parse repository name from path (e.g., /repo-name or /)
    let repo_name = parse_repo_name(path);

    // Track repository access
    if let Some(ref name) = repo_name {
        if let Err(e) = track_repo_access(&app_state, server_id, connection_id, name).await {
            error!("Failed to track repository access: {}", e);
        }
    }

    // Route based on command
    match cmd {
        "capabilities" => {
            handle_capabilities(
                repo_name,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                connection_id,
                server_id,
            )
            .await
        }
        "heads" => {
            handle_heads(
                repo_name,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                connection_id,
                server_id,
            )
            .await
        }
        "branchmap" => {
            handle_branchmap(
                repo_name,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                connection_id,
                server_id,
            )
            .await
        }
        "listkeys" => {
            let namespace = params.get("namespace").map(|s| s.as_str()).unwrap_or("bookmarks");
            handle_listkeys(
                repo_name,
                namespace,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                connection_id,
                server_id,
            )
            .await
        }
        "getbundle" if method == Method::POST => {
            // Read request body
            let body_bytes = match req.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    error!("Failed to read request body: {}", e);
                    let _ = status_tx.send(format!("[ERROR] Failed to read request body: {}", e));
                    return Ok(build_error_response(
                        StatusCode::BAD_REQUEST,
                        "Failed to read request body",
                    ));
                }
            };

            trace!(
                "Mercurial getbundle request body ({} bytes)",
                body_bytes.len()
            );
            let _ = status_tx.send(format!(
                "[TRACE] Mercurial getbundle request: {} bytes",
                body_bytes.len()
            ));

            handle_getbundle(
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
        "" => {
            // No command - could be a root request
            Ok(build_text_response(
                StatusCode::OK,
                "Mercurial HTTP Server - NetGet\nSpecify ?cmd=capabilities to see server capabilities",
            ))
        }
        _ => Ok(build_error_response(
            StatusCode::NOT_FOUND,
            &format!("Unknown command: {}", cmd),
        )),
    }
}

/// Parse repository name from URL path
fn parse_repo_name(path: &str) -> Option<String> {
    // Path formats:
    // /repo-name
    // / (root repository)
    let trimmed = path.trim_matches('/');

    if trimmed.is_empty() {
        Some("default".to_string())
    } else {
        Some(trimmed.to_string())
    }
}

/// Parse query parameters from query string
fn parse_query_params(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            match (parts.next(), parts.next()) {
                (Some(key), Some(value)) => {
                    Some((key.to_string(), urlencoding::decode(value).ok()?.to_string()))
                }
                _ => None,
            }
        })
        .collect()
}

/// Handle ?cmd=capabilities
async fn handle_capabilities(
    repo_name: Option<String>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<MercurialProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Mercurial capabilities for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Mercurial capabilities for repo: {}", repo));

    // Get sync actions for capabilities response
    let sync_actions = protocol.get_sync_actions();

    // Build prompt for LLM
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Mercurial client is requesting capabilities for repository "{}".

{}

You MUST respond with ONE of these actions:
1. "hg_capabilities" - Provide server capabilities
2. "hg_error" - If repository doesn't exist or access denied

Response format:
{{
  "actions": [
    {{
      "type": "hg_capabilities",
      "capabilities": ["batch", "branchmap", "getbundle", "httpheader=1024", "httppostargs", "known", "lookup", "pushkey", "unbundle=HG10GZ,HG10BZ,HG10UN"]
    }}
  ]
}}

Provide standard Mercurial capabilities for this repository."#,
        repo, actions_desc
    );

    debug!("Calling LLM for Mercurial capabilities: {}", repo);
    let _ = status_tx.send("[DEBUG] Calling LLM for capabilities".to_string());

    // Call LLM with retry
    let llm_response = match llm_client
        .generate_with_retry(
            &model,
            &prompt,
            r#"[{"type": "hg_capabilities", ...}]"#
        )
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("LLM call failed: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM call failed: {}", e));
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Mercurial capabilities: {}", llm_response);
    let _ = status_tx.send(format!("[TRACE] LLM response: {}", llm_response));

    // Parse LLM response as actions
    let actions_result: Value = match serde_json::from_str(&llm_response) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to parse LLM response: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to parse LLM response: {}", e));
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
            Ok(ActionResult::Custom { name, data }) if name == "hg_capabilities_response" => {
                // Build capabilities response (newline-separated)
                let capabilities = data
                    .get("capabilities")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_else(|| "batch\nbranchmap\ngetbundle\nknown\nlookup".to_string());

                Ok(build_text_response(StatusCode::OK, &capabilities))
            }
            Ok(ActionResult::Custom { name, data }) if name == "hg_error_response" => {
                let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("Error");
                let code = data
                    .get("code")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(500)
                    as u16;

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

/// Handle ?cmd=heads
async fn handle_heads(
    repo_name: Option<String>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<MercurialProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Mercurial heads for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Mercurial heads for repo: {}", repo));

    let sync_actions = protocol.get_sync_actions();
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Mercurial client is requesting repository heads for "{}".

{}

You MUST respond with ONE of these actions:
1. "hg_heads" - Provide list of head node IDs (40-char hex strings)
2. "hg_error" - If repository doesn't exist

Response format:
{{
  "actions": [
    {{
      "type": "hg_heads",
      "heads": ["a1b2c3d4e5f6789012345678901234567890abcd", "1234567890abcdef1234567890abcdef12345678"]
    }}
  ]
}}

Node IDs should be 40-character hex strings (can be fake for virtual repos).
Provide repository heads."#,
        repo, actions_desc
    );

    debug!("Calling LLM for Mercurial heads: {}", repo);
    let _ = status_tx.send("[DEBUG] Calling LLM for heads".to_string());

    let llm_response = match llm_client
        .generate_with_retry(&model, &prompt, r#"[{"type": "hg_heads", ...}]"#)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("LLM call failed: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM call failed: {}", e));
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Mercurial heads: {}", llm_response);
    let _ = status_tx.send(format!("[TRACE] LLM response: {}", llm_response));

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
            Ok(ActionResult::Custom { name, data }) if name == "hg_heads_response" => {
                let heads = data
                    .get("heads")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());

                Ok(build_text_response(StatusCode::OK, &heads))
            }
            Ok(ActionResult::Custom { name, data }) if name == "hg_error_response" => {
                let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("Error");
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

/// Handle ?cmd=branchmap
async fn handle_branchmap(
    repo_name: Option<String>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<MercurialProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Mercurial branchmap for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Mercurial branchmap for repo: {}", repo));

    let sync_actions = protocol.get_sync_actions();
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Mercurial client is requesting branch mappings for repository "{}".

{}

You MUST respond with ONE of these actions:
1. "hg_branchmap" - Provide branch name to node ID mappings
2. "hg_error" - If repository doesn't exist

Response format:
{{
  "actions": [
    {{
      "type": "hg_branchmap",
      "branches": {{
        "default": ["abc123..."],
        "stable": ["def456..."]
      }}
    }}
  ]
}}

Provide branch mappings for this repository."#,
        repo, actions_desc
    );

    debug!("Calling LLM for Mercurial branchmap: {}", repo);
    let _ = status_tx.send("[DEBUG] Calling LLM for branchmap".to_string());

    let llm_response = match llm_client
        .generate_with_retry(&model, &prompt, r#"[{"type": "hg_branchmap", ...}]"#)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("LLM call failed: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Mercurial branchmap: {}", llm_response);

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
            Ok(ActionResult::Custom { name, data }) if name == "hg_branchmap_response" => {
                let branches = data.get("branches").and_then(|v| v.as_object());
                let mut response_text = String::new();

                if let Some(branches_obj) = branches {
                    for (branch_name, nodes) in branches_obj {
                        if let Some(nodes_arr) = nodes.as_array() {
                            let nodes_str = nodes_arr
                                .iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            response_text.push_str(&format!("{} {}\n", branch_name, nodes_str));
                        }
                    }
                } else {
                    response_text = "default 0000000000000000000000000000000000000000\n".to_string();
                }

                Ok(build_text_response(StatusCode::OK, &response_text))
            }
            Ok(ActionResult::Custom { name, data }) if name == "hg_error_response" => {
                let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("Error");
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

/// Handle ?cmd=listkeys&namespace=...
async fn handle_listkeys(
    repo_name: Option<String>,
    namespace: &str,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<MercurialProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Mercurial listkeys for repository: {}, namespace: {}", repo, namespace);
    let _ = status_tx.send(format!(
        "[DEBUG] Mercurial listkeys for repo: {}, namespace: {}",
        repo, namespace
    ));

    let sync_actions = protocol.get_sync_actions();
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Mercurial client is requesting listkeys for repository "{}" in namespace "{}".

{}

You MUST respond with ONE of these actions:
1. "hg_listkeys" - Provide key-value mappings for the namespace
2. "hg_error" - If repository doesn't exist

Response format:
{{
  "actions": [
    {{
      "type": "hg_listkeys",
      "keys": {{
        "master": "abc123...",
        "develop": "def456..."
      }}
    }}
  ]
}}

Common namespaces:
- bookmarks: Repository bookmarks
- tags: Repository tags
- phases: Phase information
- namespaces: Available namespaces

Provide key-value mappings for this namespace."#,
        repo, namespace, actions_desc
    );

    debug!("Calling LLM for Mercurial listkeys: {}, {}", repo, namespace);
    let _ = status_tx.send("[DEBUG] Calling LLM for listkeys".to_string());

    let llm_response = match llm_client
        .generate_with_retry(&model, &prompt, r#"[{"type": "hg_listkeys", ...}]"#)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("LLM call failed: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Mercurial listkeys: {}", llm_response);

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
            Ok(ActionResult::Custom { name, data }) if name == "hg_listkeys_response" => {
                let keys = data.get("keys").and_then(|v| v.as_object());
                let mut response_text = String::new();

                if let Some(keys_obj) = keys {
                    for (key, value) in keys_obj {
                        if let Some(value_str) = value.as_str() {
                            response_text.push_str(&format!("{}\t{}\n", key, value_str));
                        }
                    }
                }

                Ok(build_text_response(StatusCode::OK, &response_text))
            }
            Ok(ActionResult::Custom { name, data }) if name == "hg_error_response" => {
                let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("Error");
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

/// Handle POST ?cmd=getbundle
async fn handle_getbundle(
    repo_name: Option<String>,
    _body: Bytes,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<MercurialProtocol>,
    _connection_id: ConnectionId,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let repo = repo_name.unwrap_or_else(|| "default".to_string());

    debug!("Mercurial getbundle for repository: {}", repo);
    let _ = status_tx.send(format!("[DEBUG] Mercurial getbundle for repo: {}", repo));

    let sync_actions = protocol.get_sync_actions();
    let model = app_state.get_ollama_model().await;

    let mut actions_desc = String::from("Available actions:\n");
    for action in &sync_actions {
        actions_desc.push_str(&format!("\n{}\n", action.to_prompt_text()));
    }

    let prompt = format!(
        r#"A Mercurial client is requesting a bundle (changegroup) for repository "{}".

{}

You MUST respond with ONE of these actions:
1. "hg_send_bundle" - Send bundle data (for clone/pull operations)
2. "hg_error" - If repository doesn't exist or error occurred

For MVP, you can send a minimal bundle or empty bundle. In production, this would contain
the actual Mercurial changesets requested by the client.

Response format:
{{
  "actions": [
    {{
      "type": "hg_send_bundle",
      "bundle_type": "HG10UN",
      "bundle_data": ""
    }}
  ]
}}

bundle_type can be: HG10UN (uncompressed), HG10GZ (gzip), HG10BZ (bzip2)
bundle_data should be empty string for an empty bundle, or actual bundle data.

Generate a bundle response."#,
        repo, actions_desc
    );

    debug!("Calling LLM for Mercurial getbundle: {}", repo);
    let _ = status_tx.send("[DEBUG] Calling LLM for getbundle".to_string());

    let llm_response = match llm_client
        .generate_with_retry(&model, &prompt, r#"[{"type": "hg_send_bundle", ...}]"#)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("LLM call failed: {}", e);
            return Ok(build_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Internal error: {}", e),
            ));
        }
    };

    trace!("LLM response for Mercurial getbundle");

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
            Ok(ActionResult::Custom { name, data }) if name == "hg_bundle_response" => {
                let bundle_data = data.get("bundle_data").and_then(|v| v.as_str()).unwrap_or("");

                // For now, return empty or minimal bundle
                let bundle_bytes = if bundle_data.is_empty() {
                    // Empty bundle
                    vec![]
                } else {
                    // Could decode base64 here if LLM provided bundle data
                    bundle_data.as_bytes().to_vec()
                };

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/mercurial-0.1")
                    .body(Full::new(Bytes::from(bundle_bytes)))
                    .unwrap())
            }
            Ok(ActionResult::Custom { name, data }) if name == "hg_error_response" => {
                let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("Error");
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
                    let mut recent_repos: Vec<String> = obj.get("recent_repos")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    if !recent_repos.contains(&repo_name.to_string()) {
                        recent_repos.push(repo_name.to_string());
                        // Keep only last 10 repos
                        if recent_repos.len() > 10 {
                            recent_repos.remove(0);
                        }
                    }
                    obj.insert("recent_repos".to_string(), serde_json::to_value(&recent_repos).unwrap_or(serde_json::json!([])));
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

/// Build a text response
fn build_text_response(status: StatusCode, text: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .body(Full::new(Bytes::from(text.to_string())))
        .unwrap()
}
