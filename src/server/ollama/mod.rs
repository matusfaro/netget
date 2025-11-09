//! Ollama-compatible API server implementation
//!
//! Ollama API runs over HTTP. The LLM controls responses to API endpoints
//! like /api/generate, /api/chat, /api/tags, etc.

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
use crate::server::ollama::actions::OllamaProtocol;
use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;

/// Ollama-compatible API server that delegates to LLM
pub struct OllamaServer;

impl OllamaServer {
    /// Spawn the Ollama API server with integrated LLM actions
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
        info!("Ollama API server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] Ollama API server listening on {}", local_addr));

        let protocol = Arc::new(OllamaProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Ollama API connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] Ollama API connection from {}", remote_addr));

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

                            // Create a service that handles Ollama API requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_ollama_request(
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
                                error!("Error serving Ollama API connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] Ollama API connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Ollama API connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept Ollama API connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single Ollama API request
async fn handle_ollama_request(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    _protocol: Arc<OllamaProtocol>,
    _server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();

    debug!("Ollama API request: {} {}", method, path);
    let _ = status_tx.send(format!("[DEBUG] Ollama API {} {}", method, path));

    // Route the request
    match (method.clone(), path) {
        (Method::GET, "/api/tags") => {
            handle_tags_list(llm_client, status_tx).await
        }
        (Method::POST, "/api/generate") => {
            handle_generate(req, llm_client, app_state, status_tx).await
        }
        (Method::POST, "/api/chat") => {
            handle_chat(req, llm_client, app_state, status_tx).await
        }
        (Method::POST, "/api/embeddings") => {
            handle_embeddings(req, llm_client, status_tx).await
        }
        (Method::POST, "/api/show") => {
            handle_show(req, status_tx).await
        }
        (Method::POST, "/api/pull") => {
            handle_pull(req, status_tx).await
        }
        (Method::POST, "/api/create") => {
            handle_create(req, status_tx).await
        }
        (Method::POST, "/api/copy") => {
            handle_copy(req, status_tx).await
        }
        (Method::DELETE, "/api/delete") => {
            handle_delete(req, status_tx).await
        }
        _ => {
            debug!("Ollama API: Unknown endpoint {} {}", method, path);
            let _ = status_tx.send(format!("[DEBUG] Ollama API: Unknown endpoint {} {}", method, path));
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": "Not Found"
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle GET /api/tags - List available models
async fn handle_tags_list(
    llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("Ollama API: Listing models");
    let _ = status_tx.send("[DEBUG] Ollama API: Listing models".to_string());

    // Get models from Ollama
    match llm_client.list_models().await {
        Ok(models) => {
            trace!("Ollama models: {:?}", models);
            let _ = status_tx.send(format!("[TRACE] Found {} models from Ollama", models.len()));

            // Convert to Ollama format
            let ollama_models: Vec<Value> = models
                .iter()
                .map(|model| {
                    json!({
                        "name": model,
                        "modified_at": "2024-01-01T00:00:00Z",
                        "size": 0,
                        "digest": "0000000000000000",
                        "details": {
                            "format": "gguf",
                            "family": "llama"
                        }
                    })
                })
                .collect();

            let response = json!({
                "models": ollama_models
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
                    "error": format!("Failed to list models: {}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /api/generate - Generate text
async fn handle_generate(
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
                    "error": "Failed to read request body"
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
                    "error": "Invalid JSON"
                }).to_string())))
                .unwrap());
        }
    };

    trace!("Generate request: {}", serde_json::to_string_pretty(&request_json).unwrap_or_default());
    let _ = status_tx.send(format!("[TRACE] Generate request"));

    // Extract model and prompt
    let model = match request_json.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => app_state.get_ollama_model().await,
    };

    let prompt = request_json.get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    debug!("Generate: model={}, prompt_len={}", model, prompt.len());
    let _ = status_tx.send(format!("[DEBUG] Generate: model={}, prompt_len={}", model, prompt.len()));

    // Generate response using Ollama
    match llm_client.generate(&model, prompt).await {
        Ok(response_text) => {
            let response = json!({
                "model": model,
                "created_at": "2024-01-01T00:00:00Z",
                "response": response_text,
                "done": true
            });

            trace!("Generate response generated");
            let _ = status_tx.send("[TRACE] Generate response generated".to_string());

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(response.to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("Failed to generate response: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to generate response: {}", e));

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": format!("Failed to generate: {}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /api/chat - Chat completion
async fn handle_chat(
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
                    "error": "Failed to read request body"
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
                    "error": "Invalid JSON"
                }).to_string())))
                .unwrap());
        }
    };

    trace!("Chat request: {}", serde_json::to_string_pretty(&request_json).unwrap_or_default());

    // Extract model and messages
    let model = match request_json.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => app_state.get_ollama_model().await,
    };

    let messages = request_json.get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    debug!("Chat: model={}, {} messages", model, messages.len());
    let _ = status_tx.send(format!("[DEBUG] Chat: model={}, {} messages", model, messages.len()));

    // Build prompt from messages
    let mut prompt = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        prompt.push_str(&format!("{}: {}\n", role, content));
    }
    prompt.push_str("assistant: ");

    // Generate response using Ollama
    match llm_client.generate(&model, &prompt).await {
        Ok(response_text) => {
            let response = json!({
                "model": model,
                "created_at": "2024-01-01T00:00:00Z",
                "message": {
                    "role": "assistant",
                    "content": response_text
                },
                "done": true
            });

            trace!("Chat response generated");
            let _ = status_tx.send("[TRACE] Chat response generated".to_string());

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
                    "error": format!("Failed to chat: {}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /api/embeddings - Generate embeddings
async fn handle_embeddings(
    req: Request<Incoming>,
    _llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let _request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let _ = status_tx.send("[DEBUG] Embeddings request received".to_string());

    // Return mock embeddings (768 dimensions)
    let embedding: Vec<f32> = (0..768).map(|i| (i as f32) / 768.0).collect();

    let response = json!({
        "embedding": embedding
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap())
}

/// Handle POST /api/show - Show model info
async fn handle_show(
    req: Request<Incoming>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
    let _ = status_tx.send(format!("[DEBUG] Show model: {}", model));

    let response = json!({
        "modelfile": format!("FROM {}", model),
        "parameters": "temperature 0.7",
        "template": "{{ .Prompt }}",
        "details": {
            "format": "gguf",
            "family": "llama"
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap())
}

/// Handle POST /api/pull - Pull a model
async fn handle_pull(
    req: Request<Incoming>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
    let _ = status_tx.send(format!("[DEBUG] Pull model: {}", model));

    let response = json!({
        "status": "success",
        "digest": "sha256:0000000000000000",
        "total": 1000000
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap())
}

/// Handle POST /api/create - Create a model
async fn handle_create(
    req: Request<Incoming>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
    let _ = status_tx.send(format!("[DEBUG] Create model: {}", model));

    let response = json!({
        "status": "success"
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap())
}

/// Handle POST /api/copy - Copy a model
async fn handle_copy(
    req: Request<Incoming>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let source = request_json.get("source").and_then(|v| v.as_str()).unwrap_or("unknown");
    let destination = request_json.get("destination").and_then(|v| v.as_str()).unwrap_or("unknown");
    let _ = status_tx.send(format!("[DEBUG] Copy model: {} -> {}", source, destination));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json!({"status": "success"}).to_string())))
        .unwrap())
}

/// Handle DELETE /api/delete - Delete a model
async fn handle_delete(
    req: Request<Incoming>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
    let _ = status_tx.send(format!("[DEBUG] Delete model: {}", model));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json!({"status": "success"}).to_string())))
        .unwrap())
}
