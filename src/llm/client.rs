//! Ollama client for LLM communication

use std::collections::HashMap;
use std::fs::File;

use crate::llm::actions::{
    execute_tool, summarize_actions, ActionResponse, ToolAction, ToolResult,
};
use anyhow::{Context, Result};
use bytes::Bytes;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

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
}

/// RAII guard for Ollama lock file
///
/// Automatically releases the lock when dropped.
struct OllamaLockGuard {
    file: Option<File>,
}

impl Drop for OllamaLockGuard {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            // Unlock the file (errors are logged but not fatal)
            if let Err(e) = fs2::FileExt::unlock(&file) {
                error!("Failed to release Ollama lock: {}", e);
            } else {
                debug!("Ollama lock released");
            }
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

/// Chat API request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
}

/// Chat API response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: Message,
    #[allow(dead_code)]
    done: bool,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    prompt_eval_count: Option<u64>,
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
        instruction: String,
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

/// Ollama API client
#[derive(Clone)]
pub struct OllamaClient {
    base_url: String,
    client: reqwest::Client,
    /// Whether to use file locking for Ollama API access
    lock_enabled: bool,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::new_with_options(base_url, false)
    }

    /// Create a new Ollama client with options
    pub fn new_with_options(base_url: impl Into<String>, lock_enabled: bool) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            lock_enabled,
        }
    }

    /// Create a default client pointing to localhost
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        Self::new("http://localhost:11434")
    }

    /// Acquire the Ollama lock file if locking is enabled
    ///
    /// Returns a guard that will release the lock when dropped.
    /// The lock file is created at ./ollama.lock in the current directory.
    ///
    /// Implements stale lock detection: if the lock file is older than 30 seconds,
    /// it's assumed to be stale and the lock is forcibly acquired.
    fn acquire_ollama_lock(&self) -> Result<Option<OllamaLockGuard>> {
        if !self.lock_enabled {
            return Ok(None);
        }

        use std::fs::OpenOptions;
        use std::path::Path;
        use std::time::{Duration, SystemTime};

        let lock_path = Path::new("./ollama.lock");
        let stale_timeout = Duration::from_secs(30);

        debug!("Acquiring Ollama lock at {:?}", lock_path);

        // Open (or create) the lock file
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(lock_path)
            .context("Failed to open Ollama lock file")?;

        // Try to acquire lock with timeout/stale detection
        #[allow(clippy::never_loop)]
        let lock_acquired = loop {
            // Try non-blocking lock first
            match file.try_lock_exclusive() {
                Ok(()) => {
                    debug!("Ollama lock acquired immediately");
                    break true;
                }
                Err(_) => {
                    // Lock is held, check if it's stale
                    if let Ok(metadata) = file.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                                if elapsed > stale_timeout {
                                    warn!(
                                        "Ollama lock is stale ({}s old), breaking it",
                                        elapsed.as_secs()
                                    );

                                    // Force-acquire the lock (blocking if needed)
                                    // This is safe because we've determined the lock is stale
                                    file.lock_exclusive()
                                        .context("Failed to acquire exclusive lock after detecting stale lock")?;

                                    debug!("Ollama lock acquired after breaking stale lock");
                                    break true;
                                }
                            }
                        }
                    }

                    // Not stale yet, wait with blocking lock
                    debug!("Ollama lock is held, waiting...");
                    file.lock_exclusive()
                        .context("Failed to acquire exclusive lock on Ollama lock file")?;
                    break true;
                }
            }
        };

        if lock_acquired {
            // Update the lock file's modification time to mark it as active
            file.set_len(0)?; // Truncate
            use std::io::Write;
            writeln!(file, "{}", std::process::id())?; // Write our PID
            file.sync_all()?; // Ensure it's written

            debug!("Ollama lock acquired and updated");
            Ok(Some(OllamaLockGuard { file: Some(file) }))
        } else {
            Err(anyhow::anyhow!("Failed to acquire Ollama lock"))
        }
    }

    /// Generate a completion from the model with optional JSON schema
    pub async fn generate(&self, model: &str, prompt: &str) -> Result<String> {
        self.generate_with_format(model, prompt, None).await
    }

    /// Generate a completion with a specific JSON schema format
    pub async fn generate_with_format(
        &self,
        model: &str,
        prompt: &str,
        format: Option<serde_json::Value>,
    ) -> Result<String> {
        // Acquire Ollama lock if enabled (blocks until available)
        let _lock_guard = self.acquire_ollama_lock()?;
        let url = format!("{}/api/generate", self.base_url);

        debug!("Sending prompt to Ollama (model: {})", model);
        if let Some(ref schema) = format {
            debug!("Using structured output with JSON schema");
            debug!(
                "Schema: {}",
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
            );
        }
        debug!("Prompt: {}", prompt);

        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            stream: false,
            format,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Ollama request failed: {} - {}", status, text);
            anyhow::bail!("Ollama request failed: {} - {}", status, text);
        }

        let response: GenerateResponse = response
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        info!(
            "Received response from Ollama ({} tokens)",
            response.eval_count.unwrap_or(0)
        );
        debug!("Response: {}", response.response);

        Ok(response.response)
    }

    /// Chat completion using conversation messages
    ///
    /// Uses Ollama's /api/chat endpoint which supports conversation history.
    ///
    /// # Arguments
    /// * `model` - Model name (e.g., "qwen3-coder:30b")
    /// * `messages` - Conversation history with roles (system/user/assistant/tool)
    /// * `format` - Optional JSON schema for structured outputs
    ///
    /// # Returns
    /// * `Ok(String)` - The assistant's response content
    pub async fn chat(
        &self,
        model: &str,
        messages: Vec<Message>,
        format: Option<serde_json::Value>,
    ) -> Result<String> {
        // Acquire Ollama lock if enabled (blocks until available)
        let _lock_guard = self.acquire_ollama_lock()?;

        let url = format!("{}/api/chat", self.base_url);

        debug!(
            "Sending chat request to Ollama (model: {}, {} messages)",
            model,
            messages.len()
        );
        if let Some(ref schema) = format {
            debug!("Using structured output with JSON schema");
            debug!(
                "Schema: {}",
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
            );
        }
        for (i, msg) in messages.iter().enumerate() {
            debug!(
                "Message {}: [{}] {}",
                i + 1,
                msg.role,
                if msg.content.len() > 200 {
                    format!("{}...", &msg.content[..200])
                } else {
                    msg.content.clone()
                }
            );
        }

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            stream: false,
            format,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send chat request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Ollama chat request failed: {} - {}", status, text);
            anyhow::bail!("Ollama chat request failed: {} - {}", status, text);
        }

        let response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse Ollama chat response")?;

        info!(
            "Received chat response from Ollama ({} tokens)",
            response.eval_count.unwrap_or(0)
        );
        debug!("Response: {}", response.message.content);

        Ok(response.message.content)
    }

    /// Generate a structured LlmResponse for data/connection events
    pub async fn generate_llm_response(&self, model: &str, prompt: &str) -> Result<LlmResponse> {
        // NOTE: Structured outputs disabled due to Ollama compatibility issues
        // Ollama's JSON schema support is limited and doesn't handle:
        // - Complex union types (oneOf)
        // - Optional fields consistently
        // - Schema validation reliably across different models
        //
        // Using unstructured generation with JSON parsing as a more reliable approach
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Try to parse as JSON, with fallback to legacy format
        LlmResponse::from_str(&response_text)
    }

    /// Generate a structured HttpLlmResponse for HTTP requests
    pub async fn generate_http_response(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<HttpLlmResponse> {
        // NOTE: Structured outputs disabled - see generate_llm_response for details
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
        // NOTE: Structured outputs disabled - see generate_llm_response for details
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        CommandInterpretation::from_str(&response_text)
    }

    /// Generate action response with multi-turn tool calling support
    ///
    /// This method implements a loop where:
    /// 1. LLM returns actions (may include tool calls)
    /// 2. Regular actions are collected
    /// 3. Tool calls are executed
    /// 4. If tool calls exist, append results and call LLM again
    /// 5. Repeat until no tool calls or max iterations reached
    ///
    /// # Arguments
    /// * `model` - Model name
    /// * `prompt_builder` - Async function that builds prompts with iteration and tool results
    /// * `max_iterations` - Maximum number of LLM calls (default: 5)
    ///
    /// # Returns
    /// * `Vec<serde_json::Value>` - All non-tool actions collected across iterations
    pub async fn generate_with_tools<F, Fut>(
        &self,
        model: &str,
        prompt_builder: F,
        max_iterations: usize,
        approval_tx: Option<
            tokio::sync::mpsc::UnboundedSender<crate::state::app_state::WebApprovalRequest>,
        >,
        web_search_mode: crate::state::app_state::WebSearchMode,
    ) -> Result<Vec<serde_json::Value>>
    where
        F: Fn(usize, usize, Vec<ToolResult>) -> Fut,
        Fut: std::future::Future<Output = String>,
    {
        let mut all_actions = Vec::new();
        let mut tool_results = Vec::new();

        for iteration in 1..=max_iterations {
            // Build prompt for this iteration
            let prompt = prompt_builder(iteration, max_iterations, tool_results.clone()).await;

            // Generate response
            debug!("Multi-turn iteration {}/{}", iteration, max_iterations);
            let response_text = self.generate_with_format(model, &prompt, None).await?;

            // Parse as action response
            let action_response = ActionResponse::from_str(&response_text)
                .context("Failed to parse action response")?;

            // Log action summary
            let summary = summarize_actions(&action_response.actions);
            info!(
                "LLM response (iteration {}/{}): {}",
                iteration, max_iterations, summary
            );

            // Separate tool calls from regular actions
            let (tools, regular): (Vec<_>, Vec<_>) = action_response
                .actions
                .into_iter()
                .partition(ToolAction::is_tool_action);

            // Collect regular actions
            all_actions.extend(regular);

            // If no tool calls, we're done
            if tools.is_empty() {
                debug!("No tool calls in response, finishing multi-turn loop");
                break;
            }

            // If this is the last iteration, warn about unused tool calls
            if iteration == max_iterations {
                warn!(
                    "Maximum iterations reached with {} pending tool calls",
                    tools.len()
                );
                break;
            }

            // Execute tool calls
            debug!("Executing {} tool calls", tools.len());
            tool_results.clear();

            for tool_json in tools {
                match ToolAction::from_json(&tool_json) {
                    Ok(tool_action) => {
                        info!("→ Executing tool: {}", tool_action.describe());
                        let result =
                            execute_tool(&tool_action, approval_tx.as_ref(), web_search_mode, None)
                                .await;
                        info!("  Result: {}", result.summary());
                        tool_results.push(result);
                    }
                    Err(e) => {
                        error!("Failed to parse tool action: {}", e);
                        tool_results.push(ToolResult::error(
                            "unknown",
                            "parse_error",
                            format!("Failed to parse tool action: {}", e),
                        ));
                    }
                }
            }

            debug!(
                "Completed iteration {}/{}, {} tool results to include in next iteration",
                iteration,
                max_iterations,
                tool_results.len()
            );
        }

        info!(
            "Multi-turn generation complete: {} total actions collected",
            all_actions.len()
        );
        Ok(all_actions)
    }

    /// Check if Ollama is available
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to list models")?;

        let response: ListModelsResponse = response
            .json()
            .await
            .context("Failed to parse models list")?;

        Ok(response.models.into_iter().map(|m| m.name).collect())
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        OllamaClient::default()
    }
}

#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ListModelsResponse {
    models: Vec<Model>,
}

#[derive(Debug, Deserialize)]
struct Model {
    name: String,
}
