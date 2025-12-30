//! Ollama client using ollama-rs library for LLM communication

use std::collections::HashMap;

use crate::llm::actions::{
    execute_tool, summarize_actions, ActionResponse, ToolAction, ToolResult,
};
use anyhow::{Context, Result};
use bytes::Bytes;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Strip markdown code fences (```json ... ``` or ``` ... ```) from text
/// This helps handle cases where the LLM wraps JSON responses in markdown formatting
/// Handles multiple trailing backticks (e.g., ```\n```)
fn strip_markdown_fences(text: &str) -> String {
    let mut result = text.trim();

    // Remove opening fence (```json or ```)
    result = result
        .strip_prefix("```json")
        .or_else(|| result.strip_prefix("```"))
        .unwrap_or(result)
        .trim();

    // Remove ALL trailing backticks (loop until no more)
    // Handles cases like: {...}\n```\n```
    while let Some(stripped) = result.strip_suffix("```") {
        result = stripped.trim();
    }

    result.to_string()
}

/// Message in a conversation with role (system/user/assistant/tool)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Token usage statistics from an LLM response
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    /// Number of tokens in the prompt (input)
    pub prompt_tokens: u64,
    /// Number of tokens in the response (output)
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion)
    pub total_tokens: u64,
}

impl TokenUsage {
    /// Create from ollama-rs GenerationResponse
    pub fn from_response(response: &ollama_rs::generation::completion::GenerationResponse) -> Self {
        let prompt_tokens = response.prompt_eval_count.unwrap_or(0) as u64;
        let completion_tokens = response.eval_count.unwrap_or(0) as u64;

        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

/// Response from generate_with_format including token usage
#[derive(Debug, Clone)]
pub struct GenerateResponse {
    /// The generated text
    pub text: String,
    /// Token usage statistics
    pub token_usage: TokenUsage,
}

impl Message {
    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    /// Create a tool message
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
        }
    }
}

/// Structured response from the LLM
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LlmResponse {
    /// Data to send over the connection (None = no output)
    #[serde(default)]
    pub output: Option<String>,

    /// Whether to close this specific connection
    #[serde(default)]
    pub close_connection: bool,

    /// Whether to wait for more data before responding
    #[serde(default)]
    pub wait_for_more: bool,

    /// Whether to shut down the entire server
    #[serde(default)]
    pub shutdown_server: bool,

    /// Optional log message for debugging
    #[serde(default)]
    pub log_message: Option<String>,

    /// Update memory - completely replace existing memory
    #[serde(default)]
    pub set_memory: Option<String>,

    /// Append to memory (added to end with newline separator)
    #[serde(default)]
    pub append_memory: Option<String>,
}

impl LlmResponse {
    /// Parse from JSON string with fallback to legacy text format
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();

        // Try to parse as JSON first
        if let Ok(response) = serde_json::from_str::<LlmResponse>(trimmed) {
            return Ok(response);
        }

        // Fallback: handle legacy text responses
        match trimmed {
            "NO_RESPONSE" => Ok(Self::default()),
            "CLOSE_CONNECTION" => Ok(Self {
                close_connection: true,
                ..Default::default()
            }),
            "WAIT_FOR_MORE" => Ok(Self {
                wait_for_more: true,
                ..Default::default()
            }),
            _ => {
                // Treat as raw output text
                Ok(Self {
                    output: Some(trimmed.to_string()),
                    ..Default::default()
                })
            }
        }
    }
}

/// Structured HTTP response from the LLM
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpLlmResponse {
    /// HTTP status code
    pub status: u16,

    /// Response headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Response body
    pub body: String,

    /// Optional log message for debugging
    #[serde(default)]
    pub log_message: Option<String>,

    /// Update memory - completely replace existing memory
    #[serde(default)]
    pub set_memory: Option<String>,

    /// Append to memory (added to end with newline separator)
    #[serde(default)]
    pub append_memory: Option<String>,
}

impl Default for HttpLlmResponse {
    fn default() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: String::new(),
            log_message: None,
            set_memory: None,
            append_memory: None,
        }
    }
}

