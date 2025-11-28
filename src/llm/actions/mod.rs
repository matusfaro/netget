//! Action-based system for LLM interactions
//!
//! This module provides a unified action system where both user input
//! and network events return arrays of actions to execute.

pub mod client_trait;
pub mod common;
pub mod easy_trait;
pub mod executor;
pub mod protocol_trait;
pub mod summary;
pub mod tools;

// Re-export commonly used functions and types
pub use client_trait::Client;
// Export the Client trait
pub use common::{
    generate_base_stack_documentation, get_network_event_common_actions,
    get_user_input_common_actions,
};
pub use easy_trait::Easy;
// Export the Easy trait
pub use protocol_trait::{Protocol, Server};
// Export StartupExamples for protocol implementations
pub use summary::{summarize_action, summarize_actions};
pub use tools::{
    execute_tool, get_all_tool_actions, get_network_event_tool_actions, ToolAction, ToolResult,
};

use serde::{Deserialize, Serialize};

/// Examples showing how to start a protocol with different handler modes
///
/// These examples are required for every protocol and are used in:
/// - Protocol documentation (shown when user requests docs)
/// - Prompt templates to guide LLM in creating servers/clients
///
/// Each example is a complete `open_server` action JSON that can be
/// directly executed. The examples must be validated by tests.
#[derive(Clone, Debug, Serialize)]
pub struct StartupExamples {
    /// Complete open_server action with LLM handler mode
    /// Shows how to start the protocol with LLM-controlled responses
    pub llm_mode: serde_json::Value,

    /// Complete open_server action with script handler mode
    /// Shows event_handlers with script type handlers
    pub script_mode: serde_json::Value,

    /// Complete open_server action with static handler mode
    /// Shows event_handlers with static type handlers and predefined actions
    pub static_mode: serde_json::Value,
}

impl StartupExamples {
    /// Create new startup examples
    pub fn new(
        llm_mode: serde_json::Value,
        script_mode: serde_json::Value,
        static_mode: serde_json::Value,
    ) -> Self {
        Self {
            llm_mode,
            script_mode,
            static_mode,
        }
    }

    /// Validate that all examples are well-formed open_server actions
    ///
    /// Returns Ok(()) if valid, Err with description if invalid.
    /// This is called by parameterized tests to ensure examples stay valid.
    pub fn validate(&self, protocol_name: &str) -> Result<(), String> {
        self.validate_example(&self.llm_mode, "llm_mode", protocol_name)?;
        self.validate_example(&self.script_mode, "script_mode", protocol_name)?;
        self.validate_example(&self.static_mode, "static_mode", protocol_name)?;
        Ok(())
    }

    fn validate_example(
        &self,
        example: &serde_json::Value,
        mode_name: &str,
        protocol_name: &str,
    ) -> Result<(), String> {
        // Must be an object
        let obj = example.as_object().ok_or_else(|| {
            format!(
                "Protocol {} {} example must be a JSON object",
                protocol_name, mode_name
            )
        })?;

        // Must have "type" field
        let action_type = obj.get("type").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "Protocol {} {} example missing 'type' field",
                protocol_name, mode_name
            )
        })?;

        // Type must be "open_server" or "open_client"
        if action_type != "open_server" && action_type != "open_client" {
            return Err(format!(
                "Protocol {} {} example has type '{}', expected 'open_server' or 'open_client'",
                protocol_name, mode_name, action_type
            ));
        }

        // Must have "base_stack" field
        if !obj.contains_key("base_stack") {
            return Err(format!(
                "Protocol {} {} example missing 'base_stack' field",
                protocol_name, mode_name
            ));
        }

        // For script_mode and static_mode, must have event_handlers
        if mode_name == "script_mode" || mode_name == "static_mode" {
            let handlers = obj.get("event_handlers").ok_or_else(|| {
                format!(
                    "Protocol {} {} example missing 'event_handlers' array",
                    protocol_name, mode_name
                )
            })?;

            if !handlers.is_array() {
                return Err(format!(
                    "Protocol {} {} example 'event_handlers' must be an array",
                    protocol_name, mode_name
                ));
            }
        }

        // For llm_mode, should have instruction
        if mode_name == "llm_mode" && !obj.contains_key("instruction") {
            return Err(format!(
                "Protocol {} llm_mode example missing 'instruction' field",
                protocol_name
            ));
        }

        Ok(())
    }

    /// Convert examples to prompt text for LLM documentation
    pub fn to_prompt_text(&self) -> String {
        let mut text = String::new();

        text.push_str("### Starting this Protocol\n\n");

        text.push_str("**LLM Mode** (LLM handles all responses intelligently):\n");
        text.push_str("```json\n");
        text.push_str(&serde_json::to_string_pretty(&self.llm_mode).unwrap_or_default());
        text.push_str("\n```\n\n");

        text.push_str("**Script Mode** (code-based deterministic responses):\n");
        text.push_str("```json\n");
        text.push_str(&serde_json::to_string_pretty(&self.script_mode).unwrap_or_default());
        text.push_str("\n```\n\n");

        text.push_str("**Static Mode** (fixed, unchanging responses):\n");
        text.push_str("```json\n");
        text.push_str(&serde_json::to_string_pretty(&self.static_mode).unwrap_or_default());
        text.push_str("\n```\n\n");

        text
    }
}

/// Definition of a configuration parameter for prompt generation
///
/// This describes a startup parameter that a protocol accepts,
/// including its name, type, description, and an example value.
#[derive(Debug, Clone, Serialize)]
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
        matches!(
            self.name.as_str(),
            "read_file"
                | "web_search"
                | "read_base_stack_docs"
                | "list_network_interfaces"
                | "list_models"
                | "generate_random"
        )
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
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let trimmed = s.trim();

        // Strip markdown code fences if present (```json ... ``` or ``` ... ```)
        let json_str = if trimmed.starts_with("```") {
            // Find the first newline after opening fence
            let start = trimmed.find('\n').unwrap_or(3);
            // Find the closing fence (must be after start)
            let end = trimmed[start..]
                .rfind("```")
                .map(|pos| start + pos)
                .unwrap_or(trimmed.len());
            // Extract content between fences (ensure valid slice)
            if start <= end {
                trimmed[start..end].trim()
            } else {
                trimmed
            }
        } else {
            trimmed
        };

        // Strip any extra characters before the JSON object
        // Sometimes LLMs add extra text like "Y{" instead of just "{"
        let json_start = json_str.find('{').unwrap_or(0);
        let clean_json = &json_str[json_start..];

        serde_json::from_str::<ActionResponse>(clean_json).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse action response: {}\n\n❌ Expected format:\n{{\n  \"actions\": [\n    {{ \"type\": \"...\", ... }}\n  ]\n}}\n\n❌ Actual response:\n{}",
                e,
                clean_json
            )
        })
    }

    /// Create empty action response
    pub fn empty() -> Self {
        Self {
            actions: Vec::new(),
        }
    }
}

impl std::str::FromStr for ActionResponse {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ActionResponse::from_str(s)
    }
}
