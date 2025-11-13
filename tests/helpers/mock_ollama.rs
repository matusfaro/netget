//! Mock Ollama HTTP server for E2E testing
//!
//! This server mimics Ollama's HTTP API and allows tests to provide
//! mock responses without needing a real Ollama instance.
//!
//! Key benefits:
//! - Call counting works (server tracks calls in same process as test)
//! - No subprocess memory isolation issues
//! - True E2E testing (netget subprocess makes real HTTP calls)
//! - Easy debugging (server can log all requests)

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use netget::testing::{LlmContext, MockLlmConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, info, warn};

use super::common::E2EResult;

/// Mock Ollama server that responds with configured mock responses
pub struct MockOllamaServer {
    /// Port the server is listening on
    pub port: u16,
    /// Mock configuration (shared with handler)
    config: Arc<Mutex<MockLlmConfig>>,
    /// Shutdown signal
    _shutdown_tx: oneshot::Sender<()>,
}

/// Ollama chat request format
#[derive(Debug, Deserialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(default)]
    stream: bool,
}

/// Ollama message format
#[derive(Debug, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

/// Ollama chat response format
#[derive(Debug, Serialize)]
struct OllamaChatResponse {
    model: String,
    created_at: String,
    message: OllamaResponseMessage,
    done: bool,
}

/// Ollama response message
#[derive(Debug, Serialize)]
struct OllamaResponseMessage {
    role: String,
    content: String,
}

/// Ollama tags/models response
#[derive(Debug, Serialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Serialize)]
struct OllamaModel {
    name: String,
    modified_at: String,
    size: u64,
}

/// Shared state for axum handlers
#[derive(Clone)]
struct ServerState {
    config: Arc<Mutex<MockLlmConfig>>,
}

impl MockOllamaServer {
    /// Start a mock Ollama server on a random available port
    ///
    /// The server will:
    /// 1. Listen on 127.0.0.1 with OS-assigned port
    /// 2. Implement `/api/chat` and `/api/tags` endpoints
    /// 3. Use provided mock config to generate responses
    /// 4. Track call counts for verification
    pub async fn start(config: MockLlmConfig) -> E2EResult<Self> {
        let config = Arc::new(Mutex::new(config));
        let state = ServerState {
            config: config.clone(),
        };

        // Build router with Ollama-compatible endpoints
        let app = Router::new()
            .route("/api/chat", post(handle_chat))
            .route("/api/tags", get(handle_tags))
            .route("/api/version", get(handle_version))
            .with_state(state);

        // Bind to random port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind mock Ollama server: {}", e))?;

        let port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?
            .port();

        info!("🔧 Mock Ollama server starting on port {}", port);

        // Create shutdown channel
        let (_shutdown_tx, shutdown_rx) = oneshot::channel();

        // Spawn server in background
        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            });

            if let Err(e) = server.await {
                warn!("Mock Ollama server error: {}", e);
            }
        });

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        info!("✅ Mock Ollama server ready on port {}", port);

        Ok(Self {
            port,
            config,
            _shutdown_tx,
        })
    }

    /// Get the base URL for this server
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Verify that all mock expectations were met
    pub async fn verify_calls(&self) -> E2EResult<()> {
        let config = self.config.lock().await;
        config.mark_verified();

        let mut errors = Vec::new();

        for (idx, rule) in config.rules.iter().enumerate() {
            let actual = rule.actual_calls.load(std::sync::atomic::Ordering::SeqCst);

            // Check exact count
            if let Some(expected) = rule.expected_calls {
                if actual != expected {
                    errors.push(format!(
                        "Rule #{} ({}): Expected {} calls, got {}",
                        idx,
                        rule.describe(),
                        expected,
                        actual
                    ));
                }
            }

            // Check minimum
            if let Some(min) = rule.min_calls {
                if actual < min {
                    errors.push(format!(
                        "Rule #{} ({}): Expected at least {} calls, got {}",
                        idx,
                        rule.describe(),
                        min,
                        actual
                    ));
                }
            }

            // Check maximum
            if let Some(max) = rule.max_calls {
                if actual > max {
                    errors.push(format!(
                        "Rule #{} ({}): Expected at most {} calls, got {}",
                        idx,
                        rule.describe(),
                        max,
                        actual
                    ));
                }
            }
        }

        if !errors.is_empty() {
            let mut error_msg = String::from("Mock verification failed:\n");
            for error in &errors {
                error_msg.push_str("  ");
                error_msg.push_str(error);
                error_msg.push('\n');
            }

            // Get call history for debugging
            let history = config.call_history.lock().await;
            if !history.is_empty() {
                error_msg.push_str("\nAll LLM call history:\n");
                for (idx, call) in history.iter().enumerate() {
                    error_msg.push_str(&format!("  Call #{}: ", idx + 1));
                    error_msg.push_str(&format!("instruction=\"{}\" ", call.context.instruction));
                    if let Some(ref event_type) = call.context.event_type {
                        error_msg.push_str(&format!("event_type=\"{}\" ", event_type));
                    }
                    error_msg.push('\n');
                }
            }

            return Err(error_msg.into());
        }

        Ok(())
    }
}

