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
    #[serde(default)]
    format: Option<serde_json::Value>,
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

/// Ollama generate request format
#[derive(Debug, Deserialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    format: Option<serde_json::Value>,
}

/// Ollama generate response format
#[derive(Debug, Serialize)]
struct OllamaGenerateResponse {
    model: String,
    created_at: String,
    response: String,
    done: bool,
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
            .route("/api/generate", post(handle_generate))
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

    // DEBUG: Log extracted context [FIX v2]
    eprintln!("🔍🔍🔍 Mock context extracted:");
    eprintln!("  event_type: {:?}", context.event_type);
    eprintln!("  instruction: {}", &context.instruction[..context.instruction.len().min(200)]);
    eprintln!("  prompt preview: {}", &context.prompt[..context.prompt.len().min(500)]);

    // Get mock response
    let config = state.config.lock().await;

    // DEBUG: Log all rules and their match status
    eprintln!("🔍 Checking {} mock rules:", config.rules.len());
    for (idx, rule) in config.rules.iter().enumerate() {
        let matches = rule.matches(&context);
        eprintln!("  Rule #{}: {} -> {}", idx, rule.describe(), if matches { "✅ MATCH" } else { "❌ no match" });
    }

    let mock_response = match config.find_match(&context).await {
        Some((idx, response)) => {
            eprintln!("✅ Using matched rule #{}", idx);
            response
        },
        None => {
            eprintln!("❌ NO RULE MATCHED!");
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

/// Handle POST /api/generate
async fn handle_generate(
    State(state): State<ServerState>,
    Json(request): Json<OllamaGenerateRequest>,
) -> Response {
    debug!(
        "🔧 Mock Ollama received generate request: model={}, prompt_len={}",
        request.model,
        request.prompt.len()
    );

    // Extract context from prompt
    let context = extract_context_from_prompt(&request.prompt);

    // Get mock response
    let config = state.config.lock().await;
    let mock_response = match config.find_match(&context).await {
        Some((_idx, response)) => response,
        None => {
            warn!("🔧 No mock rule matched generate request, returning default error");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "No mock rule matched this request"
                })),
            )
                .into_response();
        }
    };

    debug!("🔧 Mock Ollama returning generate response");

    // Format as Ollama generate response
    let response = OllamaGenerateResponse {
        model: request.model,
        created_at: chrono::Utc::now().to_rfc3339(),
        response: mock_response.to_response_string(),
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
/// This must match the logic in src/llm/ollama_client.rs::extract_llm_context()
fn extract_context(request: &OllamaChatRequest) -> LlmContext {
    // Get the last user message (the prompt)
    let prompt = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");

    let mut context = LlmContext::new(prompt.to_string());

    // Try to extract event type from prompt
    // First look for "Event ID:" format (preferred for mock testing)
    if let Some(event_id_line) = prompt.lines().find(|line| line.contains("Event ID:")) {
        if let Some(event_id) = event_id_line.split("Event ID:").nth(1) {
            let event_id = event_id.trim();
            if !event_id.is_empty() {
                debug!("🔧 Extracted event type from Event ID: '{}'", event_id);
                context.event_type = Some(event_id.to_string());
            }
        }
    } else if let Some(event_line) = prompt.lines().find(|line| line.contains("Event:")) {
        // Fallback: try to extract from "Event:" line (legacy format)
        if let Some(event_type) = event_line.split("Event:").nth(1) {
            let event_type = event_type.trim().split_whitespace().next().unwrap_or("");
            if !event_type.is_empty() {
                debug!("🔧 Extracted event type from Event: '{}'", event_type);
                context.event_type = Some(event_type.to_string());
            }
        }
    }

    // Try to extract instruction
    // Look for "Your instruction:" or similar patterns
    if let Some(instruction_line) = prompt.lines().find(|line| {
        line.contains("Your instruction:") || line.contains("instruction:")
    }) {
        if let Some(instruction) = instruction_line
            .split("instruction:")
            .nth(1)
            .or_else(|| instruction_line.split("Your instruction:").nth(1))
        {
            let instruction_trimmed = instruction.trim();
            debug!("🔧 Extracted instruction: '{}'", instruction_trimmed);
            context.instruction = instruction_trimmed.to_string();
        }
    } else {
        debug!("🔧 Instruction extraction: no 'instruction:' line found, trying fallback");
        // Fallback: Look for the last substantial non-empty line (likely the user message)
        let lines: Vec<&str> = prompt.lines().collect();
        for line in lines.iter().rev() {
            let trimmed = line.trim();
            // Skip empty lines, very short lines, and lines that look like headers or system text
            if !trimmed.is_empty()
                && trimmed.len() > 5
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("You ")
                && !trimmed.starts_with("Your ")
                && !trimmed.starts_with("Please ")
                && !trimmed.contains("```")
                && !trimmed.starts_with("Use `/")
            {
                debug!("🔧 Extracted instruction (fallback): '{}'", trimmed);
                context.instruction = trimmed.to_string();
                break;
            }
        }
    }

    // Try to extract event data (JSON after "Context data:", "Event data:", or "Data:")
    if let Some(data_start_idx) = prompt.find("Context data:")
        .or_else(|| prompt.find("Event data:"))
        .or_else(|| prompt.find("Data:"))
    {
        let after_data = &prompt[data_start_idx..];

        // Try to find JSON object
        if let Some(json_start) = after_data.find('{') {
            let json_str = &after_data[json_start..];

            // Try to parse as JSON using streaming parser (stops at first complete JSON value)
            use serde_json::Deserializer;
            let mut deserializer = Deserializer::from_str(json_str).into_iter::<serde_json::Value>();
            if let Some(Ok(json)) = deserializer.next() {
                debug!("🔧 Successfully parsed event data: {}", json);
                context.event_data = json;
            }
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

/// Extract LLM context from a generate request prompt
fn extract_context_from_prompt(prompt: &str) -> LlmContext {
    let mut context = LlmContext::new(prompt.to_string());

    // Try to extract event type from prompt
    // First look for "Event ID:" format (preferred for mock testing)
    if let Some(event_id_line) = prompt.lines().find(|line| line.contains("Event ID:")) {
        if let Some(event_id) = event_id_line.split("Event ID:").nth(1) {
            let event_id = event_id.trim();
            if !event_id.is_empty() {
                debug!("🔧 Extracted event type from Event ID: '{}'", event_id);
                context.event_type = Some(event_id.to_string());
            }
        }
    } else if let Some(event_line) = prompt.lines().find(|line| line.contains("Event:")) {
        // Fallback: try to extract from "Event:" line (legacy format)
        if let Some(event_type) = event_line.split("Event:").nth(1) {
            let event_type = event_type.trim().split_whitespace().next().unwrap_or("");
            if !event_type.is_empty() {
                debug!("🔧 Extracted event type from Event: '{}'", event_type);
                context.event_type = Some(event_type.to_string());
            }
        }
    }

    // Try to extract instruction - look for patterns
    if let Some(instruction_line) = prompt.lines().find(|line| {
        line.contains("Your instruction:") || line.contains("instruction:")
    }) {
        if let Some(instruction) = instruction_line
            .split("instruction:")
            .nth(1)
            .or_else(|| instruction_line.split("Your instruction:").nth(1))
        {
            let instruction_trimmed = instruction.trim();
            debug!("🔧 Extracted instruction: '{}'", instruction_trimmed);
            context.instruction = instruction_trimmed.to_string();
        }
    } else {
        debug!("🔧 Instruction extraction: no 'instruction:' line found, trying fallback");
        // Fallback: Look for the last substantial non-empty line
        let lines: Vec<&str> = prompt.lines().collect();
        for line in lines.iter().rev() {
            let trimmed = line.trim();
            if !trimmed.is_empty()
                && trimmed.len() > 5
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("You ")
                && !trimmed.starts_with("Your ")
                && !trimmed.starts_with("Please ")
                && !trimmed.contains("```")
                && !trimmed.starts_with("Use `/")
            {
                debug!("🔧 Extracted instruction (fallback): '{}'", trimmed);
                context.instruction = trimmed.to_string();
                break;
            }
        }
    }

    // Try to extract event data
    if let Some(data_start_idx) = prompt.find("Context data:")
        .or_else(|| prompt.find("Event data:"))
        .or_else(|| prompt.find("Data:"))
    {
        let after_data = &prompt[data_start_idx..];
        if let Some(json_start) = after_data.find('{') {
            let json_str = &after_data[json_start..];
            use serde_json::Deserializer;
            let mut deserializer = Deserializer::from_str(json_str).into_iter::<serde_json::Value>();
            if let Some(Ok(json)) = deserializer.next() {
                debug!("🔧 Successfully parsed event data: {}", json);
                context.event_data = json;
            }
        }
    }

    context
}
