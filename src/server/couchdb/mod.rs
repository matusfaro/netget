//! CouchDB server implementation
//!
//! Implements a CouchDB-compatible HTTP/JSON REST API on port 5984.
//! The LLM controls all database operations, document management, views, changes feed,
//! and maintains "virtual" data through conversation context.

pub mod actions;

use std::collections::HashMap;
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

use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::server::connection::ConnectionId;
use crate::server::CouchDbProtocol;
use crate::state::app_state::AppState;
use crate::{console_error, console_info};

/// CouchDB server that delegates all operations to LLM
pub struct CouchDbServer;

impl CouchDbServer {
    /// Spawn the CouchDB server with integrated LLM actions
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        enable_auth: bool,
        admin_username: String,
        admin_password: String,
    ) -> anyhow::Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(
            status_tx,
            "CouchDB server listening on {} (auth: {})",
            local_addr,
            if enable_auth { "enabled" } else { "disabled" }
        );

        let protocol = Arc::new(CouchDbProtocol::new());
        let auth_config = Arc::new(AuthConfig {
            enabled: enable_auth,
            admin_username,
            admin_password,
        });

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!(
                            "CouchDB connection {} from {}",
                            connection_id, remote_addr
                        );
                        let _ = status_tx.send(format!(
                            "[INFO] CouchDB connection from {}",
                            remote_addr
                        ));

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
                        let auth_config_clone = auth_config.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();
                            let app_state_for_service = app_state_clone.clone();

                            // Create a service that handles CouchDB requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                let auth_clone = auth_config_clone.clone();
                                handle_couchdb_request_with_llm(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    server_id,
                                    auth_clone,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                error!("Error serving CouchDB connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send(format!(
                                "[INFO] CouchDB connection {} closed",
                                connection_id
                            ));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(
                            status_tx,
                            "Failed to accept CouchDB connection: {}",
                            e
                        );
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Authentication configuration
struct AuthConfig {
    enabled: bool,
    admin_username: String,
    admin_password: String,
}

/// Handle a single CouchDB request with LLM
#[allow(clippy::too_many_arguments)]
async fn handle_couchdb_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<CouchDbProtocol>,
    server_id: crate::state::ServerId,
    auth_config: Arc<AuthConfig>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(|q| q.to_string());

    // Extract authorization header
    let authorization = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Check authentication if enabled
    if auth_config.enabled {
        if let Some(auth_header) = &authorization {
            if !check_basic_auth(
                auth_header,
                &auth_config.admin_username,
                &auth_config.admin_password,
            ) {
                debug!("CouchDB authentication failed");
                let _ = status_tx.send("[DEBUG] CouchDB authentication failed".to_string());
                return Ok(create_auth_required_response());
            }
        } else {
            // No auth header provided
            debug!("CouchDB authentication required");
            let _ = status_tx.send("[DEBUG] CouchDB authentication required".to_string());
            return Ok(create_auth_required_response());
        }
    }

    // Read JSON body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            console_error!(
                status_tx,
                "Failed to read CouchDB request body: {}",
                e
            );
            Bytes::new()
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    debug!(
        "CouchDB request: {} {} ({} bytes)",
        method,
        uri,
        body_bytes.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] CouchDB {} {} ({} bytes)",
        method,
        path,
        body_bytes.len()
    ));

    // Parse query parameters
    let query_params: HashMap<String, String> = query
        .as_ref()
        .map(|q| {
            form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default();

    // Detect operation type from path and method
    let (operation, database, doc_id) = detect_couchdb_operation(&method, &path);

    if !body_str.is_empty() {
        trace!("CouchDB request body: {}", body_str);
        let _ = status_tx.send(format!("[TRACE] CouchDB request: {}", body_str));
    }

    // Create CouchDB request event
    let event = crate::protocol::Event::new(
        &actions::COUCHDB_REQUEST_EVENT,
        serde_json::json!({
            "method": method,
            "path": path,
            "operation": operation,
            "database": database,
            "doc_id": doc_id,
            "query_params": query_params,
            "request_body": body_str,
            "authorization": authorization.map(|_| "***"),  // Don't log credentials
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
            // Look for CouchDB-specific response actions
            for result in execution_result.protocol_results {
                match result {
                    ActionResult::Custom { name, data } => {
                        if name == "couchdb_response" {
                            let status =
                                data.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
                            let body = data.get("body").and_then(|v| v.as_str()).unwrap_or("{}");
                            let etag = data.get("etag").and_then(|v| v.as_str());
                            let www_authenticate = data.get("www_authenticate").and_then(|v| v.as_str());

                            debug!("CouchDB response: status={}", status);
                            let _ = status_tx
                                .send(format!("[DEBUG] CouchDB → {} response", status));
                            trace!("CouchDB response body: {}", body);
                            let _ =
                                status_tx.send(format!("[TRACE] CouchDB response: {}", body));

                            let mut response_builder = Response::builder()
                                .status(status)
                                .header("Content-Type", "application/json")
                                .header("Server", "CouchDB/3.5.1 (NetGet LLM)");

                            if let Some(etag_value) = etag {
                                response_builder = response_builder.header("ETag", etag_value);
                            }

                            if let Some(www_auth) = www_authenticate {
                                response_builder = response_builder.header("WWW-Authenticate", www_auth);
                            }

                            return Ok(response_builder
                                .body(Full::new(Bytes::from(body.to_string())))
                                .unwrap());
                        }
                    }
                    _ => {
                        // Other actions don't affect HTTP response
                    }
                }
            }

            // No CouchDB response found, return default OK
            debug!("No CouchDB response from LLM, returning 200 OK with default response");
            let default_response = serde_json::json!({
                "ok": true
            })
            .to_string();

            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .header("Server", "CouchDB/3.5.1 (NetGet LLM)")
                .body(Full::new(Bytes::from(default_response)))
                .unwrap())
        }
        Err(e) => {
            console_error!(status_tx, "LLM error for CouchDB request: {}", e);

            let error_response = serde_json::json!({
                "error": "internal_server_error",
                "reason": format!("LLM processing error: {}", e)
            })
            .to_string();

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/json")
                .header("Server", "CouchDB/3.5.1 (NetGet LLM)")
                .body(Full::new(Bytes::from(error_response)))
                .unwrap())
        }
    }
}

