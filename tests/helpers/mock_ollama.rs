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
use super::mock_config::MockLlmConfig;
use super::mock_matcher::LlmContext;
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
        // Add timeout to prevent deadlock on lock acquisition
        let config = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.config.lock()
        )
        .await
        .map_err(|_| "Timeout acquiring mock config lock (deadlock detected)")?;

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

            // Get call history for debugging (with timeout to prevent deadlock)
            let history = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                config.call_history.lock()
            )
            .await
            .map_err(|_| "Timeout acquiring call history lock (deadlock detected)")?;
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

    // Get mock response (using synchronous matching to avoid holding lock across await)
    let match_result = {
        let config = state.config.lock().await;

        // DEBUG: Log all rules and their match status
        eprintln!("🔍 Checking {} mock rules:", config.rules.len());
        for (idx, rule) in config.rules.iter().enumerate() {
            let matches = rule.matches(&context);
            eprintln!("  Rule #{}: {} -> {}", idx, rule.describe(), if matches { "✅ MATCH" } else { "❌ no match" });
        }

        config.find_match_sync(&context)
    }; // Lock is dropped here!

    // Record call history AFTER releasing config lock
    let mock_response = match match_result {
        Some((idx, response, description)) => {
            eprintln!("✅ Using matched rule #{}", idx);
            // Record call in separate lock acquisition
            state.config.lock().await.record_call(context.clone(), idx, description).await;
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
            content: mock_response.to_response_string(Some(&context.event_data)),
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

    // DEBUG: Log extracted context
    eprintln!("🔍🔍🔍 Mock generate context extracted:");
    eprintln!("  event_type: {:?}", context.event_type);
    // Use chars().take() to avoid splitting UTF-8 characters
    let instruction_preview: String = context.instruction.chars().take(200).collect();
    eprintln!("  instruction: {}", instruction_preview);
    eprintln!("  prompt length: {} chars", request.prompt.len());
    let first_500: String = request.prompt.chars().take(500).collect();
    eprintln!("  prompt first 500 chars: {}", first_500);
    // For last 500 chars, skip first N chars and take remaining
    let skip_count = request.prompt.chars().count().saturating_sub(500);
    let last_500: String = request.prompt.chars().skip(skip_count).collect();
    eprintln!("  prompt last 500 chars: {}", last_500);

    // Get mock response (using synchronous matching to avoid holding lock across await)
    let match_result = {
        let config = state.config.lock().await;

        // DEBUG: Log all rules and their match status
        eprintln!("🔍 Checking {} mock rules:", config.rules.len());
        for (idx, rule) in config.rules.iter().enumerate() {
            let matches = rule.matches(&context);
            eprintln!("  Rule #{}: {} -> {}", idx, rule.describe(), if matches { "✅ MATCH" } else { "❌ no match" });
        }

        config.find_match_sync(&context)
    }; // Lock is dropped here!

    // Record call history AFTER releasing config lock
    let mock_response = match match_result {
        Some((idx, response, description)) => {
            eprintln!("✅ Using matched rule #{}", idx);
            // Record call in separate lock acquisition
            state.config.lock().await.record_call(context.clone(), idx, description).await;
            response
        },
        None => {
            eprintln!("❌ NO RULE MATCHED!");
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
        response: mock_response.to_response_string(Some(&context.event_data)),
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
    // First, try to find the user input section at the end of the prompt
    // User input typically appears after system capabilities or markers like "## System Capabilities"
    let has_user_message = if let Some(cap_idx) = prompt.find("## System Capabilities").or_else(|| prompt.find("# Current State")) {
        // Extract everything after the capabilities section
        let after_cap = &prompt[cap_idx..];

        // Try to find the end of the capabilities section using various markers
        let end_marker_and_len = [
            ("DataLink protocol unavailable", "DataLink protocol unavailable".len()),
            ("- **Raw socket access**: ✓ Available", "- **Raw socket access**: ✓ Available".len()),
            ("- **Raw socket access**: ✗ Unavailable", "- **Raw socket access**: ✗ Unavailable".len()),
            ("- **Privileged ports (<1024)**: ✓ Available", "- **Privileged ports (<1024)**: ✓ Available".len()),
        ];

        let mut after_system = None;
        for (marker, len) in &end_marker_and_len {
            if let Some(end_idx) = after_cap.find(marker) {
                after_system = Some(&after_cap[end_idx + len..]);
                debug!("🔧 Found capabilities end marker: '{}'", marker);
                break;
            }
        }

        if let Some(after_system) = after_system {
            // Collect ALL non-empty lines after the system section as the instruction
            // (not just the first line, to support multi-line instructions)
            let instruction_text = after_system
                .lines()
                .skip_while(|line| line.trim().is_empty())
                .collect::<Vec<&str>>()
                .join("\n")
                .trim()
                .to_string();

            if !instruction_text.is_empty() {
                debug!("🔧 Extracted instruction from end of prompt (first 200 chars): '{}'",
                    &instruction_text[..instruction_text.len().min(200)]);
                context.instruction = instruction_text;
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        // Fallback: check for [user] message pattern
        if let Some(user_line) = prompt.lines().find(|line| {
            line.starts_with("[user]") || line.contains("Message") && line.contains("[user]")
        }) {
            // Extract text after [user]
            if let Some(after_user) = user_line.split("[user]").nth(1) {
                let instruction_trimmed = after_user.trim();
                if !instruction_trimmed.is_empty() {
                    debug!("🔧 Extracted instruction from [user] message: '{}'", instruction_trimmed);
                    context.instruction = instruction_trimmed.to_string();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };

    // If no [user] message found, try traditional instruction markers
    if !has_user_message {
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
            debug!("🔧 Instruction extraction: no markers found, trying fallback");
            // Fallback: User input is typically at the END of the prompt (after all system instructions)
            // Look for the LAST substantial non-empty line as the user input
            let lines: Vec<&str> = prompt.lines().collect();
            let mut found = false;

            // Search from the end backwards to find the user input
            for line in lines.iter().rev() {
                let trimmed = line.trim();
                if !trimmed.is_empty()
                    && trimmed.len() > 5
                    && !trimmed.starts_with('#')
                    && !trimmed.starts_with('-')
                    && !trimmed.starts_with("You ")
                    && !trimmed.starts_with("Your ")
                    && !trimmed.starts_with("Please ")
                    && !trimmed.contains("```")
                    && !trimmed.starts_with("Use `/")
                    && !trimmed.ends_with(":")
                    && !trimmed.contains("CRITICAL:")
                    && !trimmed.contains("**")
                {
                    debug!("🔧 Extracted instruction from end of prompt: '{}'", trimmed);
                    context.instruction = trimmed.to_string();
                    found = true;
                    break;
                }
            }

            // If still not found, try the old forward search as last resort
            if !found {
                for line in lines.iter() {
                    let trimmed = line.trim();
                    let lower = trimmed.to_lowercase();
                    if !trimmed.is_empty()
                        && trimmed.len() > 5
                        && !trimmed.starts_with('#')
                        && !trimmed.starts_with("You ")
                        && !trimmed.starts_with("Your ")
                        && !trimmed.starts_with("Please ")
                        && !trimmed.contains("```")
                        && !trimmed.starts_with("Use `/")
                        && (lower.starts_with("listen")
                            || lower.starts_with("start")
                            || lower.starts_with("create")
                            || lower.starts_with("open")
                            || lower.starts_with("run")
                            || lower.starts_with("spawn")
                            || lower.starts_with("connect")
                        )
                    {
                        debug!("🔧 Extracted instruction (forward fallback): '{}'", trimmed);
                        context.instruction = trimmed.to_string();
                        found = true;
                        break;
                    }
                }
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
