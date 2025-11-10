//! Ollama client using ollama-rs library for LLM communication

use std::collections::HashMap;

use crate::llm::actions::{
    execute_tool, summarize_actions, ActionResponse, ToolAction, ToolResult,
};
use anyhow::{Context, Result};
use bytes::Bytes;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::{ChatMessage, MessageRole};
use ollama_rs::Ollama;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Message in a conversation with role (system/user/assistant/tool)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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

    /// Convert to ollama-rs ChatMessage
    fn to_ollama_message(&self) -> ChatMessage {
        let role = match self.role.as_str() {
            "system" => MessageRole::System,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        };
        ChatMessage::new(role, self.content.clone())
    }
}

/// Structured response from the LLM
#[derive(Debug, Clone, Deserialize, Serialize)]
#[derive(Default)]
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
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: impl Into<String>) -> Self {
        let url_str = base_url.into();
        let ollama = Ollama::new(url_str.as_str(), 11434);
        Self {
            ollama,
            status_tx: None,
        }
    }

    /// Create a default client pointing to localhost
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        let ollama = Ollama::default();
        Self {
            ollama,
            status_tx: None,
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

    /// Generate a completion from the model with optional JSON schema
    ///
    /// IMPORTANT: This method is crate-private. Use `action_helper::call_llm_with_actions()`
    /// instead for all LLM calls. The action helper provides a unified interface with
    /// proper prompt building, response parsing, and action execution.
    ///
    /// Only use this directly in:
    /// - action_helper module (the primary consumer)
    /// - handler for user input commands
    pub(crate) async fn generate(&self, model: &str, prompt: &str) -> Result<String> {
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
    ) -> Result<String> {
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
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!("[TRACE] LLM prompt:\r\n{}", prompt.replace('\n', "\r\n")));
        }
        if let Some(ref schema) = format {
            trace!(
                "JSON schema:\n{}",
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
            );
            if let Some(ref tx) = self.status_tx {
                let schema_str = serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string());
                let _ = tx.send(format!(
                    "[TRACE] JSON schema:\r\n{}",
                    schema_str.replace('\n', "\r\n")
                ));
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

        let response = tokio::time::timeout(
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

        // DEBUG: Summary
        debug!(
            "LLM response: response_len={} chars",
            response.response.len()
        );
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(format!(
                "[DEBUG] LLM response: response_len={} chars",
                response.response.len()
            ));
        }

        // TRACE: Full payload with pretty-printed JSON if possible
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response.response) {
            let pretty = serde_json::to_string_pretty(&json).unwrap_or(response.response.clone());
            trace!("Full LLM response (JSON):\n{}", pretty);
            if let Some(ref tx) = self.status_tx {
                // Send each line separately to ensure proper formatting
                let _ = tx.send("[TRACE] LLM response (JSON):".to_string());
                for line in pretty.lines() {
                    let _ = tx.send(format!("[TRACE] {}", line));
                }
            }
        } else {
            trace!("Full LLM response (text):\n{}", response.response);
            if let Some(ref tx) = self.status_tx {
                let _ = tx.send("[TRACE] LLM response (text):".to_string());
                for line in response.response.lines() {
                    let _ = tx.send(format!("[TRACE] {}", line));
                }
            }
        }

        Ok(response.response)
    }

    /// Generate a structured LlmResponse for data/connection events
    pub async fn generate_llm_response(&self, model: &str, prompt: &str) -> Result<LlmResponse> {
        // Disabled structured outputs - ollama-rs FormatType::Json still causes issues
        // Using unstructured generation with JSON parsing
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the response with fallback
        serde_json::from_str::<LlmResponse>(&response_text)
            .or_else(|_| LlmResponse::from_str(&response_text))
            .context("Failed to parse LLM response")
    }

    /// Generate a structured HttpLlmResponse for HTTP requests
    pub async fn generate_http_response(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<HttpLlmResponse> {
        // Disabled structured outputs - see generate_llm_response
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        HttpLlmResponse::from_str(&response_text)
    }

    /// Generate a structured CommandInterpretation for user commands
    pub async fn generate_command_interpretation(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<CommandInterpretation> {
        // Disabled structured outputs - see generate_llm_response
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        CommandInterpretation::from_str(&response_text)
    }

    /// Generate action response with multi-turn tool calling support using message-based conversation
    ///
    /// This method implements a message-based conversation loop:
    /// 1. Build initial prompt (system + user message)
    /// 2. LLM responds with actions (may include tool calls)
    /// 3. Regular actions are collected
    /// 4. Tool calls are executed
    /// 5. Tool results appended to conversation history
    /// 6. Repeat with full conversation context (no iteration numbers)
    ///
    /// # Arguments
    /// * `model` - Model name
    /// * `initial_prompt_builder` - Function that builds the initial system+user prompt
    /// * `max_iterations` - Maximum number of LLM calls (default: 5)
    /// * `approval_tx` - Channel for sending web approval requests to UI (optional)
    /// * `web_search_mode` - Current web search mode (On/Off/Ask)
    ///
    /// # Returns
    ///
    /// * `Vec<serde_json::Value>` - All non-tool actions collected across conversation turns
    ///
    /// Chat completion using conversation messages
    ///
    /// Uses ollama-rs chat API which supports conversation history.
    ///
    /// # Arguments
    /// * `model` - Model name (e.g., "qwen3-coder:30b")
    /// * `messages` - Conversation history with roles (system/user/assistant/tool)
    /// * `format` - Optional JSON schema for structured outputs (not currently supported via ollama-rs)
    ///
    /// # Returns
    /// * `Ok(String)` - The assistant's response content
    pub async fn chat(
        &self,
        model: &str,
        messages: Vec<Message>,
        _format: Option<serde_json::Value>,
    ) -> Result<String> {
        debug!("Sending chat request (model: {}, {} messages)", model, messages.len());
        for (i, msg) in messages.iter().enumerate() {
            debug!("Message {}: [{}] {}",  i + 1, msg.role,
                if msg.content.len() > 200 {
                    format!("{}...", &msg.content[..200])
                } else {
                    msg.content.clone()
                }
            );
        }

        // Convert our messages to ollama-rs ChatMessage format
        let ollama_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| m.to_ollama_message())
            .collect();

        // Create chat request
        let request = ChatMessageRequest::new(model.to_string(), ollama_messages);

        // Send request with timeout (2 minutes for large models)
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            self.ollama.send_chat_messages(request),
        )
        .await
        .context("Ollama API call timed out after 120 seconds - check if Ollama is running and model is loaded")?
        .context("Failed to send chat request to Ollama")?;

        let content = response.message.content;

        debug!("Received chat response from Ollama ({} chars)", content.len());
        trace!("Full response: {}", content);

        Ok(content)
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
    ///
    /// # Returns
    /// * `Ok(String)` - The LLM response (may be after retry)
    pub async fn generate_with_retry(
        &self,
        model: &str,
        prompt: &str,
        expected_format: &str,
    ) -> Result<String> {
        const MAX_RETRIES: usize = 1;

        for attempt in 1..=MAX_RETRIES + 1 {
            debug!("Generate attempt {}/{}", attempt, MAX_RETRIES + 1);

            // Generate response
            let response = self.generate(model, prompt).await?;

            // Try to parse as ActionResponse to check format
            match ActionResponse::from_str(&response) {
                Ok(_) => {
                    // Valid format!
                    if attempt > 1 {
                        info!("Retry successful on attempt {}", attempt);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    if attempt <= MAX_RETRIES {
                        // We have retries left
                        warn!("Parse error on attempt {}: {}", attempt, e);
                        warn!(
                            "Malformed response: {}",
                            if response.len() > 500 {
                                format!("{}...", &response[..500])
                            } else {
                                response.clone()
                            }
                        );

                        // Build retry prompt with correction
                        let retry_prompt = format!(
                            "{}\n\n---\n\nYour previous response was invalid and could not be parsed.\n\nError: {}\n\nRequired format: {}\n\nPlease provide your response again in the correct format.",
                            prompt,
                            e,
                            expected_format
                        );

                        // Try again with corrected prompt (will happen in next loop iteration)
                        // Update the prompt variable for next attempt
                        if attempt < MAX_RETRIES + 1 {
                            info!("Retrying with corrective feedback...");
                            // Recursive call with corrected prompt (needs Box::pin for recursion)
                            return Box::pin(self.generate_with_retry(model, &retry_prompt, expected_format)).await;
                        }
                    } else {
                        // No more retries
                        error!("Failed to get valid response after {} attempts", attempt);
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
        approval_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::state::app_state::WebApprovalRequest>>,
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
        trace!("New messages:\n{}", &conversation_history[last_logged_position..]);
        last_logged_position = conversation_history.len();

        for turn in 1..=max_iterations {
            // Generate response with full conversation history
            debug!("Conversation turn {}/{}", turn, max_iterations);

            let response_text = self
                .generate_with_format(model, &conversation_history, None)
                .await?;

            // Parse as action response
            let action_response = ActionResponse::from_str(&response_text)
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
            conversation_history.push_str(&response_text);

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
                        let result = execute_tool(&tool_action, approval_tx.as_ref(), web_search_mode, None).await;
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
            trace!("New messages:\n{}", &conversation_history[last_logged_position..]);
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
