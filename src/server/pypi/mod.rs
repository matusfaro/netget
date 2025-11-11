//! PyPI (Python Package Index) server implementation
//!
//! Implements PEP 503 Simple Repository API for serving Python packages.

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
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::PypiProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_info, console_trace, console_warn};
use actions::PYPI_REQUEST_EVENT;

/// PyPI server that delegates package serving to LLM
pub struct PypiServer;

impl PypiServer {
    /// Spawn the PyPI server with integrated LLM actions
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
        console_info!(status_tx, "PyPI server listening on {}", local_addr);

        let protocol = Arc::new(PypiProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        console_info!(
                            status_tx,
                            "Accepted PyPI connection {} from {}",
                            connection_id,
                            remote_addr
                        );

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

                            // Create a service that handles requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_pypi_request_with_llm_actions(
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
                            if let Err(err) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                error!("Error serving PyPI connection: {:?}", err);
                                let _ = status_tx_clone.send(format!(
                                    "[ERROR] Error serving PyPI connection: {:?}",
                                    err
                                ));
                            }

                            // Mark connection as closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone
                                .send(format!("✗ PyPI connection {connection_id} closed"));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept PyPI connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single PyPI request with integrated LLM actions
async fn handle_pypi_request_with_llm_actions(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<PypiProtocol>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details first for logging
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
            console_error!(status_tx, "Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // Determine request type based on path
    let request_type = if path == "/" || path == "/simple" || path == "/simple/" {
        "list_packages"
    } else if path.starts_with("/simple/") && path.ends_with('/') {
        "list_files"
    } else if path.starts_with("/packages/") {
        "download_file"
    } else {
        "unknown"
    };

    // Extract package name if applicable
    let package_name = if request_type == "list_files" {
        path.trim_start_matches("/simple/")
            .trim_end_matches('/')
            .to_string()
    } else if request_type == "download_file" {
        path.trim_start_matches("/packages/")
            .split('/')
            .last()
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    // DEBUG: Log request summary to both file and TUI
    debug!(
        "PyPI request: {} {} [{}] ({} bytes) from {:?}",
        method,
        uri,
        request_type,
        body_bytes.len(),
        connection_id
    );
    let _ = status_tx.send(format!(
        "[DEBUG] PyPI request: {} {} [{}] ({} bytes)",
        method,
        uri,
        request_type,
        body_bytes.len()
    ));

    // TRACE: Log full request details
    trace!("PyPI request headers:");
    for (name, value) in &headers {
        trace!("  {}: {}", name, value);
    }

    // Create PyPI request event
    let body_text = String::from_utf8_lossy(&body_bytes);
    let event = Event::new(
        &PYPI_REQUEST_EVENT,
        serde_json::json!({
            "method": method,
            "uri": uri,
            "path": path,
            "headers": headers,
            "body": body_text,
            "request_type": request_type,
            "package_name": package_name,
        }),
    );

    // Call LLM to generate PyPI response
    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    )
    .await
    {
        Ok(execution_result) => {
            debug!("LLM PyPI response received");
            let _ = status_tx.send("[DEBUG] LLM PyPI response received".to_string());

            // Display messages
            for msg in execution_result.messages {
                let _ = status_tx.send(msg);
            }

            // Extract PyPI response from protocol results
            // Default response in case nothing was produced
            let mut status_code = 200;
            let mut response_headers = HashMap::new();
            let mut response_body = String::new();

            for protocol_result in execution_result.protocol_results {
                if let ActionResult::Output(output_data) = protocol_result {
                    // Parse the output as JSON containing PyPI response fields
                    if let Ok(json_value) =
                        serde_json::from_slice::<serde_json::Value>(&output_data)
                    {
                        if let Some(status) = json_value.get("status").and_then(|v| v.as_u64()) {
                            status_code = status as u16;
                        }
                        if let Some(headers_obj) =
                            json_value.get("headers").and_then(|v| v.as_object())
                        {
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
            }

            let _ = status_tx.send(format!(
                "→ PyPI {} {} → {} ({} bytes)",
                method,
                uri,
                status_code,
                response_body.len()
            ));

            // Build the HTTP response
            let mut response = Response::builder().status(status_code);

            // Add headers
            for (name, value) in response_headers {
                response = response.header(name, value);
            }

            Ok(response
                .body(Full::new(Bytes::from(response_body)))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error generating PyPI response: {}", e);
            let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, e));

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap())
        }
    }
}
