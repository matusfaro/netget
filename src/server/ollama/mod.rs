//! Ollama-compatible API server implementation
//!
//! V2: LLM controls all responses to API endpoints

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
use tracing::{debug, error, info};

use crate::server::connection::ConnectionId;
use crate::server::ollama::actions::{
    OllamaProtocol, OLLAMA_GENERATE_REQUEST_EVENT, OLLAMA_CHAT_REQUEST_EVENT,
    OLLAMA_MODELS_REQUEST_EVENT,
};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::call_llm_with_protocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Ollama-compatible API server with LLM control
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
        console_info!(status_tx, "[INFO] Ollama API server listening on {}", local_addr);

        let protocol = Arc::new(OllamaProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(
                            app_state.get_next_unified_id().await
                        );
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        console_info!(status_tx, "[INFO] Ollama API connection from {}", remote_addr);

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
                        console_info!(status_tx, "__UPDATE_UI__");

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
                        console_error!(status_tx, "[ERROR] Failed to accept Ollama API connection: {}", e);
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
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OllamaProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();

    console_debug!(status_tx, "[DEBUG] Ollama API {} {}", method, path);

    // Route the request
    match (method.clone(), path) {
        (Method::GET, "/api/tags") => {
            handle_tags_list_v2(connection_id, llm_client, app_state, status_tx, protocol, server_id).await
        }
        (Method::POST, "/api/generate") => {
            handle_generate_v2(req, connection_id, llm_client, app_state, status_tx, protocol, server_id).await
        }
        (Method::POST, "/api/chat") => {
            handle_chat_v2(req, connection_id, llm_client, app_state, status_tx, protocol, server_id).await
        }
        (Method::POST, "/api/embeddings") => {
            handle_embeddings(req, status_tx).await
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
            console_debug!(status_tx, "[DEBUG] Ollama API: Unknown endpoint {} {}", method, path);
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

/// Handle GET /api/tags - List available models (V2: LLM controlled)
async fn handle_tags_list_v2(
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OllamaProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    console_debug!(status_tx, "[DEBUG] Ollama API: Listing models (LLM controlled)");

    // Create event for models request
    let event = Event::new(
        &OLLAMA_MODELS_REQUEST_EVENT,
        json!({}),
    );

    // Get instruction for this server
    let instruction = app_state.get_instruction(server_id).await
        .unwrap_or_else(|| "Respond to Ollama API requests".to_string());

    // Build event description from instruction and event type
    let event_description = format!("{} - {}", instruction, event.event_type.description);

    // Call LLM
    match call_llm_with_protocol(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event_description,
        protocol.as_ref(),
    ).await {
        Ok(llm_result) => {
            // Look for ollama_models_response action
            for action in &llm_result.raw_actions {
                if action.get("type").and_then(|v| v.as_str()) == Some("ollama_models_response") {
                    if let Some(models) = action.get("models").and_then(|v| v.as_array()) {
                        let ollama_models: Vec<Value> = models
                            .iter()
                            .map(|model_name| {
                                let name = model_name.as_str().unwrap_or("unknown");
                                json!({
                                    "name": name,
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

                        return Ok(Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(response.to_string())))
                            .unwrap());
                    }
                }
            }

            // No action found, return empty list
            let response = json!({"models": []});
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(response.to_string())))
                .unwrap())
        }
        Err(e) => {
            console_error!(status_tx, "[ERROR] LLM error: {}", e);

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({
                    "error": format!("LLM error: {}", e)
                }).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /api/generate - Generate text (V2: LLM controlled)
async fn handle_generate_v2(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OllamaProtocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Read request body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({"error": "Failed to read body"}).to_string())))
                .unwrap());
        }
    };

    // Parse JSON
    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let prompt = request_json.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let stream = request_json.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    debug!("Generate: model={}, prompt_len={}, stream={}", model, prompt.len(), stream);

    // Create event for generate request
    let event = Event::new(
        &OLLAMA_GENERATE_REQUEST_EVENT,
        json!({
            "model": model,
            "prompt": prompt,
            "stream": stream
        }),
    );

    // Get instruction
    let instruction = app_state.get_instruction(server_id).await
        .unwrap_or_else(|| "Respond to Ollama API requests".to_string());

    // Build event description from instruction and event type
    let event_description = format!("{} - {}", instruction, event.event_type.description);

    // Call LLM
    match call_llm_with_protocol(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event_description,
        protocol.as_ref(),
    ).await {
        Ok(llm_result) => {
            // Look for ollama_generate_response action
            for action in &llm_result.raw_actions {
                if action.get("type").and_then(|v| v.as_str()) == Some("ollama_generate_response") {
                    if let Some(response_text) = action.get("response_text").and_then(|v| v.as_str()) {
                        if stream {
                            // Streaming response: send NDJSON chunks (all at once)
                            return Ok(build_streaming_generate_response(&model, response_text));
                        } else {
                            // Non-streaming response
                            let response = json!({
                                "model": model,
                                "created_at": "2024-01-01T00:00:00Z",
                                "response": response_text,
                                "done": true
                            });

                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(response.to_string())))
                                .unwrap());
                        }
                    }
                }
            }

            // No valid action, return error
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({"error": "No response from LLM"}).to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json!({"error": format!("LLM error: {}", e)}).to_string())))
                .unwrap())
        }
    }
}