/// Handle POST /api/chat
async fn handle_chat(
    State(state): State<ServerState>,
    Json(request): Json<OllamaChatRequest>,
) -> Response {
    debug!(
        "🔧 Mock Ollama received chat request: model={}, messages={}",
        request.model,
        request.messages.len()
    );

    // Extract context from request
    let context = extract_context(&request);

    // Get mock response
    let config = state.config.lock().await;
    let mock_response = match config.find_match(&context).await {
        Some((_idx, response)) => response,
        None => {
            warn!("🔧 No mock rule matched, returning default error");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "No mock rule matched this request"
                })),
            )
                .into_response();
        }
    };

    debug!("🔧 Mock Ollama returning response");

    // Format as Ollama chat response
    let response = OllamaChatResponse {
        model: request.model,
        created_at: chrono::Utc::now().to_rfc3339(),
        message: OllamaResponseMessage {
            role: "assistant".to_string(),
            content: mock_response.to_response_string(),
        },
        done: true,
    };

    Json(response).into_response()
}

/// Handle GET /api/tags (list models)
async fn handle_tags() -> Response {
    debug!("🔧 Mock Ollama received tags request");

    let response = OllamaTagsResponse {
        models: vec![OllamaModel {
            name: "qwen3-coder:30b".to_string(),
            modified_at: chrono::Utc::now().to_rfc3339(),
            size: 1000000,
        }],
    };

    Json(response).into_response()
}

/// Handle GET /api/version
async fn handle_version() -> Response {
    debug!("🔧 Mock Ollama received version request");
    Json(serde_json::json!({"version": "0.0.1-mock"})).into_response()
}

/// Extract LLM context from Ollama request
fn extract_context(request: &OllamaChatRequest) -> LlmContext {
    let mut context = LlmContext::new(String::new());

    // Extract instruction from last user message
    if let Some(last_user_msg) = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
    {
        // Check if this looks like a full prompt (has "# Role" section)
        if last_user_msg.content.contains("# Role") {
            // Parse structured sections
            context.instruction = extract_section(&last_user_msg.content, "# User Input")
                .unwrap_or_else(|| last_user_msg.content.clone());
            context.event_type = extract_event_type(&last_user_msg.content);
        } else {
            // Simple instruction
            context.instruction = last_user_msg.content.clone();
        }
    }

    // Determine iteration (how many assistant messages so far)
    context.iteration = request
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .count();

    context
}

/// Extract a section from a prompt (e.g., "# User Input" → "...")
fn extract_section(content: &str, header: &str) -> Option<String> {
    let start_marker = format!("{}\n", header);
    let start = content.find(&start_marker)?;
    let after_header = start + start_marker.len();

    // Find next header or end of string
    let remaining = &content[after_header..];
    let end = remaining
        .find("\n# ")
        .unwrap_or(remaining.len());

    Some(remaining[..end].trim().to_string())
}

/// Extract event type from event data section
fn extract_event_type(content: &str) -> Option<String> {
    // Look for event type in "# Current Event" section
    let event_section = extract_section(content, "# Current Event")?;

    // Parse JSON to get event_type field
    if let Ok(event_json) = serde_json::from_str::<serde_json::Value>(&event_section) {
        event_json["event_type"]
            .as_str()
            .map(|s| s.to_string())
    } else {
        None
    }
}
