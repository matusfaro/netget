//! HTTP server implementation using hyper
pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::server::connection::ConnectionId;
use actions::HTTP_REQUEST_EVENT;
use crate::server::HttpProtocol;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// HTTP server that delegates request handling to LLM
pub struct HttpServer;

impl HttpServer {
    /// Spawn the HTTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("HTTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(HttpProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Accepted HTTP connection {} from {}", connection_id, remote_addr);

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
                            protocol_info: ProtocolConnectionInfo::Http {
                                recent_requests: Vec::new(),
                            },
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

                            // Create a service that handles requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_http_request_with_llm_actions(
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
                                error!("Error serving HTTP connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("✗ HTTP connection {connection_id} closed"));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept HTTP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single HTTP request with integrated LLM actions
async fn handle_http_request_with_llm_actions(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<HttpProtocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Use shared request extraction logic
    let request_data = crate::server::http_common::handler::extract_request_data(
        req,
        "HTTP",
        &status_tx,
    ).await;

    // Create HTTP request event (no version field for HTTP/1.1)
    let body_text = String::from_utf8_lossy(&request_data.body_bytes);
    let event = Event::new(&HTTP_REQUEST_EVENT, serde_json::json!({
        "method": request_data.method,
        "uri": request_data.uri,
        "headers": request_data.headers,
        "body": if body_text.is_empty() { "" } else { body_text.as_ref() }
    }));

    // Call LLM to generate HTTP response
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    ).await {
        Ok(execution_result) => {
            debug!("LLM HTTP response received");

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Use shared response building logic
            crate::server::http_common::handler::build_response(
                execution_result.protocol_results,
                "HTTP",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
        Err(e) => {
            // Use shared error response building
            crate::server::http_common::handler::build_error_response(
                e,
                "HTTP",
                &request_data.method,
                &request_data.uri,
                &status_tx,
            )
        }
    }
}
