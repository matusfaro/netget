//! Persistent configuration for LLM backends and settings
//!
//! Stores user preferences in ~/.netget/config.toml

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// LLM backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackendType {
    /// Ollama HTTP API (external service)
    Ollama,
    /// Embedded llama.cpp inference
    #[cfg(feature = "embedded-llm")]
    Embedded,
}

impl std::fmt::Display for LlmBackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmBackendType::Ollama => write!(f, "Ollama"),
            #[cfg(feature = "embedded-llm")]
            LlmBackendType::Embedded => write!(f, "Embedded (llama.cpp)"),
        }
    }
}

/// Persistent configuration for NetGet LLM settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetGetConfig {
    /// Last used backend type
    #[serde(default)]
    pub last_backend: Option<LlmBackendType>,

    /// Ollama configuration
    #[serde(default)]
    pub ollama: OllamaConfig,

    /// Embedded LLM configuration
    #[cfg(feature = "embedded-llm")]
    #[serde(default)]
    pub embedded: EmbeddedLlmConfig,

    /// Model download directory
    #[serde(default = "default_models_dir")]
    pub models_dir: PathBuf,
}

/// Ollama-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    /// Ollama API base URL
    #[serde(default = "default_ollama_url")]
    pub base_url: String,

    /// Last used Ollama model
    #[serde(default)]
    pub last_model: Option<String>,

    /// Prefer Ollama over embedded (when both available)
    #[serde(default = "default_true")]
    pub prefer: bool,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_url(),
            last_model: None,
            prefer: true,
        }
    }
}

/// Embedded LLM configuration
#[cfg(feature = "embedded-llm")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedLlmConfig {
    /// Last used model path
    #[serde(default)]
    pub last_model_path: Option<PathBuf>,

    /// Context size (tokens)
    #[serde(default = "default_context_size")]
    pub context_size: u32,

    /// Max generation tokens
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// GPU layers to offload (u32::MAX = auto)
    #[serde(default = "default_gpu_layers")]
    pub n_gpu_layers: u32,

    /// Number of threads (0 = auto)
    #[serde(default)]
    pub n_threads: u32,
}

#[cfg(feature = "embedded-llm")]
impl Default for EmbeddedLlmConfig {
    fn default() -> Self {
        Self {
            last_model_path: None,
            context_size: default_context_size(),
            max_tokens: default_max_tokens(),
            n_gpu_layers: default_gpu_layers(),
            n_threads: 0,
        }
    }
}

impl Default for NetGetConfig {
    fn default() -> Self {
        Self {
            last_backend: None,
            ollama: OllamaConfig::default(),
            #[cfg(feature = "embedded-llm")]
            embedded: EmbeddedLlmConfig::default(),
            models_dir: default_models_dir(),
        }
    }
}

impl NetGetConfig {
    /// Load configuration from ~/.netget/config.toml
    pub fn load() -> Result<Self> {
        let config_path = Self::config_file_path()?;

        if !config_path.exists() {
            debug!("Config file not found, using defaults: {}", config_path.display());
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path)
            .context("Failed to read config file")?;

        let config: NetGetConfig = toml::from_str(&contents)
            .context("Failed to parse config file")?;

        debug!("Loaded config from: {}", config_path.display());
        Ok(config)
    }

    /// Save configuration to ~/.netget/config.toml
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path()?;

        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }

        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;

        std::fs::write(&config_path, contents)
            .context("Failed to write config file")?;

        debug!("Saved config to: {}", config_path.display());
        Ok(())
    }

    /// Get path to config file (~/.netget/config.toml)
    pub fn config_file_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Cannot find home directory")?;
        Ok(home.join(".netget").join("config.toml"))
    }

    /// Get path to models directory (~/.netget/models/)
    pub fn models_directory(&self) -> &Path {
        &self.models_dir
    }

    /// Ensure models directory exists
    pub fn ensure_models_dir(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.models_dir)
            .context("Failed to create models directory")?;
        Ok(self.models_dir.clone())
    }

    /// Update last used backend and save
    pub fn set_last_backend(&mut self, backend: LlmBackendType) -> Result<()> {
        self.last_backend = Some(backend);
        self.save()
    }

    /// Update Ollama model and save
    pub fn set_ollama_model(&mut self, model: impl Into<String>) -> Result<()> {
        self.ollama.last_model = Some(model.into());
        self.last_backend = Some(LlmBackendType::Ollama);
        self.save()
    }

    /// Update embedded model path and save
    #[cfg(feature = "embedded-llm")]
    pub fn set_embedded_model(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        self.embedded.last_model_path = Some(path.into());
        self.last_backend = Some(LlmBackendType::Embedded);
        self.save()
    }
}

// Default value functions
fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_models_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".netget")
        .join("models")
}

fn default_context_size() -> u32 {
    4096
}

fn default_max_tokens() -> usize {
    2048
}

fn default_gpu_layers() -> u32 {
    u32::MAX // Auto-detect
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NetGetConfig::default();
        assert_eq!(config.ollama.base_url, "http://localhost:11434");
        assert!(config.ollama.prefer);
        assert_eq!(config.last_backend, None);
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = NetGetConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: NetGetConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.ollama.base_url, config.ollama.base_url);
    }

    #[cfg(feature = "embedded-llm")]
    #[test]
    fn test_embedded_config() {
        let config = NetGetConfig::default();
        assert_eq!(config.embedded.context_size, 4096);
        assert_eq!(config.embedded.max_tokens, 2048);
        assert_eq!(config.embedded.n_gpu_layers, u32::MAX);
    }
}
