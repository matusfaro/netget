//! OpenAI-compatible API server implementation
//!
//! OpenAI API runs over HTTP. The LLM uses Ollama to generate chat completions
//! and return them in OpenAI-compatible format.

pub mod actions;

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
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::server::connection::ConnectionId;
use crate::server::openai::actions::OpenAiProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;

/// OpenAI-compatible API server that delegates to LLM/Ollama
pub struct OpenAiServer;

impl OpenAiServer {
    /// Spawn the OpenAI API server with integrated LLM actions
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
        info!("OpenAI API server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] OpenAI API server listening on {}", local_addr));

        let protocol = Arc::new(OpenAiProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("OpenAI API connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] OpenAI API connection from {}", remote_addr));

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
                            protocol_info: ProtocolConnectionInfo::OpenAi {
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

                            // Create a service that handles OpenAI API requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_openai_request(
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
                                error!("Error serving OpenAI API connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] OpenAI API connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept OpenAI API connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept OpenAI API connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single OpenAI API request
async fn handle_openai_request(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    _protocol: Arc<OpenAiProtocol>,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();

    debug!("OpenAI API request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] OpenAI API {} {}", method, path));

    // Route the request
    match (method.clone(), path) {
        (Method::GET, "/v1/models") => {
            handle_models_list(llm_client, status_tx).await
        }
        (Method::POST, "/v1/chat/completions") => {
            handle_chat_completions(req, llm_client, app_state, status_tx).await
        }
        _ => {
            debug!("OpenAI API: Unknown endpoint {} {}", method, path);
            let _ = status_tx.send(format!("[DEBUG] OpenAI API: Unknown endpoint {} {}", method, path));
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": {
                        "message": "Not Found",
                        "type": "invalid_request_error",
                        "code": "not_found"
                    }
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle GET /v1/models - List available models from Ollama
async fn handle_models_list(
    llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("OpenAI API: Listing models from Ollama");
    let _ = status_tx.send("[DEBUG] OpenAI API: Listing models from Ollama".to_string());

    // Get models from Ollama
    match llm_client.list_models().await {
        Ok(models) => {
            trace!("Ollama models: {:?}", models);
            let _ = status_tx.send(format!("[TRACE] Found {} models from Ollama", models.len()));

            // Convert to OpenAI format
            let openai_models: Vec<Value> = models
                .iter()
                .map(|model| {
                    json!({
                        "id": model,
                        "object": "model",
                        "created": 1686935002, // Static timestamp
                        "owned_by": "ollama"
                    })
                })
                .collect();

            let response = json!({
                "object": "list",
                "data": openai_models
            });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(response.to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("Failed to list Ollama models: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to list Ollama models: {}", e));

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": {
                        "message": format!("Failed to list models: {}", e),
                        "type": "server_error",
                        "code": "internal_error"
                    }
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /v1/chat/completions - Generate chat completion
async fn handle_chat_completions(
    req: Request<Incoming>,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read request body: {}", e));
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": {
                        "message": "Failed to read request body",
                        "type": "invalid_request_error"
                    }
                }).to_string())))
                .unwrap());
        }
    };

    // Parse JSON
    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to parse JSON: {}", e));
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": {
                        "message": "Invalid JSON",
                        "type": "invalid_request_error"
                    }
                }).to_string())))
                .unwrap());
        }
    };

    trace!("Chat completion request: {}", serde_json::to_string_pretty(&request_json).unwrap_or_default());
    let _ = status_tx.send(format!("[TRACE] Chat completion request: {}", serde_json::to_string_pretty(&request_json).unwrap_or_default()));

    // Extract model and messages
    let model = match request_json.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => app_state.get_ollama_model().await,
    };

    let messages = request_json.get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    debug!("Chat completion: model={}, {} messages", model, messages.len());
    let _ = status_tx.send(format!("[DEBUG] Chat completion: model={}, {} messages", model, messages.len()));

    // Convert messages to Ollama format and generate response
    match generate_chat_response(&llm_client, &model, messages, &request_json, &status_tx).await {
        Ok(response_text) => {
            // Build OpenAI-compatible response
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let completion_id = format!("chatcmpl-{}", created);

            let response = json!({
                "id": completion_id,
                "object": "chat.completion",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": response_text
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 0,
                    "completion_tokens": 0,
                    "total_tokens": 0
                }
            });

            trace!("Chat completion response: {}", serde_json::to_string_pretty(&response).unwrap_or_default());
            let _ = status_tx.send(format!("[TRACE] Chat completion response generated"));

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(response.to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("Failed to generate chat response: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to generate chat response: {}", e));

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": {
                        "message": format!("Failed to generate response: {}", e),
                        "type": "server_error",
                        "code": "internal_error"
                    }
                }).to_string())))
                .unwrap())
        }
    }
}

/// Generate chat response using Ollama
async fn generate_chat_response(
    llm_client: &OllamaClient,
    model: &str,
    messages: Vec<Value>,
    request_json: &Value,
    status_tx: &mpsc::UnboundedSender<String>,
) -> anyhow::Result<String> {
    // Extract parameters
    let temperature = request_json.get("temperature").and_then(|v| v.as_f64());
    let max_tokens = request_json.get("max_tokens").and_then(|v| v.as_u64());
    let top_p = request_json.get("top_p").and_then(|v| v.as_f64());

    debug!("Generating response with model={}, temp={:?}, max_tokens={:?}, top_p={:?}",
           model, temperature, max_tokens, top_p);
    let _ = status_tx.send(format!("[DEBUG] Generating response with model={}", model));

    // Build prompt from messages
    let mut prompt = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        prompt.push_str(&format!("{}: {}\n", role, content));
    }
    prompt.push_str("assistant: ");

    debug!("Calling Ollama with prompt length: {}", prompt.len());

    // Call Ollama
    let response = llm_client
        .generate(model, &prompt)
        .await?;

    Ok(response)
}
