//! Event-specific instructions and configuration
//!
//! This module provides simple default instructions for each EventType,
//! with the ability to override them at server configuration time.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Event-specific instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInstructions {
    /// Mandatory instructions string
    pub instructions: String,

    /// Optional examples for the event
    #[serde(default)]
    pub examples: Vec<Example>,
}

/// An example for an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    /// Input example
    pub input: String,

    /// Expected output
    pub output: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Server instruction configuration
#[derive(Debug, Clone, Default)]
pub struct ServerInstructionConfig {
    /// Global instructions that apply to all events
    pub global_instructions: Option<String>,

    /// Event-specific instruction overrides
    pub event_overrides: HashMap<String, EventInstructions>,

    /// Existing scripting configuration (preserved)
    pub scripts: HashMap<String, crate::scripting::types::ScriptConfig>,
}

impl ServerInstructionConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set global instructions
    pub fn with_global_instructions(mut self, instructions: String) -> Self {
        self.global_instructions = Some(instructions);
        self
    }

    /// Add an event-specific override
    pub fn with_event_override(
        mut self,
        event_type: String,
        instructions: EventInstructions,
    ) -> Self {
        self.event_overrides.insert(event_type, instructions);
        self
    }

    /// Get instructions for a specific event type
    pub fn get_instructions_for_event(&self, event_type: &str) -> Option<&EventInstructions> {
        self.event_overrides.get(event_type)
    }

    /// Check if a script handles this event type (priority over instructions)
    pub fn has_script_for_event(&self, event_type: &str) -> bool {
        self.scripts.iter().any(|(_, config)| config.handles_context(event_type))
    }
}

/// Result of instruction resolution
#[derive(Debug, Clone)]
pub enum InstructionSource {
    /// Using a script
    Script(String),
    /// Using event override instructions
    EventOverride(EventInstructions),
    /// Using default instructions
    Default(EventInstructions),
    /// Using global instructions only
    GlobalOnly(String),
    /// No instructions found
    None,
}

impl EventInstructions {
    /// Create new event instructions with just text
    pub fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: instructions.into(),
            examples: Vec::new(),
        }
    }

    /// Add an example
    pub fn with_example(mut self, input: impl Into<String>, output: impl Into<String>) -> Self {
        self.examples.push(Example {
            input: input.into(),
            output: output.into(),
            description: None,
        });
        self
    }

    /// Add an example with description
    pub fn with_described_example(
        mut self,
        input: impl Into<String>,
        output: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.examples.push(Example {
            input: input.into(),
            output: output.into(),
            description: Some(description.into()),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_instructions_builder() {
        let instructions = EventInstructions::new("Handle HTTP request")
            .with_example("GET /", "200 OK")
            .with_described_example(
                "POST /api",
                r#"{"status": "success"}"#,
                "API endpoint example",
            );

        assert_eq!(instructions.instructions, "Handle HTTP request");
        assert_eq!(instructions.examples.len(), 2);
        assert_eq!(instructions.examples[0].input, "GET /");
        assert_eq!(instructions.examples[1].description, Some("API endpoint example".to_string()));
    }

    #[test]
    fn test_server_config() {
        let config = ServerInstructionConfig::new()
            .with_global_instructions("Be helpful".to_string())
            .with_event_override(
                "http_request".to_string(),
                EventInstructions::new("Return JSON responses"),
            );

        assert_eq!(config.global_instructions, Some("Be helpful".to_string()));
        assert!(config.get_instructions_for_event("http_request").is_some());
        assert!(config.get_instructions_for_event("unknown").is_none());
    }
}