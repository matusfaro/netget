//! LLM client supporting Ollama and OpenAI-compatible endpoints

use std::collections::HashMap;

use crate::llm::actions::ActionResponse;
use crate::llm::circuit_breaker::{is_transport_failure, BreakerStatus, CircuitBreaker};
use crate::logging::emit::Log;
use anyhow::{Context, Result};
use bytes::Bytes;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Internal backend implementation for LLM communication
#[derive(Clone)]
enum LlmBackend {
    /// Ollama HTTP API via ollama-rs
    Ollama(Ollama),
    /// OpenAI-compatible API via reqwest
    OpenAI {
        client: reqwest::Client,
        base_url: String,
        api_key: String,
    },
    /// No model: enqueue each request and await an answer from the calling MCP
    /// agent (see `crate::llm::agent_queue`). MCP `--llm-agent` mode only.
    Queue {
        queue: std::sync::Arc<crate::llm::agent_queue::LlmRequestQueue>,
        /// How long to wait for the agent's answer before erroring.
        timeout: std::time::Duration,
    },
}

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
    /// Tool call ID for tool result messages (role="tool")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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

/// Request for chat_with_tools - structured messages with native tool definitions
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Conversation messages (system, user, assistant, tool)
    pub messages: Vec<Message>,
    /// Tool schemas in OpenAI/Ollama format (from ActionDefinition::to_tool_schema())
    pub tools: Vec<serde_json::Value>,
    /// Model name (e.g., "qwen3-coder:30b")
    pub model: String,
}

/// Response from chat_with_tools - may contain text and/or tool calls
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// Text content of the response (may be None if only tool calls)
    pub content: Option<String>,
    /// Native tool calls requested by the LLM
    pub tool_calls: Vec<ToolCall>,
    /// Token usage statistics
    pub token_usage: TokenUsage,
}

/// A native tool call from the LLM
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Unique ID for this tool call (used to match tool results)
    pub id: String,
    /// Name of the function/tool to call
    pub function_name: String,
    /// Arguments as a JSON object
    pub arguments: serde_json::Value,
}

