//! Action-based system for LLM interactions
//!
//! This module provides a unified action system where both user input
//! and network events return arrays of actions to execute.

pub mod protocol_trait;
pub mod executor;
pub mod common;

// Re-export commonly used functions and types
pub use common::{get_user_input_common_actions, get_network_event_common_actions, generate_base_stack_documentation};

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

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
        let required = if self.required { "required" } else { "optional" };
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
    /// Convert to prompt text format for LLM
    pub fn to_prompt_text(&self) -> String {
        let mut text = format!("{}. {}\n", self.name, self.description);
        text.push_str("{\n");
        text.push_str(&format!("  \"type\": \"{}\",\n", self.name));

        for (i, param) in self.parameters.iter().enumerate() {
            let required = if param.required { "required" } else { "optional" };
            text.push_str(&format!(
                "  \"{}\": {},  // {} ({})\n",
                param.name,
                param.type_hint,
                param.description,
                required
            ));

            if i == self.parameters.len() - 1 {
                // Remove trailing comma from last parameter
                text = text.trim_end_matches(",  // ").to_string();
                text.push_str("  // ");
                text.push_str(&format!("{} ({})\n", param.description, required));
            }
        }

        text.push_str("}\n\nExample:\n");
        text.push_str(&serde_json::to_string_pretty(&self.example).unwrap_or_default());
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
///
/// WARNING: If you modify this struct, you MUST also update the corresponding
/// JSON schema file at: src/llm/schemas/action_response.json
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ActionResponse {
    /// Array of actions to execute in order
    pub actions: Vec<serde_json::Value>,
}

impl ActionResponse {
    /// Parse from JSON string
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let trimmed = s.trim();
        serde_json::from_str::<ActionResponse>(trimmed)
            .map_err(|e| anyhow::anyhow!("Failed to parse action response: {}", e))
    }

    /// Create empty action response
    pub fn empty() -> Self {
        Self { actions: Vec::new() }
    }
}
