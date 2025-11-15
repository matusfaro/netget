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

        // Wait for server to be ready by making HTTP requests
        let max_retries = 20; // 20 retries * 50ms = 1 second max
        let mut ready = false;
        for attempt in 1..=max_retries {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            // Try to make an HTTP request to the server (GET /api/version)
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}/api/version", port);
            if client.get(&url).timeout(tokio::time::Duration::from_millis(100)).send().await.is_ok() {
                ready = true;
                info!("🔧 Mock Ollama server ready after {}ms", attempt * 50);
                break;
            }
        }

        if !ready {
            return Err(format!("Mock Ollama server failed to become ready on port {}", port).into());
        }

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
    eprintln!("  instruction: {}", &context.instruction[..context.instruction.floor_char_boundary(context.instruction.len().min(200))]);
    eprintln!("  prompt length: {} chars", request.prompt.len());
    eprintln!("  prompt first 500 chars: {}", &request.prompt[..request.prompt.floor_char_boundary(request.prompt.len().min(500))]);
    let preview_start = request.prompt.floor_char_boundary(request.prompt.len().saturating_sub(500));
    eprintln!("  prompt last 500 chars: {}", &request.prompt[preview_start..]);

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
    let mut is_event_prompt = false;
    if let Some(event_id_line) = prompt.lines().find(|line| line.contains("Event ID:")) {
        if let Some(event_id) = event_id_line.split("Event ID:").nth(1) {
            let event_id = event_id.trim();
            if !event_id.is_empty() {
                debug!("🔧 Extracted event type from Event ID: '{}'", event_id);
                context.event_type = Some(event_id.to_string());
                is_event_prompt = true;
            }
        }
    } else if let Some(event_line) = prompt.lines().find(|line| line.contains("Event:")) {
        // Fallback: try to extract from "Event:" line (legacy format)
        if let Some(event_type) = event_line.split("Event:").nth(1) {
            let event_type = event_type.trim().split_whitespace().next().unwrap_or("");
            if !event_type.is_empty() {
                debug!("🔧 Extracted event type from Event: '{}'", event_type);
                context.event_type = Some(event_type.to_string());
                is_event_prompt = true;
            }
        }
    }

    // Also detect event prompts by presence of "## Event-Specific Instructions"
    // (task execution prompts don't have "Event ID:" but do have this section)
    if !is_event_prompt && prompt.contains("## Event-Specific Instructions") {
        is_event_prompt = true;
        debug!("🔧 Detected event prompt from '## Event-Specific Instructions' section");
    }

    // Try to extract instruction - look for patterns
    // PRIORITY 0: For event prompts, extract from "## Global Instructions" or "## Event-Specific Instructions"
    let has_user_message = if is_event_prompt {
        // For events, look for instruction sections in the system message
        if let Some(global_inst_idx) = prompt.find("## Global Instructions") {
            let after_header = &prompt[global_inst_idx + "## Global Instructions".len()..];
            let lines: Vec<&str> = after_header.lines().collect();
            let mut found = false;
            for line in lines.iter() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.len() > 5 && !trimmed.starts_with('#') {
                    debug!("🔧 Extracted instruction from Global Instructions: '{}'", trimmed);
                    context.instruction = trimmed.to_string();
                    found = true;
                    break;
                }
            }
            found
        } else if let Some(event_inst_idx) = prompt.find("## Event-Specific Instructions") {
            let after_header = &prompt[event_inst_idx + "## Event-Specific Instructions".len()..];

            // Find the end of this section (next ## header or "## Network Event Instructions")
            let section_end = after_header.find("\n##")
                .or_else(|| after_header.find("## Network Event Instructions"))
                .unwrap_or(after_header.len());
            let section = &after_header[..section_end];

            // Collect all non-empty, non-header lines and join them
            let mut instruction_parts = Vec::new();
            for line in section.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    instruction_parts.push(trimmed);
                }
            }

            if !instruction_parts.is_empty() {
                let combined = instruction_parts.join("\n");
                debug!("🔧 Extracted instruction from Event-Specific Instructions: '{}'", combined);
                context.instruction = combined;
                true
            } else {
                false
            }
        } else {
            false
        }
    }
    // PRIORITY 1: User input appears at the END of the prompt (after system message)
    // The conversation structure is: [system message]\n\n[user input]\n\n
    // So we look for the last substantial line that looks like user input
    // First try: Look after "# Current State" section and find the last substantial line
    else if let Some(current_state_idx) = prompt.find("# Current State") {
        let after_current_state = &prompt[current_state_idx..];

        // Find lines after the last system capability
        if let Some(last_cap_idx) = after_current_state.rfind("- **Raw socket access**")
            .or_else(|| after_current_state.rfind("- **Privileged ports"))
        {
            let after_caps = &after_current_state[last_cap_idx..];

            // Split into lines and find the first substantial non-empty line
            let lines: Vec<&str> = after_caps.lines().collect();
            let mut found_instruction = false;

            // Search forward from the beginning to find the user input
            for line in lines.iter() {
                let trimmed = line.trim();
                // Look for substantial lines that don't look like system text
                if !trimmed.is_empty()
                    && trimmed.len() > 5
                    && !trimmed.starts_with('-')  // Not a bullet point
                    && !trimmed.starts_with('#')  // Not a header
                    && !trimmed.contains("**")    // Not bold formatting
                    && !trimmed.starts_with("✓")  // Not a checkmark
                    && !trimmed.starts_with("✗")  // Not an X mark
                {
                    debug!("🔧 Extracted instruction from end of prompt: '{}'", trimmed);
                    context.instruction = trimmed.to_string();
                    found_instruction = true;
                    break;
                }
            }
            found_instruction
        } else {
            false
        }
    }
    // PRIORITY 2: Look for System Capabilities section (older format)
    else if let Some(cap_idx) = prompt.find("## System Capabilities") {
        // Extract everything after the capabilities section
        let after_cap = &prompt[cap_idx..];
        // Find the end of the capabilities section (usually ends with "DataLink protocol unavailable" or similar)
        if let Some(end_idx) = after_cap.find("DataLink protocol unavailable") {
            let after_system = &after_cap[end_idx + "DataLink protocol unavailable".len()..];
            // Look for the first substantial non-empty line after the system section
            let lines: Vec<&str> = after_system.lines().collect();
            let mut found_instruction = false;
            for line in lines.iter() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.len() > 5 {
                    // Check if this is a "Trigger: User input:" or "User input:" line
                    if let Some(after_marker) = trimmed.strip_prefix("Trigger: User input:") {
                        let instruction = after_marker.trim().trim_matches('"').trim_matches('\'');
                        debug!("🔧 Extracted instruction from Trigger: User input: '{}'", instruction);
                        context.instruction = instruction.to_string();
                        found_instruction = true;
                        break;
                    } else if let Some(after_marker) = trimmed.strip_prefix("User input:") {
                        let instruction = after_marker.trim().trim_matches('"').trim_matches('\'');
                        debug!("🔧 Extracted instruction from User input: '{}'", instruction);
                        context.instruction = instruction.to_string();
                        found_instruction = true;
                        break;
                    } else {
                        debug!("🔧 Extracted instruction from end of prompt: '{}'", trimmed);
                        context.instruction = trimmed.to_string();
                        found_instruction = true;
                        break;
                    }
                }
            }
            found_instruction
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