impl Message {
    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// Create a tool message (legacy, without tool_call_id)
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// Create a tool result message with tool_call_id for native tool calling
    pub fn tool_result(tool_call_id: String, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_call_id: Some(tool_call_id),
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

/// Default per-request wall-clock bound for a backend call.
pub const DEFAULT_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// LLM API client supporting Ollama and OpenAI-compatible backends
#[derive(Clone)]
pub struct OllamaClient {
    backend: LlmBackend,
    status_tx: Option<mpsc::UnboundedSender<String>>,
    mock_config_file: Option<std::path::PathBuf>,
    app_state: Option<crate::state::AppState>,
    /// Shared across clones, so every caller sees the same backend health.
    breaker: std::sync::Arc<CircuitBreaker>,
    /// Wall-clock bound on a single backend call.
    request_timeout: std::time::Duration,
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
            backend: LlmBackend::Ollama(ollama),
            status_tx: None,
            mock_config_file: None,
            app_state: None,
            breaker: std::sync::Arc::new(CircuitBreaker::default()),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Create a default client pointing to localhost
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        let ollama = Ollama::default();
        Self {
            backend: LlmBackend::Ollama(ollama),
            status_tx: None,
            mock_config_file: None,
            app_state: None,
            breaker: std::sync::Arc::new(CircuitBreaker::default()),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Create a new Ollama client with options (lock_enabled is ignored, maintained for compatibility)
    pub fn new_with_options(base_url: impl Into<String>, _lock_enabled: bool) -> Self {
        // Note: lock_enabled is ignored here as locking is handled at a different layer
        Self::new(base_url)
    }

    /// Create a new client for an OpenAI-compatible API endpoint
    pub fn new_openai(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            backend: LlmBackend::OpenAI {
                client,
                base_url: base_url.into().trim_end_matches('/').to_string(),
                api_key: api_key.into(),
            },
            status_tx: None,
            mock_config_file: None,
            app_state: None,
            breaker: std::sync::Arc::new(CircuitBreaker::default()),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Create a client that queues LLM requests for the calling MCP agent to answer.
    /// No model is contacted; each request waits up to `timeout` for an answer.
    pub fn new_queue(
        queue: std::sync::Arc<crate::llm::agent_queue::LlmRequestQueue>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            backend: LlmBackend::Queue { queue, timeout },
            status_tx: None,
            mock_config_file: None,
            app_state: None,
            breaker: std::sync::Arc::new(CircuitBreaker::default()),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Returns the backend type as a string ("ollama", "openai", or "agent")
    pub fn backend_type(&self) -> &str {
        match &self.backend {
            LlmBackend::Ollama(_) => "ollama",
            LlmBackend::OpenAI { .. } => "openai",
            LlmBackend::Queue { .. } => "agent",
        }
    }

    /// Returns the base URL of the current backend
    pub fn backend_url(&self) -> String {
        match &self.backend {
            LlmBackend::Ollama(ollama) => {
                // ollama-rs doesn't expose the URL directly, reconstruct from known state
                // The URL was parsed in new(), but we can't easily get it back.
                // For display purposes, use a best-effort approach.
                format!("{}", ollama.uri())
            }
            LlmBackend::OpenAI { base_url, .. } => base_url.clone(),
            LlmBackend::Queue { .. } => "(calling agent)".to_string(),
        }
    }

    /// Set the status channel for sending trace logs to TUI
    pub fn with_status_tx(mut self, status_tx: mpsc::UnboundedSender<String>) -> Self {
        self.status_tx = Some(status_tx);
        self
    }

    /// The TUI status channel this client narrates to, if any.
    ///
    /// Exposed so the event-logging path (`action_helper::call_llm`) can route
    /// templated event lifecycle lines to the same TUI stream the transport
    /// uses, instead of always passing `None` to the template renderer.
    pub fn status_tx(&self) -> Option<&mpsc::UnboundedSender<String>> {
        self.status_tx.as_ref()
    }

    /// Dual-sink log facade bound to this client's status channel.
    ///
    /// The transport layer owns **wire facts** (model, sizes, tokens) which are
    /// `DEBUG`/`TRACE` and therefore file-only through the facade's defaults; it
    /// narrates to the TUI only for `WARN`/`ERROR`. The conversation layer is the
    /// one that narrates the round-trip to the TUI. This split is what removes the
    /// request/response double-logging.
    fn log(&self) -> Log<'_> {
        Log::new(self.status_tx.as_ref())
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

    /// Override the wall-clock bound on a single backend call
    /// (default [`DEFAULT_REQUEST_TIMEOUT`]).
    pub fn with_request_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Replace the circuit breaker (thresholds are per-breaker; see
    /// [`crate::llm::circuit_breaker`]).
    ///
    /// The breaker is shared by every clone of this client, so one backend outage is
    /// observed once rather than rediscovered by each connection.
    pub fn with_circuit_breaker(mut self, breaker: std::sync::Arc<CircuitBreaker>) -> Self {
        self.breaker = breaker;
        self
    }

    /// The shared circuit breaker guarding this client's backend.
    pub fn circuit_breaker(&self) -> &std::sync::Arc<CircuitBreaker> {
        &self.breaker
    }

    /// Snapshot of backend health — a server whose breaker has tripped should say so.
    pub fn circuit_breaker_status(&self) -> BreakerStatus {
        self.breaker.status()
    }

    /// Whether the breaker guards this backend.
    ///
    /// The agent-queue backend is excluded: its "timeouts" mean the calling MCP agent has
    /// not answered yet, which is not a transport fault, and tripping on a slow human-driven
    /// agent would break the `--llm-agent` flow outright.
    fn breaker_applies(&self) -> bool {
        matches!(
            self.backend,
            LlmBackend::Ollama(_) | LlmBackend::OpenAI { .. }
        )
    }

    /// Fail fast if the backend is known to be down.
    fn breaker_guard(&self) -> Result<()> {
        if !self.breaker_applies() {
            return Ok(());
        }
        match self.breaker.acquire() {
            Ok(()) => Ok(()),
            Err(open) => {
                debug!("Short-circuiting LLM request: {}", open);
                self.log().warn(self.breaker.status().summary());
                Err(anyhow::Error::new(open))
            }
        }
    }

    /// Feed a request outcome back into the breaker and pass the result through unchanged.
    fn record_backend_outcome<T>(&self, result: Result<T>) -> Result<T> {
        if !self.breaker_applies() {
            return result;
        }

        match &result {
            Ok(_) => self.breaker.record_success(),
            Err(e) if is_transport_failure(e) => {
                let summary = format!("{:#}", e);
                if self.breaker.record_failure(&summary) {
                    self.log().error(self.breaker.status().summary());
                } else {
                    warn!(
                        "LLM transport failure {}/{} before the circuit breaker opens: {}",
                        self.breaker.status().consecutive_failures,
                        self.breaker.failure_threshold(),
                        crate::utils::truncate_for_log(&summary, 200)
                    );
                }
            }
            // The backend answered, it just answered with an error. Transport is fine.
            Err(_) => self.breaker.record_success(),
        }

        result
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
        // Fail immediately if the backend is already known to be down, rather than paying
        // another full request timeout to rediscover it. See `crate::llm::circuit_breaker`.
        self.breaker_guard()?;
        let result = self.generate_with_format_inner(model, prompt, format).await;
        self.record_backend_outcome(result)
    }

    async fn generate_with_format_inner(
        &self,
        model: &str,
        prompt: &str,
        format: Option<serde_json::Value>,
    ) -> Result<GenerateResponse> {
        // Transport owns wire facts: DEBUG summary + TRACE payload, both file-only.
        // The conversation layer is the one that narrates the round-trip to the TUI.
        let log = self.log();
        log.debug(format!(
            "LLM request: model={}, prompt_len={} chars, format={}",
            model,
            prompt.len(),
            if format.is_some() { "JSON" } else { "text" }
        ));
        log.payload("Full LLM prompt", prompt, usize::MAX);
        if let Some(ref schema) = format {
            log.trace(format!(
                "JSON schema:\n{}",
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
            ));
        }

        // Dispatch to the appropriate backend
        let (response_text, token_usage) = match &self.backend {
            LlmBackend::Ollama(ollama) => {
                let mut request = GenerationRequest::new(model.to_string(), prompt.to_string());

                // Set num_predict to allow longer responses (especially for binary protocol data)
                use ollama_rs::models::ModelOptions;
                let options = ModelOptions::default().num_predict(2048);
                request = request.options(options);

                // Add format if provided
                if let Some(_schema) = format {
                    use ollama_rs::generation::parameters::FormatType;
                    request = request.format(FormatType::Json);
                }

                let api_response = tokio::time::timeout(
                    self.request_timeout,
                    ollama.generate(request),
                )
                .await
                .with_context(|| format!("Ollama API call timed out after {:?}.\n   Please check:\n   1. Ollama is running (https://ollama.ai)\n   2. Model is loaded and ready\n   3. Use `/model` to list and select a model", self.request_timeout))?
                .map_err(|e| {
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

                let usage = TokenUsage::from_response(&api_response);
                (api_response.response, usage)
            }

            LlmBackend::OpenAI {
                client,
                base_url,
                api_key,
            } => {
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": [{ "role": "user", "content": prompt }],
                    "max_tokens": 2048,
                });

                if format.is_some() {
                    body["response_format"] = serde_json::json!({ "type": "json_object" });
                }

                let url = format!("{}/v1/chat/completions", base_url);

                let http_response = tokio::time::timeout(
                    self.request_timeout,
                    client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", api_key))
                        .header("Content-Type", "application/json")
                        .json(&body)
                        .send(),
                )
                .await
                .with_context(|| {
                    format!("OpenAI API call timed out after {:?}", self.request_timeout)
                })?
                .context("OpenAI API request failed")?;

                let status = http_response.status();
                let response_body: serde_json::Value = http_response
                    .json()
                    .await
                    .context("Failed to parse OpenAI API response")?;

                if !status.is_success() {
                    let error_msg = response_body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");

                    match status.as_u16() {
                        401 => anyhow::bail!(
                            "✗  Authentication failed. Check your API key.\n   Error: {}",
                            error_msg
                        ),
                        404 => anyhow::bail!(
                            "✗  Model '{}' not found.\n   Error: {}",
                            model,
                            error_msg
                        ),
                        429 => anyhow::bail!(
                            "✗  Rate limited by API provider.\n   Error: {}",
                            error_msg
                        ),
                        _ => anyhow::bail!("✗  OpenAI API error ({}): {}", status, error_msg),
                    }
                }

                let text = response_body["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let usage = TokenUsage {
                    prompt_tokens: response_body["usage"]["prompt_tokens"]
                        .as_u64()
                        .unwrap_or(0),
                    completion_tokens: response_body["usage"]["completion_tokens"]
                        .as_u64()
                        .unwrap_or(0),
                    total_tokens: response_body["usage"]["total_tokens"].as_u64().unwrap_or(0),
                };

                (text, usage)
            }

            LlmBackend::Queue { queue, timeout } => {
                // No model: enqueue the prompt and await the agent's action JSON.
                let (id, rx) =
                    queue.submit(model.to_string(), vec![Message::user(prompt)], Vec::new());
                let actions = match tokio::time::timeout(*timeout, rx).await {
                    Ok(Ok(actions)) => actions,
                    Ok(Err(_)) => {
                        queue.expire(id);
                        anyhow::bail!(
                            "agent-queue: LLM request #{} was dropped before it was answered",
                            id
                        );
                    }
                    Err(_) => {
                        queue.expire(id);
                        anyhow::bail!(
                            "agent-queue: LLM request #{} timed out after {:?} without an agent answer",
                            id,
                            timeout
                        );
                    }
                };
                let text = serde_json::json!({ "actions": actions }).to_string();
                (text, TokenUsage::default())
            }
        };

        // Record tokens in app state if available (for /usage command)
        if let Some(ref state) = self.app_state {
            state
                .record_llm_tokens(token_usage.prompt_tokens, token_usage.completion_tokens)
                .await;
        }

        // Wire fact: response size + tokens. DEBUG, file-only.
        log.debug(format!(
            "LLM response: response_len={} chars, tokens={}i/{}o/{}t",
            response_text.len(),
            token_usage.prompt_tokens,
            token_usage.completion_tokens,
            token_usage.total_tokens
        ));

        // Check for empty response (model may be incompatible with JSON format).
        // This is a real failure the peer will feel, so it reaches the TUI (ERROR).
        if response_text.is_empty() || response_text.trim().is_empty() {
            let error_msg = format!(
                "Model '{}' returned empty response (used {} completion tokens).",
                model, token_usage.completion_tokens
            );
            log.error(&error_msg);
            return Err(anyhow::anyhow!(error_msg));
        }

        // TRACE: full payload, file-only (pretty-printed JSON when possible).
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_text) {
            let pretty =
                serde_json::to_string_pretty(&json).unwrap_or_else(|_| response_text.clone());
            log.payload("Full LLM response (JSON)", &pretty, usize::MAX);
        } else {
            log.payload("Full LLM response (text)", &response_text, usize::MAX);
        }

        Ok(GenerateResponse {
            text: response_text,
            token_usage,
        })
    }

    /// Chat with native tool calling support
    ///
    /// Sends structured messages with tool definitions to the LLM backend.
    /// The LLM can respond with text content and/or tool calls.
    ///
    /// This is the preferred method for the chat completions migration.
    /// For Ollama: uses /api/chat with tools parameter
    /// For OpenAI: uses /v1/chat/completions with tools parameter
    ///
    /// # Arguments
    /// * `request` - Chat request with messages, tools, and model
    ///
    /// # Returns
    /// * `Ok(ChatResponse)` - Response with optional content and tool calls
    pub(crate) async fn chat_with_tools(&self, request: &ChatRequest) -> Result<ChatResponse> {
        // See `generate_with_format`: fail fast while the backend is known to be down.
        self.breaker_guard()?;
        let result = self.chat_with_tools_inner(request).await;
        self.record_backend_outcome(result)
    }

    async fn chat_with_tools_inner(&self, request: &ChatRequest) -> Result<ChatResponse> {
        // Transport wire facts: file-only through the facade defaults.
        let log = self.log();
        log.debug(format!(
            "Chat request: model={}, messages={}, tools={}",
            request.model,
            request.messages.len(),
            request.tools.len()
        ));

        log.trace(format!(
            "Chat messages: {:?}",
            request
                .messages
                .iter()
                .map(|m| {
                    format!(
                        "[{}] {}",
                        m.role,
                        crate::utils::truncate_for_log(&m.content, 100)
                    )
                })
                .collect::<Vec<_>>()
        ));

        let chat_response = match &self.backend {
            LlmBackend::Ollama(ollama) => self.chat_with_tools_ollama(ollama, request).await?,
            LlmBackend::OpenAI {
                client,
                base_url,
                api_key,
            } => {
                self.chat_with_tools_openai(client, base_url, api_key, request)
                    .await?
            }
            LlmBackend::Queue { queue, timeout } => {
                self.chat_with_tools_queue(queue.clone(), *timeout, request)
                    .await?
            }
        };

        // Record tokens in app state if available
        if let Some(ref state) = self.app_state {
            state
                .record_llm_tokens(
                    chat_response.token_usage.prompt_tokens,
                    chat_response.token_usage.completion_tokens,
                )
                .await;
        }

        log.debug(format!(
            "Chat response: content_len={}, tool_calls={}, tokens={}i/{}o/{}t",
            chat_response.content.as_ref().map(|c| c.len()).unwrap_or(0),
            chat_response.tool_calls.len(),
            chat_response.token_usage.prompt_tokens,
            chat_response.token_usage.completion_tokens,
            chat_response.token_usage.total_tokens
        ));

        Ok(chat_response)
    }

    /// Agent-queue backend: enqueue the request and await the calling MCP agent's
    /// answer. The answer (a NetGet action JSON array) is wrapped as
    /// `{"actions":[...]}` in the response content — the same shape the test mock
    /// Ollama server produces — so `ActionResponse::from_str` parses it downstream.
    async fn chat_with_tools_queue(
        &self,
        queue: std::sync::Arc<crate::llm::agent_queue::LlmRequestQueue>,
        timeout: std::time::Duration,
        request: &ChatRequest,
    ) -> Result<ChatResponse> {
        let (id, rx) = queue.submit(
            request.model.clone(),
            request.messages.clone(),
            request.tools.clone(),
        );
        self.log().debug(format!(
            "agent-queue: enqueued LLM request #{} ({} tools), awaiting agent answer (timeout {:?})",
            id,
            request.tools.len(),
            timeout
        ));

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(actions)) => {
                // The native-tools pipeline (ConversationHandler) reads tool_calls, not
                // content: turn each answered action into a ToolCall whose function name
                // is the action `type` and whose arguments are the remaining fields.
                let tool_calls = actions
                    .into_iter()
                    .enumerate()
                    .map(|(i, mut action)| {
                        let function_name = action
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if let Some(map) = action.as_object_mut() {
                            map.remove("type");
                        }
                        ToolCall {
                            id: format!("agent-{}", i),
                            function_name,
                            arguments: action,
                        }
                    })
                    .collect();
                Ok(ChatResponse {
                    content: None,
                    tool_calls,
                    token_usage: TokenUsage::default(),
                })
            }
            Ok(Err(_)) => {
                queue.expire(id);
                anyhow::bail!(
                    "agent-queue: LLM request #{} was dropped before it was answered",
                    id
                )
            }
            Err(_) => {
                queue.expire(id);
                anyhow::bail!(
                    "agent-queue: LLM request #{} timed out after {:?} without an agent answer",
                    id,
                    timeout
                )
            }
        }
    }

    /// Ollama backend: /api/chat with tools
    async fn chat_with_tools_ollama(
        &self,
        ollama: &Ollama,
        request: &ChatRequest,
    ) -> Result<ChatResponse> {
        // Build the request body manually since ollama-rs may not support tools natively
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                });
                if let Some(ref tool_call_id) = m.tool_call_id {
                    // This message is a tool result - it's not a standard Ollama field,
                    // but some Ollama models support it. For now, embed in content.
                    msg["tool_call_id"] = serde_json::json!(tool_call_id);
                }
                msg
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": false,
        });

