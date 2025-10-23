//! Ollama client using ollama-rs library for LLM communication

use std::collections::HashMap;

use anyhow::{Context, Result};
use bytes::Bytes;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

// Note: We still derive JsonSchema for development/testing purposes,
// but at runtime we use the explicit JSON schemas in src/llm/schemas/
#[allow(unused_imports)]
use schemars::JsonSchema;

/// Structured response from the LLM
///
/// WARNING: If you modify this struct, you MUST also update the corresponding
/// JSON schema file at: src/llm/schemas/llm_response.json
/// The schema file is used for Ollama's structured output feature.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
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

impl Default for LlmResponse {
    fn default() -> Self {
        Self {
            output: None,
            close_connection: false,
            wait_for_more: false,
            shutdown_server: false,
            log_message: None,
            set_memory: None,
            append_memory: None,
        }
    }
}

impl LlmResponse {
    /// Parse from JSON string with fallback to legacy text format
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
///
/// WARNING: If you modify this struct, you MUST also update the corresponding
/// JSON schema file at: src/llm/schemas/http_response.json
/// The schema file is used for Ollama's structured output feature.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
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

impl HttpLlmResponse {
    /// Parse from JSON string
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

/// Action types for command interpretation
///
/// WARNING: If you modify this enum, you MUST also update the corresponding
/// JSON schema file at: src/llm/schemas/command_interpretation.json
/// The schema file is used for Ollama's structured output feature.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandAction {
    UpdateInstruction {
        instruction: String
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
    CloseServer,
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
///
/// WARNING: If you modify this struct, you MUST also update the corresponding
/// JSON schema file at: src/llm/schemas/command_interpretation.json
/// The schema file is used for Ollama's structured output feature.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
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
    pub fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        serde_json::from_str::<CommandInterpretation>(trimmed)
            .context("Failed to parse command interpretation as JSON")
    }
}

/// Ollama API client using ollama-rs
#[derive(Clone)]
pub struct OllamaClient {
    ollama: Ollama,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: impl Into<String>) -> Self {
        let url_str = base_url.into();
        let ollama = Ollama::new(url_str.as_str(), 11434);
        Self { ollama }
    }

    /// Create a default client pointing to localhost
    pub fn default() -> Self {
        let ollama = Ollama::default();
        Self { ollama }
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
        format: Option<serde_json::Value>
    ) -> Result<String> {
        // DEBUG: Summary
        debug!(
            "LLM request: model={}, prompt_len={} chars, format={}",
            model,
            prompt.len(),
            if format.is_some() { "JSON" } else { "text" }
        );

        // TRACE: Full payload
        trace!("Full LLM prompt:\n{}", prompt);
        if let Some(ref schema) = format {
            trace!("JSON schema:\n{}", serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string()));
        }

        let mut request = GenerationRequest::new(model.to_string(), prompt.to_string());

        // Add format if provided
        if let Some(_schema) = format {
            // For now, use plain JSON format since we need to handle structured JSON differently
            // The ollama-rs StructuredJson format requires a schemars Schema type
            // We'll use the simpler JSON format and rely on prompt engineering
            use ollama_rs::generation::parameters::FormatType;
            request = request.format(FormatType::Json);
        }

        let response = self.ollama
            .generate(request)
            .await
            .map_err(|e| anyhow::anyhow!("Ollama request failed: {}", e))?;

        // DEBUG: Summary
        debug!(
            "LLM response: response_len={} chars",
            response.response.len()
        );

        // TRACE: Full payload with pretty-printed JSON if possible
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response.response) {
            trace!("Full LLM response (JSON):\n{}", serde_json::to_string_pretty(&json).unwrap_or(response.response.clone()));
        } else {
            trace!("Full LLM response (text):\n{}", response.response);
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
    pub async fn generate_http_response(&self, model: &str, prompt: &str) -> Result<HttpLlmResponse> {
        // Disabled structured outputs - see generate_llm_response
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        HttpLlmResponse::from_str(&response_text)
    }

    /// Generate a structured CommandInterpretation for user commands
    pub async fn generate_command_interpretation(&self, model: &str, prompt: &str) -> Result<CommandInterpretation> {
        // Disabled structured outputs - see generate_llm_response
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        CommandInterpretation::from_str(&response_text)
    }

    /// Check if Ollama is available
    pub async fn is_available(&self) -> bool {
        // Try to list models as a health check
        self.list_models().await.is_ok()
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let models = self.ollama
            .list_local_models()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list models: {}", e))?;

        Ok(models.into_iter().map(|m| m.name).collect())
    }
}