//! Ollama client for LLM communication

use std::collections::HashMap;

use anyhow::{Context, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

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
    UpdateInstruction { instruction: String },
    OpenServer {
        port: u16,
        base_stack: String,
        #[serde(default)]
        send_first: bool,
        #[serde(default)]
        initial_memory: Option<String>,
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

/// Ollama API client
#[derive(Clone)]
pub struct OllamaClient {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Create a default client pointing to localhost
    pub fn default() -> Self {
        Self::new("http://localhost:11434")
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
        let url = format!("{}/api/generate", self.base_url);

        debug!("Sending prompt to Ollama (model: {})", model);
        if let Some(ref schema) = format {
            debug!("Using structured output with JSON schema");
            debug!("Schema: {}", serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string()));
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

        info!("Received response from Ollama ({} tokens)", response.eval_count.unwrap_or(0));
        debug!("Response: {}", response.response);

        Ok(response.response)
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
    pub async fn generate_http_response(&self, model: &str, prompt: &str) -> Result<HttpLlmResponse> {
        // NOTE: Structured outputs disabled - see generate_llm_response for details
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        HttpLlmResponse::from_str(&response_text)
    }

    /// Generate a structured CommandInterpretation for user commands
    pub async fn generate_command_interpretation(&self, model: &str, prompt: &str) -> Result<CommandInterpretation> {
        // NOTE: Structured outputs disabled - see generate_llm_response for details
        let response_text = self.generate_with_format(model, prompt, None).await?;

        // Parse the JSON response
        CommandInterpretation::from_str(&response_text)
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