        // Add tools if any
        if !request.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(request.tools.clone());
        }

        // ollama-rs doesn't expose a chat-with-tools API, so we call the endpoint via reqwest.
        // Preserve the client's full base URL (scheme, host, port, path prefix).
        let url = format!("{}/api/chat", ollama.url_str().trim_end_matches('/'));

        let http_client = reqwest::Client::new();
        let http_response = tokio::time::timeout(
            self.request_timeout,
            http_client.post(&url).json(&body).send(),
        )
        .await
        .with_context(|| {
            format!(
                "Ollama chat API call timed out after {:?}",
                self.request_timeout
            )
        })?
        .context("Ollama chat API request failed")?;

        let status = http_response.status();
        let response_body: serde_json::Value = http_response
            .json()
            .await
            .context("Failed to parse Ollama chat API response")?;

        if !status.is_success() {
            let error_msg = response_body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Ollama chat API error ({}): {}", status, error_msg);
        }

        // Parse response
        let content = response_body["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let tool_calls = if let Some(calls) = response_body["message"]["tool_calls"].as_array() {
            calls
                .iter()
                .enumerate()
                .filter_map(|(i, tc)| {
                    let function = tc.get("function")?;
                    let name = function["name"].as_str()?.to_string();
                    let arguments = function.get("arguments").cloned().unwrap_or_default();
                    Some(ToolCall {
                        id: format!("call_{}", i),
                        function_name: name,
                        arguments,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let token_usage = TokenUsage {
            prompt_tokens: response_body["prompt_eval_count"].as_u64().unwrap_or(0),
            completion_tokens: response_body["eval_count"].as_u64().unwrap_or(0),
            total_tokens: response_body["prompt_eval_count"].as_u64().unwrap_or(0)
                + response_body["eval_count"].as_u64().unwrap_or(0),
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            token_usage,
        })
    }

    /// OpenAI backend: /v1/chat/completions with tools
    async fn chat_with_tools_openai(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        request: &ChatRequest,
    ) -> Result<ChatResponse> {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                if let Some(ref tool_call_id) = m.tool_call_id {
                    serde_json::json!({
                        "role": "tool",
                        "content": m.content,
                        "tool_call_id": tool_call_id,
                    })
                } else {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                    })
                }
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": 4096,
        });

        if !request.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(request.tools.clone());
            body["tool_choice"] = serde_json::json!("auto");
        }

        let url = format!("{}/v1/chat/completions", base_url);

        let http_response = tokio::time::timeout(
            self.request_timeout,
            client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send(),
        )
        .await
        .context("OpenAI chat API call timed out after 120 seconds")?
        .context("OpenAI chat API request failed")?;

        let status = http_response.status();
        let response_body: serde_json::Value = http_response
            .json()
            .await
            .context("Failed to parse OpenAI chat API response")?;

        if !status.is_success() {
            let error_msg = response_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            anyhow::bail!("OpenAI chat API error ({}): {}", status, error_msg);
        }

        let message = &response_body["choices"][0]["message"];

        let content = message["content"]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let tool_calls = if let Some(calls) = message["tool_calls"].as_array() {
            calls
                .iter()
                .filter_map(|tc| {
                    let id = tc["id"].as_str()?.to_string();
                    let function = tc.get("function")?;
                    let name = function["name"].as_str()?.to_string();
                    let arguments_str = function["arguments"].as_str().unwrap_or("{}");
                    let arguments = serde_json::from_str(arguments_str).unwrap_or_default();
                    Some(ToolCall {
                        id,
                        function_name: name,
                        arguments,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let token_usage = TokenUsage {
            prompt_tokens: response_body["usage"]["prompt_tokens"]
                .as_u64()
                .unwrap_or(0),
            completion_tokens: response_body["usage"]["completion_tokens"]
                .as_u64()
                .unwrap_or(0),
            total_tokens: response_body["usage"]["total_tokens"].as_u64().unwrap_or(0),
        };

        Ok(ChatResponse {
            content,
            tool_calls,
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
            trace!(
                "Retry loop: attempt={}, prompt_len={}",
                attempt,
                current_prompt.len()
            );

            // Generate response
            let generate_response = self.generate(model, &current_prompt).await?;

            // Extract XML references BEFORE validating JSON format
            // This allows LLM to use <script001> tags without causing "trailing characters" errors
            let (json_only, _refs) =
                crate::llm::reference_parser::extract_references(&generate_response.text)
                    .unwrap_or_else(|_| {
                        (
                            generate_response.text.clone(),
                            std::collections::HashMap::new(),
                        )
                    });

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
                            crate::utils::truncate_for_log(&json_cleaned, 500)
                        );
                        trace!(
                            "Will retry with corrective feedback (attempt {}/{})",
                            attempt,
                            max_retries + 1
                        );

                        // Build retry prompt with correction
                        current_prompt = format!(
                            "{}\n\n---\n\nYour previous response was invalid and could not be parsed.\n\nError: {}\n\nRequired format: {}\n\nPlease provide your response again in the correct format.",
                            current_prompt,
                            e,
                            expected_format
                        );

                        info!(
                            "Retrying with corrective feedback (attempt {}/{})",
                            attempt + 1,
                            max_retries + 1
                        );
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

    /// Check if the LLM backend is available
    ///
    /// This is a probe, not a guard: it costs a round trip and races with the next real
    /// request. Callers wanting "do not attempt a doomed request" want the circuit breaker
    /// ([`Self::circuit_breaker_status`]), which is fed by the requests themselves.
    pub async fn is_available(&self) -> bool {
        self.list_models().await.is_ok()
    }

    /// List available models from the backend
    pub async fn list_models(&self) -> Result<Vec<String>> {
        match &self.backend {
            LlmBackend::Ollama(ollama) => {
                // ollama-rs applies no timeout of its own, so an unreachable host that
                // silently drops packets would hang this call — and `is_available()` with it
                // — indefinitely.
                let models = tokio::time::timeout(self.request_timeout, ollama.list_local_models())
                    .await
                    .with_context(|| {
                        format!(
                            "Listing Ollama models timed out after {:?}",
                            self.request_timeout
                        )
                    })?
                    .map_err(|e| anyhow::anyhow!("Failed to list models: {}", e))?;
                Ok(models.into_iter().map(|m| m.name).collect())
            }
            LlmBackend::OpenAI {
                client,
                base_url,
                api_key,
            } => {
                let url = format!("{}/v1/models", base_url);
                let response = client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                    .context("Failed to connect to OpenAI-compatible API")?;

                if !response.status().is_success() {
                    anyhow::bail!("OpenAI API returned error status: {}", response.status());
                }

                let body: serde_json::Value = response
                    .json()
                    .await
                    .context("Failed to parse model list response")?;

                let models = body["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                    .collect();
                Ok(models)
            }
            LlmBackend::Queue { .. } => Ok(vec!["agent".to_string()]),
        }
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        OllamaClient::default()
    }
}
