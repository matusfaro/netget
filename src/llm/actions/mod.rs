//! Action-based system for LLM interactions
//!
//! This module provides a unified action system where both user input
//! and network events return arrays of actions to execute.

pub mod common;
pub mod executor;
pub mod protocol_trait;
pub mod summary;
pub mod tools;

// Re-export commonly used functions and types
pub use common::{
    generate_base_stack_documentation, get_network_event_common_actions,
    get_user_input_common_actions,
};
pub use protocol_trait::Server; // Export the Server trait
pub use summary::{summarize_action, summarize_actions};
pub use tools::{execute_tool, get_all_tool_actions, ToolAction, ToolResult};

use serde::{Deserialize, Serialize};

/// Definition of a configuration parameter for prompt generation
///
/// This describes a startup parameter that a protocol accepts,
/// including its name, type, description, and an example value.
#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    /// Parameter name (e.g., "certificate_mode", "max_connections")
    pub name: String,

    /// Type hint for the LLM (e.g., "string", "number", "boolean", "object")
    pub type_hint: String,

    /// Human-readable description of what this parameter does
    pub description: String,

    /// Whether this parameter is required
    pub required: bool,

    /// JSON example showing a valid value for this parameter
    pub example: serde_json::Value,
}

impl ParameterDefinition {
    /// Convert to prompt text format for LLM
    pub fn to_prompt_text(&self) -> String {
        let required = if self.required {
            "required"
        } else {
            "optional"
        };
        format!(
            "\"{}\": {}  // {} ({})\nExample: {}",
            self.name,
            self.type_hint,
            self.description,
            required,
            serde_json::to_string(&self.example).unwrap_or_default()
        )
    }
}

/// Definition of an action for prompt generation
///
/// This describes an action to the LLM, including its name,
/// description, parameters, and an example.
#[derive(Debug, Clone)]
pub struct ActionDefinition {
    /// Action type name (e.g., "send_tcp_data", "close_connection")
    pub name: String,

    /// Human-readable description of what this action does
    pub description: String,

    /// List of parameters this action accepts
    pub parameters: Vec<Parameter>,

    /// JSON example showing how to use this action
    pub example: serde_json::Value,
}

impl ActionDefinition {
    /// Check if this is a tool action (returns information and triggers LLM re-invocation)
    pub fn is_tool(&self) -> bool {
        matches!(self.name.as_str(), "read_file" | "web_search" | "read_base_stack_docs" | "list_network_interfaces")
    }

    /// Convert to prompt text format for LLM
    pub fn to_prompt_text(&self) -> String {
        let mut text = format!("{}\n\n{}\n", self.name, self.description);

        // Only show schema if there are parameters
        if !self.parameters.is_empty() {
            text.push_str("\nParameters:\n");
            for param in &self.parameters {
                let required = if param.required {
                    "required"
                } else {
                    "optional"
                };
                text.push_str(&format!(
                    "- {}: {} ({}) - {}\n",
                    param.name, param.type_hint, required, param.description
                ));
            }
        }

        text.push_str("\nExample:\n```json\n");
        text.push_str(&serde_json::to_string_pretty(&self.example).unwrap_or_default());
        text.push_str("\n```");
        text
    }
}

/// Parameter definition for an action
#[derive(Debug, Clone)]
pub struct Parameter {
    /// Parameter name (e.g., "output", "connection_id")
    pub name: String,

    /// Type hint for the LLM (e.g., "string", "number", "boolean")
    pub type_hint: String,

    /// Description of what this parameter does
    pub description: String,

    /// Whether this parameter is required
    pub required: bool,
}

/// Response from LLM containing array of actions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionResponse {
    /// Array of actions to execute in order
    pub actions: Vec<serde_json::Value>,
}

impl ActionResponse {
    /// Parse from JSON string
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let trimmed = s.trim();

        // Strip markdown code fences if present (```json ... ``` or ``` ... ```)
        let json_str = if trimmed.starts_with("```") {
            // Find the first newline after opening fence
            let start = trimmed.find('\n').unwrap_or(3);
            // Find the closing fence
            let end = trimmed.rfind("```").unwrap_or(trimmed.len());
            // Extract content between fences
            trimmed[start..end].trim()
        } else {
            trimmed
        };

        // Strip any extra characters before the JSON object
        // Sometimes LLMs add extra text like "Y{" instead of just "{"
        let json_start = json_str.find('{').unwrap_or(0);
        let clean_json = &json_str[json_start..];

        serde_json::from_str::<ActionResponse>(clean_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse action response: {}. Input: {}", e, clean_json))
    }

    /// Create empty action response
    pub fn empty() -> Self {
        Self {
            actions: Vec::new(),
        }
    }
}
