//! HTTP server implementation using hyper

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
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::network::connection::ConnectionId;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for HTTP stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
    let context = r#"You are handling HTTP requests. The data contains parsed HTTP request details with method, URI, headers, and body.
Return appropriate HTTP status codes (200, 404, 500, etc.) with headers and body."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "status": 200,  // HTTP status code
  "headers": {"Content-Type": "text/html"},  // Response headers
  "body": "Response body",  // Response body
  "message": null,  // Optional message for user
  "set_memory": null,  // Replace global memory
  "append_memory": null,  // Append to global memory
  "set_connection_memory": null,  // Replace connection memory
  "append_connection_memory": null  // Append to connection memory
}"#;

    (context, output_format)
}

/// HTTP server that delegates request handling to LLM
pub struct HttpServer;

impl HttpServer {
    /// Spawn the HTTP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> anyhow::Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("HTTP server listening on {}", local_addr);

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted HTTP connection {} from {}", connection_id, remote_addr);

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();

                            // Create a service that handles requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_clone.clone();
                                let status_clone = status_for_service.clone();
                                handle_http_request_with_llm(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving HTTP connection: {:?}", err);
                            }

                            let _ = status_tx_clone.send(format!("✗ HTTP connection {} closed", connection_id));
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

/// Handle a single HTTP request with integrated LLM
async fn handle_http_request_with_llm(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!(
        "HTTP request: {} {} from {:?}",
        req.method(),
        req.uri(),
        connection_id
    );

    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();

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

    // Build event description for HTTP request
    let headers_text = headers.iter()
        .map(|(k, v)| format!("  {}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");
    let body_text = String::from_utf8_lossy(&body_bytes);

    let event_description = format!(
        r#"HTTP Request:
- Method: {}
- URI: {}
- Headers:
{}
- Body: {}"#,
        method,
        uri,
        if headers_text.is_empty() { "  (none)" } else { &headers_text },
        if body_text.is_empty() { "(empty)" } else { &body_text }
    );

    // Build prompt and call LLM
    let model = app_state.get_ollama_model().await;
    let prompt_config = get_llm_prompt_config();
    let conn_memory = String::new(); // HTTP doesn't use per-connection memory yet

    let prompt = PromptBuilder::build_network_event_prompt(
        &app_state,
        connection_id,
        &conn_memory,
        &event_description,
        prompt_config,
    ).await;

    // Call LLM to generate HTTP response
    match llm_client.generate_http_response(&model, &prompt).await {
        Ok(llm_response) => {
            // Handle memory updates (HTTP response doesn't use ProcessedResponse, handle manually)
            if let Some(set_global) = llm_response.set_memory.clone() {
                app_state.set_memory(set_global).await;
            }
            if let Some(append_global) = llm_response.append_memory.clone() {
                let current = app_state.get_memory().await;
                let new_memory = if current.is_empty() {
                    append_global
                } else {
                    format!("{}\n{}", current, append_global)
                };
                app_state.set_memory(new_memory).await;
            }

            // Log if requested
            if let Some(log_msg) = &llm_response.log_message {
                info!("{}", log_msg);
            }

            let _ = status_tx.send(format!(
                "→ HTTP {} {} → {} ({} bytes)",
                method, uri, llm_response.status, llm_response.body.len()
            ));

            // Build the HTTP response
            let mut response = Response::builder().status(llm_response.status);

            // Add headers
            for (name, value) in llm_response.headers {
                response = response.header(name, value);
            }

            Ok(response.body(Full::new(Bytes::from(llm_response.body))).unwrap())
        }
        Err(e) => {
            error!("LLM error generating HTTP response: {}", e);
            let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, e));

            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap())
        }
    }
}
