//! Types for script-based response handling

use serde::{Deserialize, Serialize};

/// Supported scripting languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptLanguage {
    Python,
    JavaScript,
}

impl ScriptLanguage {
    /// Get the command to execute this language
    pub fn command(&self) -> &'static str {
        match self {
            ScriptLanguage::Python => "python3",
            ScriptLanguage::JavaScript => "node",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "python" | "python3" => Some(ScriptLanguage::Python),
            "javascript" | "js" | "node" => Some(ScriptLanguage::JavaScript),
            _ => None,
        }
    }

    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptLanguage::Python => "python",
            ScriptLanguage::JavaScript => "javascript",
        }
    }
}

/// Source of the script code
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    /// Script loaded from a file path
    FilePath(String),
    /// Script provided as inline code
    Inline(String),
}

impl ScriptSource {
    /// Get the script code (either by reading file or returning inline code)
    pub fn get_code(&self) -> Result<String, std::io::Error> {
        match self {
            ScriptSource::FilePath(path) => std::fs::read_to_string(path),
            ScriptSource::Inline(code) => Ok(code.clone()),
        }
    }
}

/// Script configuration for a server
#[derive(Debug, Clone)]
pub struct ScriptConfig {
    /// Scripting language
    pub language: ScriptLanguage,

    /// Source of the script
    pub source: ScriptSource,

    /// Context types this script handles (e.g., ["ssh_auth", "ssh_banner"] or ["all"])
    pub handles_contexts: Vec<String>,
}

impl ScriptConfig {
    /// Check if this script handles a given context type
    pub fn handles_context(&self, context_type: &str) -> bool {
        self.handles_contexts.contains(&"all".to_string())
            || self.handles_contexts.contains(&context_type.to_string())
    }

    /// Add context types to the handles list
    pub fn add_contexts(&mut self, contexts: Vec<String>) {
        for context in contexts {
            if !self.handles_contexts.contains(&context) {
                self.handles_contexts.push(context);
            }
        }
        // If "all" is present, no need for specific contexts
        if self.handles_contexts.contains(&"all".to_string()) {
            self.handles_contexts = vec!["all".to_string()];
        }
    }

    /// Remove context types from the handles list
    pub fn remove_contexts(&mut self, contexts: &[String]) {
        self.handles_contexts.retain(|c| !contexts.contains(c));
    }
}

/// Structured input sent to scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptInput {
    /// Type of event/context (e.g., "ssh_auth", "ssh_banner", "http_request")
    pub context_type: String,

    /// Server information
    pub server: ServerContext,

    /// Connection information (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionContext>,

    /// Protocol-specific event data
    pub event: serde_json::Value,
}

/// Server context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerContext {
    /// Server ID
    pub id: u32,

    /// Listening port
    pub port: u16,

    /// Protocol stack name
    pub stack: String,

    /// Server memory (state storage)
    pub memory: String,

    /// User instructions for the server
    pub instruction: String,
}

/// Connection context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionContext {
    /// Connection ID
    pub id: String,

    /// Remote address
    pub remote_addr: String,

    /// Bytes received on this connection
    pub bytes_received: u64,

    /// Bytes sent on this connection
    pub bytes_sent: u64,
}

/// Response from a script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptResponse {
    /// Array of actions to execute (same format as LLM actions)
    #[serde(default)]
    pub actions: Vec<serde_json::Value>,

    /// Whether to fallback to LLM for this request
    #[serde(default)]
    pub fallback_to_llm: bool,

    /// Optional reason for fallback (for debugging)
    #[serde(default)]
    pub fallback_reason: Option<String>,
}

impl ScriptResponse {
    /// Create a response with actions
    pub fn with_actions(actions: Vec<serde_json::Value>) -> Self {
        Self {
            actions,
            fallback_to_llm: false,
            fallback_reason: None,
        }
    }

    /// Create a response that requests LLM fallback
    pub fn fallback(reason: impl Into<String>) -> Self {
        Self {
            actions: Vec::new(),
            fallback_to_llm: true,
            fallback_reason: Some(reason.into()),
        }
    }

    /// Parse from JSON string
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        serde_json::from_str(s)
            .map_err(|e| anyhow::anyhow!("Failed to parse script response: {}", e))
    }
}

/// Operations for updating script configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptUpdateOperation {
    /// Replace entire script configuration
    Set,

    /// Add context types to existing configuration
    AddContexts,

    /// Remove context types from existing configuration
    RemoveContexts,

    /// Disable scripts entirely (remove configuration)
    Disable,
}

impl ScriptUpdateOperation {
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "set" => Some(ScriptUpdateOperation::Set),
            "add_contexts" | "add" => Some(ScriptUpdateOperation::AddContexts),
            "remove_contexts" | "remove" => Some(ScriptUpdateOperation::RemoveContexts),
            "disable" | "clear" => Some(ScriptUpdateOperation::Disable),
            _ => None,
        }
    }
}