/// Handle POST /api/chat - Chat completion (V2: LLM controlled)
async fn handle_chat_v2(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<OllamaProtocol>,
    server_id: crate::state::ServerId,
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

    // Parse JSON
    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    let model = request_json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let messages = request_json.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let stream = request_json.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    debug!("Chat: model={}, {} messages, stream={}", model, messages.len(), stream);

    // Create event for chat request
    let event = Event::new(
        &OLLAMA_CHAT_REQUEST_EVENT,
        json!({
            "model": model,
            "messages": messages,
            "stream": stream
        }),
    );

    // Get instruction
    let instruction = app_state.get_instruction(server_id).await
        .unwrap_or_else(|| "Respond to Ollama API requests".to_string());

    // Build event description from instruction and event type
    let event_description = format!("{} - {}", instruction, event.event_type.description);

    // Call LLM
    match call_llm_with_protocol(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event_description,
        protocol.as_ref(),
    ).await {
        Ok(llm_result) => {
            // Look for ollama_chat_response action
            for action in &llm_result.raw_actions {
                if action.get("type").and_then(|v| v.as_str()) == Some("ollama_chat_response") {
                    if let Some(message_content) = action.get("message_content").and_then(|v| v.as_str()) {
                        if stream {
                            // Streaming response: send NDJSON chunks (all at once)
                            return Ok(build_streaming_chat_response(&model, message_content));
                        } else {
                            // Non-streaming response
                            let response = json!({
                                "model": model,
                                "created_at": "2024-01-01T00:00:00Z",
                                "message": {
                                    "role": "assistant",
                                    "content": message_content
                                },
                                "done": true
                            });

                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(response.to_string())))
                                .unwrap());
                        }
                    }
                }
            }

            // No valid action
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from(json!({"error": "No response from LLM"}).to_string())))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from(json!({"error": format!("LLM error: {}", e)}).to_string())))
                .unwrap())
        }
    }
}

/// Build streaming generate response (NDJSON format, all sent at once)
fn build_streaming_generate_response(model: &str, response_text: &str) -> Response<Full<Bytes>> {
    let mut ndjson = String::new();

    // Split response into words for streaming chunks
    let words: Vec<&str> = response_text.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let chunk = json!({
            "model": model,
            "created_at": "2024-01-01T00:00:00Z",
            "response": if i == 0 { word.to_string() } else { format!(" {}", word) },
            "done": false
        });
        ndjson.push_str(&chunk.to_string());
        ndjson.push('\n');
    }

    // Final done chunk
    let final_chunk = json!({
        "model": model,
        "created_at": "2024-01-01T00:00:00Z",
        "response": "",
        "done": true
    });
    ndjson.push_str(&final_chunk.to_string());
    ndjson.push('\n');

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-ndjson")
        .body(Full::new(Bytes::from(ndjson)))
        .unwrap()
}

/// Build streaming chat response (NDJSON format, all sent at once)
fn build_streaming_chat_response(model: &str, content: &str) -> Response<Full<Bytes>> {
    let mut ndjson = String::new();

    // Split content into words for streaming chunks
    let words: Vec<&str> = content.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let chunk = json!({
            "model": model,
            "created_at": "2024-01-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": if i == 0 { word.to_string() } else { format!(" {}", word) }
            },
            "done": false
        });
        ndjson.push_str(&chunk.to_string());
        ndjson.push('\n');
    }

    // Final done chunk
    let final_chunk = json!({
        "model": model,
        "created_at": "2024-01-01T00:00:00Z",
        "message": {
            "role": "assistant",
            "content": ""
        },
        "done": true
    });
    ndjson.push_str(&final_chunk.to_string());
    ndjson.push('\n');

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-ndjson")
        .body(Full::new(Bytes::from(ndjson)))
        .unwrap()
}

// Keep the simple endpoints unchanged
async fn handle_embeddings(
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

    let _request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(json!({"error": "Invalid JSON"}).to_string())))
                .unwrap());
        }
    };

    console_debug!(status_tx, "[DEBUG] Embeddings request received");

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
    console_debug!(status_tx, "[DEBUG] Show model: {}", model);

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
    console_debug!(status_tx, "[DEBUG] Pull model: {}", model);

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
    console_debug!(status_tx, "[DEBUG] Create model: {}", model);

    let response = json!({
        "status": "success"
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(response.to_string())))
        .unwrap())
}

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
    console_debug!(status_tx, "[DEBUG] Copy model: {} -> {}", source, destination);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json!({"status": "success"}).to_string())))
        .unwrap())
}

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
    console_debug!(status_tx, "[DEBUG] Delete model: {}", model);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json!({"status": "success"}).to_string())))
        .unwrap())
}