/// Detect CouchDB operation from HTTP method and path
fn detect_couchdb_operation(
    method: &str,
    path: &str,
) -> (String, Option<String>, Option<String>) {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    match (method, parts.as_slice()) {
        // Root endpoint - server info
        ("GET", [""]) => ("server_info".to_string(), None, None),

        // Special endpoints
        ("GET", ["_all_dbs"]) => ("all_dbs".to_string(), None, None),
        ("GET", ["_active_tasks"]) => ("active_tasks".to_string(), None, None),
        ("GET", ["_uuids"]) => ("uuids".to_string(), None, None),
        ("POST", ["_replicate"]) => ("replicate".to_string(), None, None),
        ("GET", ["_session"]) => ("session".to_string(), None, None),

        // Database operations
        ("PUT", [db]) if !db.starts_with('_') => {
            ("db_create".to_string(), Some(db.to_string()), None)
        }
        ("DELETE", [db]) if !db.starts_with('_') => {
            ("db_delete".to_string(), Some(db.to_string()), None)
        }
        ("GET", [db]) if !db.starts_with('_') => {
            ("db_info".to_string(), Some(db.to_string()), None)
        }
        ("POST", [db]) if !db.starts_with('_') => {
            ("doc_create".to_string(), Some(db.to_string()), None)
        }

        // Database special endpoints
        ("GET", [db, "_all_docs"]) => ("all_docs".to_string(), Some(db.to_string()), None),
        ("POST", [db, "_all_docs"]) => ("all_docs".to_string(), Some(db.to_string()), None),
        ("POST", [db, "_bulk_docs"]) => ("bulk_docs".to_string(), Some(db.to_string()), None),
        ("GET", [db, "_changes"]) => ("changes".to_string(), Some(db.to_string()), None),
        ("POST", [db, "_ensure_full_commit"]) => {
            ("ensure_full_commit".to_string(), Some(db.to_string()), None)
        }
        ("POST", [db, "_compact"]) => ("compact".to_string(), Some(db.to_string()), None),
        ("POST", [db, "_purge"]) => ("purge".to_string(), Some(db.to_string()), None),

        // Design document operations (views)
        ("GET", [db, "_design", ddoc]) => (
            "design_get".to_string(),
            Some(db.to_string()),
            Some(format!("_design/{}", ddoc)),
        ),
        ("PUT", [db, "_design", ddoc]) => (
            "design_put".to_string(),
            Some(db.to_string()),
            Some(format!("_design/{}", ddoc)),
        ),
        ("DELETE", [db, "_design", ddoc]) => (
            "design_delete".to_string(),
            Some(db.to_string()),
            Some(format!("_design/{}", ddoc)),
        ),

        // View query
        ("GET", [db, "_design", ddoc, "_view", view]) => (
            "view_query".to_string(),
            Some(db.to_string()),
            Some(format!("_design/{}/{}", ddoc, view)),
        ),
        ("POST", [db, "_design", ddoc, "_view", view]) => (
            "view_query".to_string(),
            Some(db.to_string()),
            Some(format!("_design/{}/{}", ddoc, view)),
        ),

        // Document operations
        ("GET", [db, doc_id]) if !doc_id.starts_with('_') => (
            "doc_get".to_string(),
            Some(db.to_string()),
            Some(doc_id.to_string()),
        ),
        ("PUT", [db, doc_id]) if !doc_id.starts_with('_') => (
            "doc_put".to_string(),
            Some(db.to_string()),
            Some(doc_id.to_string()),
        ),
        ("DELETE", [db, doc_id]) if !doc_id.starts_with('_') => (
            "doc_delete".to_string(),
            Some(db.to_string()),
            Some(doc_id.to_string()),
        ),
        ("HEAD", [db, doc_id]) if !doc_id.starts_with('_') => (
            "doc_head".to_string(),
            Some(db.to_string()),
            Some(doc_id.to_string()),
        ),

        // Attachment operations
        ("GET", [db, doc_id, attachment]) if !doc_id.starts_with('_') => (
            "attachment_get".to_string(),
            Some(db.to_string()),
            Some(format!("{}/{}", doc_id, attachment)),
        ),
        ("PUT", [db, doc_id, attachment]) if !doc_id.starts_with('_') => (
            "attachment_put".to_string(),
            Some(db.to_string()),
            Some(format!("{}/{}", doc_id, attachment)),
        ),
        ("DELETE", [db, doc_id, attachment]) if !doc_id.starts_with('_') => (
            "attachment_delete".to_string(),
            Some(db.to_string()),
            Some(format!("{}/{}", doc_id, attachment)),
        ),

        // Replication endpoints
        ("GET", [db, "_local", doc_id]) => (
            "local_doc_get".to_string(),
            Some(db.to_string()),
            Some(format!("_local/{}", doc_id)),
        ),
        ("PUT", [db, "_local", doc_id]) => (
            "local_doc_put".to_string(),
            Some(db.to_string()),
            Some(format!("_local/{}", doc_id)),
        ),
        ("POST", [db, "_revs_diff"]) => ("revs_diff".to_string(), Some(db.to_string()), None),
        ("POST", [db, "_bulk_get"]) => ("bulk_get".to_string(), Some(db.to_string()), None),

        // Default
        _ => ("unknown".to_string(), None, None),
    }
}

/// Check HTTP Basic Authentication
fn check_basic_auth(auth_header: &str, expected_username: &str, expected_password: &str) -> bool {
    // Format: "Basic base64(username:password)"
    if !auth_header.starts_with("Basic ") {
        return false;
    }

    let encoded = &auth_header[6..]; // Skip "Basic "

    // Decode base64
    use base64::Engine;
    let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
        Ok(d) => d,
        Err(_) => return false,
    };

    let credentials = match String::from_utf8(decoded) {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Split username:password
    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
    if parts.len() != 2 {
        return false;
    }

    parts[0] == expected_username && parts[1] == expected_password
}

/// Create 401 Unauthorized response
fn create_auth_required_response() -> Response<Full<Bytes>> {
    let error_response = serde_json::json!({
        "error": "unauthorized",
        "reason": "Authentication required"
    })
    .to_string();

    Response::builder()
        .status(401)
        .header("Content-Type", "application/json")
        .header("Server", "CouchDB/3.5.1 (NetGet LLM)")
        .header("WWW-Authenticate", "Basic realm=\"CouchDB\"")
        .body(Full::new(Bytes::from(error_response)))
        .unwrap()
}
