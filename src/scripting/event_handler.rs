//! Event handler configuration system
//!
//! This module defines how events are handled - either by LLM, script, or static responses.

use serde::{Deserialize, Serialize};

/// Pattern for matching events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventPattern {
    /// Match a specific event type ID
    Specific(String),
    /// Match all events
    Wildcard,
}

impl EventPattern {
    /// Check if this pattern matches the given event type ID
    pub fn matches(&self, event_type_id: &str) -> bool {
        match self {
            EventPattern::Specific(pattern) => pattern == event_type_id,
            EventPattern::Wildcard => true,
        }
    }

    /// Create a wildcard pattern
    pub fn wildcard() -> Self {
        EventPattern::Wildcard
    }

    /// Create a specific pattern
    pub fn specific(event_type_id: impl Into<String>) -> Self {
        EventPattern::Specific(event_type_id.into())
    }
}

impl From<String> for EventPattern {
    fn from(s: String) -> Self {
        if s == "*" || s == "all" {
            EventPattern::Wildcard
        } else {
            EventPattern::Specific(s)
        }
    }
}

impl From<&str> for EventPattern {
    fn from(s: &str) -> Self {
        if s == "*" || s == "all" {
            EventPattern::Wildcard
        } else {
            EventPattern::Specific(s.to_string())
        }
    }
}

/// Handler type for an event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventHandlerType {
    /// Handle with LLM (default behavior)
    Llm {
        /// Instruction for how the LLM should handle this event
        instruction: String,
    },

    /// Handle with inline script
    Script {
        /// Scripting language (python, javascript, go, perl)
        language: String,
        /// Inline script code
        code: String,
    },

    /// Handle with static response (actions array)
    Static {
        /// Actions to execute (actual JSON values, not strings)
        actions: Vec<serde_json::Value>,
    },
}

impl EventHandlerType {
    /// Create a script handler
    pub fn script(language: impl Into<String>, code: impl Into<String>) -> Self {
        EventHandlerType::Script {
            language: language.into(),
            code: code.into(),
        }
    }

    /// Create a static handler
    pub fn static_response(actions: Vec<serde_json::Value>) -> Self {
        EventHandlerType::Static { actions }
    }

    /// Create an LLM handler
    pub fn llm(instruction: impl Into<String>) -> Self {
        EventHandlerType::Llm {
            instruction: instruction.into(),
        }
    }
}

/// Event handler configuration - maps event patterns to handlers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventHandler {
    /// Pattern to match events
    pub event_pattern: EventPattern,

    /// Handler to use for matched events
    pub handler: EventHandlerType,
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(event_pattern: EventPattern, handler: EventHandlerType) -> Self {
        Self {
            event_pattern,
            handler,
        }
    }

    /// Check if this handler matches the given event type ID
    pub fn matches(&self, event_type_id: &str) -> bool {
        self.event_pattern.matches(event_type_id)
    }
}

/// Configuration for all event handlers
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventHandlerConfig {
    /// List of event handlers (processed in order, first match wins)
    pub handlers: Vec<EventHandler>,
}

impl EventHandlerConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add a handler to the configuration
    pub fn add_handler(&mut self, handler: EventHandler) {
        self.handlers.push(handler);
    }

    /// Find the first handler that matches the given event type ID
    pub fn find_handler(&self, event_type_id: &str) -> Option<&EventHandlerType> {
        self.handlers
            .iter()
            .find(|h| h.matches(event_type_id))
            .map(|h| &h.handler)
    }

    /// Check if any handlers are configured
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Get the number of handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }
}
