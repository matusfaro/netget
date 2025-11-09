//! Elasticsearch/OpenSearch server implementation
//!
//! Implements an Elasticsearch-compatible HTTP/JSON API on port 9200.
//! The LLM controls search queries, indexing, and maintains "virtual" data through conversation context.

pub mod actions;

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

use crate::server::connection::ConnectionId;
use crate::server::ElasticsearchProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::state::app_state::AppState;

/// Elasticsearch server that delegates search/index operations to LLM
pub struct ElasticsearchServer;

impl ElasticsearchServer {
    /// Spawn the Elasticsearch server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Elasticsearch server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] Elasticsearch server listening on {}", local_addr));

        let protocol = Arc::new(ElasticsearchProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Elasticsearch connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] Elasticsearch connection from {}", remote_addr));

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
                            protocol_info: ProtocolConnectionInfo::empty(),
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

                            // Create a service that handles Elasticsearch requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_elasticsearch_request_with_llm(
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
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving Elasticsearch connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] Elasticsearch connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Elasticsearch connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept Elasticsearch connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single Elasticsearch request with LLM
async fn handle_elasticsearch_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<ElasticsearchProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let path = req.uri().path().to_string();

    // Read JSON body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read Elasticsearch request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read Elasticsearch request body: {}", e));
            Bytes::new()
        }
    };

    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    debug!(
        "Elasticsearch request: {} {} ({} bytes)",
        method,
        uri,
        body_bytes.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] Elasticsearch {} {} ({} bytes)",
        method, path, body_bytes.len()
    ));

    // Detect operation type from path and method
    let (operation, index, doc_id) = detect_elasticsearch_operation(&method, &path);

    trace!("Elasticsearch request body: {}", body_str);
    let _ = status_tx.send(format!("[TRACE] Elasticsearch request: {}", body_str));

    // Create Elasticsearch request event
    let event = crate::protocol::Event::new(
        &actions::ELASTICSEARCH_REQUEST_EVENT,
        serde_json::json!({
            "method": method,
            "path": path,
            "operation": operation,
            "index": index,
            "doc_id": doc_id,
            "request_body": body_str,
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
            // Look for Elasticsearch-specific response actions
            for result in execution_result.protocol_results {
                match result {
                    ActionResult::Custom { name, data } => {
                        if name == "elasticsearch_response" {
                            let status = data.get("status")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(200) as u16;
                            let body = data.get("body")
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");

                            debug!("Elasticsearch response: status={}", status);
                            let _ = status_tx.send(format!("[DEBUG] Elasticsearch → {} response", status));
                            trace!("Elasticsearch response body: {}", body);
                            let _ = status_tx.send(format!("[TRACE] Elasticsearch response: {}", body));

                            return Ok(Response::builder()
                                .status(status)
                                .header("Content-Type", "application/json; charset=UTF-8")
                                .header("X-elastic-product", "Elasticsearch")
                                .body(Full::new(Bytes::from(body.to_string())))
                                .unwrap());
                        }
                    }
                    _ => {
                        // Other actions don't affect HTTP response
                    }
                }
            }

            // No Elasticsearch response found, return default OK with minimal cluster info
            debug!("No Elasticsearch response from LLM, returning 200 OK with default response");
            let default_response = serde_json::json!({
                "acknowledged": true
            }).to_string();

            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "application/json; charset=UTF-8")
                .header("X-elastic-product", "Elasticsearch")
                .body(Full::new(Bytes::from(default_response)))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error for Elasticsearch request: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM error for Elasticsearch request: {}", e));

            let error_response = serde_json::json!({
                "error": {
                    "root_cause": [{
                        "type": "server_error",
                        "reason": format!("LLM processing error: {}", e)
                    }],
                    "type": "server_error",
                    "reason": format!("LLM processing error: {}", e)
                },
                "status": 500
            }).to_string();

            Ok(Response::builder()
                .status(500)
                .header("Content-Type", "application/json; charset=UTF-8")
                .header("X-elastic-product", "Elasticsearch")
                .body(Full::new(Bytes::from(error_response)))
                .unwrap())
        }
    }
}

/// Detect Elasticsearch operation from HTTP method and path
fn detect_elasticsearch_operation(method: &str, path: &str) -> (String, Option<String>, Option<String>) {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    match (method, parts.as_slice()) {
        // Root endpoint
        ("GET", [""]) => ("cluster_info".to_string(), None, None),

        // Search operations
        ("GET" | "POST", ["_search"]) => ("search".to_string(), None, None),
        ("GET" | "POST", [index, "_search"]) => ("search".to_string(), Some(index.to_string()), None),

        // Document operations
        ("POST", [index, "_doc"]) | ("PUT", [index, "_doc"]) =>
            ("index".to_string(), Some(index.to_string()), None),
        ("POST" | "PUT", [index, "_doc", id]) | ("PUT", [index, "_create", id]) =>
            ("index".to_string(), Some(index.to_string()), Some(id.to_string())),
        ("GET", [index, "_doc", id]) =>
            ("get".to_string(), Some(index.to_string()), Some(id.to_string())),
        ("DELETE", [index, "_doc", id]) =>
            ("delete".to_string(), Some(index.to_string()), Some(id.to_string())),

        // Bulk operations
        ("POST" | "PUT", ["_bulk"]) => ("bulk".to_string(), None, None),
        ("POST" | "PUT", [index, "_bulk"]) => ("bulk".to_string(), Some(index.to_string()), None),

        // Index management
        ("PUT", [index]) if !index.starts_with('_') => ("create_index".to_string(), Some(index.to_string()), None),
        ("DELETE", [index]) if !index.starts_with('_') => ("delete_index".to_string(), Some(index.to_string()), None),
        ("GET", [index]) if !index.starts_with('_') => ("index_info".to_string(), Some(index.to_string()), None),

        // Cluster operations
        ("GET", ["_cluster", "health"]) => ("cluster_health".to_string(), None, None),
        ("GET", ["_cluster", "stats"]) => ("cluster_stats".to_string(), None, None),
        ("GET", ["_cat", endpoint]) => (format!("cat_{}", endpoint), None, None),

        // Default
        _ => ("unknown".to_string(), None, None),
    }
}