impl std::str::FromStr for LlmResponse {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LlmResponse::from_str(s)
    }
}

impl HttpLlmResponse {
    /// Parse from JSON string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        serde_json::from_str::<HttpLlmResponse>(trimmed)
            .context("Failed to parse HTTP LLM response as JSON")
    }

    /// Convert to event HttpResponse
    pub fn to_event_response(self) -> crate::events::types::HttpResponse {
        crate::events::types::HttpResponse {
            status: self.status,
            headers: self.headers,
            body: Bytes::from(self.body),
        }
    }
}

impl std::str::FromStr for HttpLlmResponse {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HttpLlmResponse::from_str(s)
    }
}

/// Action types for command interpretation
///
/// WARNING: If you modify this enum, you MUST also update the corresponding
/// The schema file is used for Ollama's structured output feature.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandAction {
    UpdateInstruction {
        instruction: String,
    },
    OpenServer {
        port: u16,
        base_stack: String,
        #[serde(default)]
        send_first: bool,
        #[serde(default)]
        initial_memory: Option<String>,
        /// The instruction prompt for handling network events
        instruction: String,
    },
    CloseServer {
        #[serde(default)]
        server_id: Option<u32>,
    },
    OpenClient {
        address: String,
        base_stack: String,
    },
    CloseConnection {
        #[serde(default)]
        connection_id: Option<String>,
    },
    ShowMessage {
        message: String,
    },
    ChangeModel {
        model: String,
    },
}

/// Structured response for command interpretation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandInterpretation {
    /// List of actions to take
    #[serde(default)]
    pub actions: Vec<CommandAction>,

    /// Optional message to display to user
    #[serde(default)]
    pub message: Option<String>,
}

impl CommandInterpretation {
    /// Parse from JSON string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        serde_json::from_str::<CommandInterpretation>(trimmed)
            .context("Failed to parse command interpretation as JSON")
    }
}

impl std::str::FromStr for CommandInterpretation {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CommandInterpretation::from_str(s)
    }
}

