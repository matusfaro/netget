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
    Llm,

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
    pub fn llm() -> Self {
        EventHandlerType::Llm
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_pattern_matching() {
        let specific = EventPattern::specific("tcp_connected");
        assert!(specific.matches("tcp_connected"));
        assert!(!specific.matches("tcp_data_received"));

        let wildcard = EventPattern::wildcard();
        assert!(wildcard.matches("tcp_connected"));
        assert!(wildcard.matches("tcp_data_received"));
        assert!(wildcard.matches("anything"));
    }

    #[test]
    fn test_event_pattern_from_string() {
        let pattern: EventPattern = "*".into();
        assert_eq!(pattern, EventPattern::Wildcard);

        let pattern: EventPattern = "all".into();
        assert_eq!(pattern, EventPattern::Wildcard);

        let pattern: EventPattern = "tcp_connected".into();
        assert_eq!(pattern, EventPattern::Specific("tcp_connected".to_string()));
    }

    #[test]
    fn test_event_handler_matching() {
        let handler = EventHandler::new(
            EventPattern::specific("tcp_connected"),
            EventHandlerType::llm(),
        );

        assert!(handler.matches("tcp_connected"));
        assert!(!handler.matches("tcp_data_received"));
    }

    #[test]
    fn test_event_handler_config_first_match() {
        let mut config = EventHandlerConfig::new();

        // Add specific handler first
        config.add_handler(EventHandler::new(
            EventPattern::specific("tcp_connected"),
            EventHandlerType::script("python", "print('hello')"),
        ));

        // Add wildcard handler second
        config.add_handler(EventHandler::new(
            EventPattern::wildcard(),
            EventHandlerType::llm(),
        ));

        // Should match the specific handler
        let handler = config.find_handler("tcp_connected");
        assert!(matches!(handler, Some(EventHandlerType::Script { .. })));

        // Should match the wildcard handler
        let handler = config.find_handler("tcp_data_received");
        assert!(matches!(handler, Some(EventHandlerType::Llm)));
    }

    #[test]
    fn test_event_handler_serialization() {
        let handler = EventHandler::new(
            EventPattern::specific("tcp_connected"),
            EventHandlerType::script("python", "print('hello')"),
        );

        let json = serde_json::to_value(&handler).unwrap();
        assert_eq!(json["event_pattern"], "tcp_connected");
        assert_eq!(json["handler"]["type"], "script");
        assert_eq!(json["handler"]["language"], "python");
        assert_eq!(json["handler"]["code"], "print('hello')");
    }

    #[test]
    fn test_static_handler_with_actions() {
        let handler = EventHandler::new(
            EventPattern::wildcard(),
            EventHandlerType::static_response(vec![
                json!({"type": "send_data", "data": "hello"}),
                json!({"type": "disconnect"}),
            ]),
        );

        let json = serde_json::to_value(&handler).unwrap();
        assert_eq!(json["handler"]["type"], "static");
        assert!(json["handler"]["actions"].is_array());
        assert_eq!(json["handler"]["actions"][0]["type"], "send_data");
    }
}
