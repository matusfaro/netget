//! Ollama client for LLM communication

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

/// Structured response from the LLM
#[derive(Debug, Clone, Deserialize, Serialize)]
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
}

impl Default for LlmResponse {
    fn default() -> Self {
        Self {
            output: None,
            close_connection: false,
            wait_for_more: false,
            shutdown_server: false,
            log_message: None,
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

    /// Generate a completion from the model
    pub async fn generate(&self, model: &str, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);

        debug!("Sending prompt to Ollama (model: {})", model);
        debug!("Prompt: {}", prompt);

        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            stream: false,
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