/// Ollama API client using ollama-rs
#[derive(Clone)]
pub struct OllamaClient {
    ollama: Ollama,
    status_tx: Option<mpsc::UnboundedSender<String>>,
    mock_config_file: Option<std::path::PathBuf>,
    app_state: Option<crate::state::AppState>,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: impl Into<String>) -> Self {
        let url_str = base_url.into();

        // Parse the URL to extract host and port
        // URL format: "http://host:port" or "http://host" (default port 11434)
        let (host, port) = if let Some(port_start) = url_str.rfind(':') {
            // Check if this is a port (not the :// in http://)
            if port_start > 6 && !url_str[port_start..].contains('/') {
                // Extract port number
                if let Ok(port_num) = url_str[port_start + 1..].parse::<u16>() {
                    // Valid port found - split host and port
                    (&url_str[..port_start], port_num)
                } else {
                    // Invalid port - use whole URL as host with default port
                    (url_str.as_str(), 11434)
                }
            } else {
                // No port in URL - use default
                (url_str.as_str(), 11434)
            }
        } else {
            // No colon found - use default port
            (url_str.as_str(), 11434)
        };

        let ollama = Ollama::new(host, port);
        Self {
            ollama,
            status_tx: None,
            mock_config_file: None,
            app_state: None,
        }
    }

    /// Create a default client pointing to localhost
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        let ollama = Ollama::default();
        Self {
            ollama,
            status_tx: None,
            mock_config_file: None,
            app_state: None,
        }
    }

    /// Create a new Ollama client with options (lock_enabled is ignored, maintained for compatibility)
    pub fn new_with_options(base_url: impl Into<String>, _lock_enabled: bool) -> Self {
        // Note: lock_enabled is ignored here as locking is handled at a different layer
        Self::new(base_url)
    }

    /// Set the status channel for sending trace logs to TUI
    pub fn with_status_tx(mut self, status_tx: mpsc::UnboundedSender<String>) -> Self {
        self.status_tx = Some(status_tx);
        self
    }

    /// Set the mock configuration file path (for testing)
    pub fn with_mock_config_file(mut self, path: Option<std::path::PathBuf>) -> Self {
        self.mock_config_file = path;
        self
    }

    /// Set the app state for token tracking
    pub fn with_app_state(mut self, state: crate::state::AppState) -> Self {
        self.app_state = Some(state);
        self
    }

    /// Generate a completion from the model with optional JSON schema
    ///
    /// IMPORTANT: This method is crate-private. Use `action_helper::call_llm_with_actions()`
    /// instead for all LLM calls. The action helper provides a unified interface with
    /// proper prompt building, response parsing, and action execution.
    ///
    /// Only use this directly in:
    /// - action_helper module (the primary consumer)
    /// - handler for user input commands
    pub(crate) async fn generate(&self, model: &str, prompt: &str) -> Result<GenerateResponse> {
        self.generate_with_format(model, prompt, None).await
    }

    /// Generate a completion with a specific JSON schema format
    ///
    /// IMPORTANT: This method is crate-private. Use `action_helper::call_llm_with_actions()`
    /// for network event handling, or the specialized methods like generate_command_interpretation()
    /// for user command interpretation.
    pub(crate) async fn generate_with_format(
        &self,
        model: &str,
        prompt: &str,
        format: Option<serde_json::Value>,
    ) -> Result<GenerateResponse> {
        // DEBUG: Summary
        debug!(
            "LLM request: model={}, prompt_len={} chars, format={}",
            model,
            prompt.len(),
            if format.is_some() { "JSON" } else { "text" }
        );
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!(
                "[DEBUG] LLM request: model={}, prompt_len={} chars",
                model,
                prompt.len()
            ));
        }

        // TRACE: Full payload
        trace!("Full LLM prompt:\n{}", prompt);
        // Disable prompt logging, it's too much
        // if let Some(ref tx) = self.status_tx {
        //     let _ = tx.send("[TRACE] LLM prompt:".to_string());
        //     for line in crate::llm::format_indented_dimmed_lines(prompt, 8) {
        //         let _ = tx.send(format!("[TRACE] {}", line));
        //     }
        // }
        if let Some(ref schema) = format {
            trace!(
                "JSON schema:\n{}",
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
            );
            if let Some(ref tx) = self.status_tx {
                let schema_str =
                    serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string());
                let _ = tx.send("[TRACE] JSON schema:".to_string());
                for line in crate::llm::format_indented_dimmed_lines(&schema_str, 8) {
                    let _ = tx.send(format!("[TRACE] {}", line));
                }
            }
        }

        let mut request = GenerationRequest::new(model.to_string(), prompt.to_string());

        // Set num_predict to allow longer responses (especially for binary protocol data)
        use ollama_rs::models::ModelOptions;
        let options = ModelOptions::default().num_predict(2048); // Allow up to 2048 tokens
        request = request.options(options);

        // Add format if provided
        if let Some(_schema) = format {
            // For now, use plain JSON format since we need to handle structured JSON differently
            // The ollama-rs StructuredJson format requires a schemars Schema type
            // We'll use the simpler JSON format and rely on prompt engineering
            use ollama_rs::generation::parameters::FormatType;
            request = request.format(FormatType::Json);
        }

        let api_response = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            self.ollama.generate(request),
        )
        .await
        .context("Ollama API call timed out after 120 seconds.\n   Please check:\n   1. Ollama is running (https://ollama.ai)\n   2. Model is loaded and ready\n   3. Use `/model` to list and select a model")?
        .map_err(|e| {
            // Check if it's a connection error
            let error_str = e.to_string().to_lowercase();
            if error_str.contains("connection") || error_str.contains("refused") || error_str.contains("connect") {
                anyhow::anyhow!(
                    "✗  Cannot connect to Ollama.\n   Please ensure:\n   1. Ollama is running: https://ollama.ai\n   2. Ollama is listening on http://localhost:11434\n   3. Use `/model` command to list and select a model\n\n   Original error: {}", e
                )
            } else if error_str.contains("not found") || error_str.contains("404") {
                anyhow::anyhow!(
                    "✗  Model not found in Ollama.\n   Please:\n   1. Pull the model: ollama pull {}\n   2. Or use `/model` to select a different model\n\n   Original error: {}", model, e
                )
            } else {
                anyhow::anyhow!("✗  Ollama request failed: {}\n   Use `/model` to check available models", e)
            }
        })?;

        // Extract token usage
        let token_usage = TokenUsage::from_response(&api_response);

        // Record tokens in app state if available (for /usage command)
        if let Some(ref state) = self.app_state {
            state.record_llm_tokens(token_usage.prompt_tokens, token_usage.completion_tokens).await;
        }

        // DEBUG: Summary with token info
        debug!(
            "LLM response: response_len={} chars, tokens={}i/{}o/{}t",
            api_response.response.len(),
            token_usage.prompt_tokens,
            token_usage.completion_tokens,
            token_usage.total_tokens
        );
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!(
                "[DEBUG] LLM response: response_len={} chars, tokens={} prompt + {} completion = {} total",
                api_response.response.len(),
                token_usage.prompt_tokens,
                token_usage.completion_tokens,
                token_usage.total_tokens
            ));
        }

        // Check for empty response (model may be incompatible with JSON format)
        if api_response.response.is_empty() || api_response.response.trim().is_empty() {
            let error_msg = format!(
                "Model '{}' returned empty response (used {} completion tokens).",
                model,
                token_usage.completion_tokens
            );
            error!("{}", error_msg);
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send(format!("[ERROR] {}", error_msg));
            }
            return Err(anyhow::anyhow!(error_msg));
        }

        // TRACE: Full payload with pretty-printed JSON if possible
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&api_response.response) {
            let pretty = serde_json::to_string_pretty(&json).unwrap_or(api_response.response.clone());
            trace!("Full LLM response (JSON):\n{}", pretty);
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send("[TRACE] LLM response (JSON):".to_string());
                for line in crate::llm::format_indented_dimmed_lines(&pretty, 8) {
                    let _ = tx.send(format!("[TRACE] {}", line));
                }
            }
        } else {
            trace!("Full LLM response (text):\n{}", api_response.response);
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send("[TRACE] LLM response (text):".to_string());
                for line in crate::llm::format_indented_dimmed_lines(&api_response.response, 8) {
                    let _ = tx.send(format!("[TRACE] {}", line));
                }
            }
        }

        Ok(GenerateResponse {
            text: api_response.response,
            token_usage,
        })
    }

    /// Generate with automatic retry on parse errors (for legacy protocols)
    ///
    /// This is a simpler retry wrapper for protocols that don't use the action system.
    /// It retries if the response doesn't parse as ActionResponse.
    ///
    /// # Arguments
    /// * `model` - Model name
    /// * `prompt` - The prompt string
    /// * `expected_format` - Description of expected format for error message
    /// * `max_retries` - Maximum number of retries (0 = no retries, just one attempt)
    ///
    /// # Returns
    /// * `Ok(String)` - The LLM response (may be after retry)
    pub async fn generate_with_retry(
        &self,
        model: &str,
        prompt: &str,
        expected_format: &str,
        max_retries: usize,
    ) -> Result<String> {
        // Make prompt owned so we can update it for retries
        let mut current_prompt = prompt.to_string();

        for attempt in 1..=max_retries + 1 {
            debug!("Generate attempt {}/{}", attempt, max_retries + 1);
            trace!("Retry loop: attempt={}, prompt_len={}", attempt, current_prompt.len());

            // Generate response
            let generate_response = self.generate(model, &current_prompt).await?;

            // Extract XML references BEFORE validating JSON format
            // This allows LLM to use <script001> tags without causing "trailing characters" errors
            let (json_only, _refs) = crate::llm::reference_parser::extract_references(&generate_response.text)
                .unwrap_or_else(|_| (generate_response.text.clone(), std::collections::HashMap::new()));

            // Strip markdown code fences if present (```json ... ``` or ``` ... ```)
            // LLMs sometimes wrap JSON in markdown formatting which causes parse errors
            let json_cleaned = strip_markdown_fences(&json_only);

            // Try to parse cleaned JSON as ActionResponse to check format
            match ActionResponse::from_str(&json_cleaned) {
                Ok(_) => {
                    // Valid format!
                    if attempt > 1 {
                        info!("Retry successful on attempt {}", attempt);
                    }
                    trace!("Parse succeeded on attempt {}", attempt);
                    return Ok(generate_response.text);
                }
                Err(e) => {
                    if attempt <= max_retries {
                        // We have retries left
                        warn!("Parse error on attempt {}: {}", attempt, e);
                        warn!(
                            "Malformed response (after XML extraction and markdown stripping): {}",
                            if json_cleaned.len() > 500 {
                                format!("{}...", &json_cleaned[..500])
                            } else {
                                json_cleaned.to_string()
                            }
                        );
                        trace!("Will retry with corrective feedback (attempt {}/{})", attempt, max_retries + 1);

                        // Build retry prompt with correction
                        current_prompt = format!(
                            "{}\n\n---\n\nYour previous response was invalid and could not be parsed.\n\nError: {}\n\nRequired format: {}\n\nPlease provide your response again in the correct format.",
                            current_prompt,
                            e,
                            expected_format
                        );

                        info!("Retrying with corrective feedback (attempt {}/{})", attempt + 1, max_retries + 1);
                        // Continue to next loop iteration with updated prompt
                    } else {
                        // No more retries
                        error!("Failed to get valid response after {} attempts", attempt);
                        trace!("Max retries exhausted, returning error");
                        return Err(e).context("LLM failed to provide valid format after retry");
                    }
                }
            }
        }

        unreachable!("Loop should always return or error")
    }

    pub async fn generate_with_tools<F, Fut>(
        &self,
        model: &str,
        initial_prompt_builder: F,
        max_iterations: usize,
        approval_tx: Option<
            tokio::sync::mpsc::UnboundedSender<crate::state::app_state::WebApprovalRequest>,
        >,
        web_search_mode: crate::state::app_state::WebSearchMode,
    ) -> Result<Vec<serde_json::Value>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = String>,
    {
        let mut all_actions = Vec::new();
        let mut conversation_history = String::new();
        let mut last_logged_position = 0; // Track where we last logged

        // Build initial prompt (system + user message)
        let initial_prompt = initial_prompt_builder().await;
        conversation_history.push_str(&initial_prompt);

        trace!(
            "Initial conversation setup: {} chars",
            conversation_history.len()
        );
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!(
                "[TRACE] Initial conversation: {} chars",
                conversation_history.len()
            ));
        }

        // Log initial messages
        trace!(
            "New messages:\n{}",
            &conversation_history[last_logged_position..]
        );
        last_logged_position = conversation_history.len();

        for turn in 1..=max_iterations {
            // Generate response with full conversation history
            debug!("Conversation turn {}/{}", turn, max_iterations);

            let generate_response = self
                .generate_with_format(model, &conversation_history, None)
                .await?;

            let response_text = &generate_response.text;

            // Parse as action response
            let action_response = ActionResponse::from_str(response_text)
                .context("Failed to parse action response")?;

            // Log action summary
            let summary = summarize_actions(&action_response.actions);
            info!("LLM response (turn {}): {}", turn, summary);
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send(format!("[INFO] LLM response (turn {}): {}", turn, summary));
            }

            // Separate tool calls from regular actions
            let (tools, regular): (Vec<_>, Vec<_>) = action_response
                .actions
                .into_iter()
                .partition(ToolAction::is_tool_action);

            // Collect regular actions
            all_actions.extend(regular);

            // If no tool calls, we're done
            if tools.is_empty() {
                debug!("No tool calls in response, conversation complete");
                break;
            }

            // If this is the last iteration, warn about unused tool calls
            if turn == max_iterations {
                warn!(
                    "Maximum iterations reached with {} pending tool calls",
                    tools.len()
                );
                if let Some(ref tx) = self.status_tx {
                    let _ = tx.send(format!(
                        "[WARN] Maximum iterations reached with {} pending tool calls",
                        tools.len()
                    ));
                }
                break;
            }

            // Append assistant's tool call actions to conversation
            conversation_history.push_str("\n\n--- Assistant Response ---\n");
            conversation_history.push_str(response_text);

            // Execute tool calls and append results to conversation
            conversation_history.push_str("\n\n--- Tool Results ---\n");

            for tool_json in tools {
                match ToolAction::from_json(&tool_json) {
                    Ok(tool_action) => {
                        info!("→ Executing tool: {}", tool_action.describe());
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!(
                                "[INFO] → Executing tool: {}",
                                tool_action.describe()
                            ));
                        }
                        let result =
                            execute_tool(&tool_action, approval_tx.as_ref(), web_search_mode, None)
                                .await;
                        info!("  Result: {}", result.summary());
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!("[INFO]   Result: {}", result.summary()));
                        }

                        // Append tool result to conversation
                        conversation_history.push_str(&format!("\n{}\n", result.to_prompt_text()));
                    }
                    Err(e) => {
                        error!("Failed to parse tool action: {}", e);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!("[ERROR] Failed to parse tool action: {}", e));
                        }

                        let error_result = ToolResult::error(
                            "unknown",
                            "parse_error",
                            format!("Failed to parse tool action: {}", e),
                        );
                        conversation_history
                            .push_str(&format!("\n{}\n", error_result.to_prompt_text()));
                    }
                }
            }

            // Add reminder to complete the original request using the tool results
            conversation_history.push_str("\nNow that you have the tool results, use the information to COMPLETE the original request.\n");
            conversation_history.push_str("If the user asked you to extract information, use show_message to report what you found.\n");
            conversation_history.push_str("If the user asked you to perform a task, execute the appropriate actions to finish it.\n");
            conversation_history.push_str("\nIMPORTANT: Return ONLY valid JSON with no extra text or characters before or after.\n");
            conversation_history.push_str("RESPONSE FORMAT: {{\"actions\": [...]}}\n");

            let conv_size = conversation_history.len();

            // Log only new messages since last checkpoint
            trace!(
                "New messages:\n{}",
                &conversation_history[last_logged_position..]
            );
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send(format!(
                    "[TRACE] Conversation updated: {} chars (added {} new chars)",
                    conv_size,
                    conv_size - last_logged_position
                ));
            }
            last_logged_position = conv_size;

            // Performance warning if conversation is getting large
            if conv_size > 50_000 {
                warn!("Conversation history is large: {} chars ({:.1} KB) - consider reducing max_iterations",
                    conv_size, conv_size as f64 / 1024.0);
                if let Some(ref tx) = self.status_tx {
                    let _ = tx.send(format!(
                        "[WARN] ⚠ Large conversation: {:.1} KB",
                        conv_size as f64 / 1024.0
                    ));
                }
            } else if conv_size > 20_000 {
                debug!(
                    "Conversation size: {} chars ({:.1} KB)",
                    conv_size,
                    conv_size as f64 / 1024.0
                );
            }

            debug!(
                "Completed turn {}/{}, continuing conversation with tool results",
                turn, max_iterations
            );
        }

        let final_size = conversation_history.len();
        info!(
            "Multi-turn conversation complete: {} actions, final size: {} chars ({:.1} KB)",
            all_actions.len(),
            final_size,
            final_size as f64 / 1024.0
        );
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!(
                "[INFO] ✓ Conversation complete: {} actions, {:.1} KB history",
                all_actions.len(),
                final_size as f64 / 1024.0
            ));
        }
        Ok(all_actions)
    }

    /// Check if Ollama is available
    pub async fn is_available(&self) -> bool {
        // Try to list models as a health check
        self.list_models().await.is_ok()
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let models = self
            .ollama
            .list_local_models()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list models: {}", e))?;

        Ok(models.into_iter().map(|m| m.name).collect())
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        OllamaClient::default()
    }
}
