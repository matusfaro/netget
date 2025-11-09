//! Default instruction registry for all protocols
//!
//! This module provides default instructions for each protocol's events.
//! These are simple, general instructions that can be overridden.

use crate::llm::event_instructions::EventInstructions;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Global registry of default instructions
pub static DEFAULT_INSTRUCTIONS: Lazy<DefaultInstructionsRegistry> =
    Lazy::new(DefaultInstructionsRegistry::new);

/// Registry containing default instructions for all event types
pub struct DefaultInstructionsRegistry {
    instructions: HashMap<String, EventInstructions>,
}

impl Default for DefaultInstructionsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultInstructionsRegistry {
    /// Create and populate the default registry
    pub fn new() -> Self {
        let mut instructions = HashMap::new();

        // HTTP defaults
        instructions.insert(
            "http_request".to_string(),
            EventInstructions::new(
                "Parse the HTTP request and return an appropriate response based on the server's instruction.",
            )
            .with_example(
                "GET / HTTP/1.1",
                r#"{"type": "send_http_response", "status": 200, "body": "Hello World"}"#,
            )
            .with_example(
                "POST /api/data",
                r#"{"type": "send_http_response", "status": 200, "headers": {"Content-Type": "application/json"}, "body": "{\"status\": \"success\"}"}"#,
            ),
        );

        // SSH defaults
        instructions.insert(
            "ssh_auth".to_string(),
            EventInstructions::new(
                "Handle SSH authentication. Accept or reject based on the server's instruction.",
            )
            .with_example(
                r#"{"username": "admin", "auth_type": "password"}"#,
                r#"{"type": "ssh_auth_decision", "accept": true}"#,
            ),
        );

        instructions.insert(
            "ssh_command".to_string(),
            EventInstructions::new(
                "Execute or simulate the SSH command based on the server's instruction.",
            )
            .with_example(
                "ls -la",
                r#"{"type": "ssh_send_output", "output": "total 24\ndrwxr-xr-x  3 user user 4096 Jan 1 00:00 .\n"}"#,
            ),
        );

        // TCP defaults
        instructions.insert(
            "tcp_data_received".to_string(),
            EventInstructions::new(
                "Process the incoming TCP data and respond according to the server's instruction.",
            )
            .with_example(
                "HELLO",
                r#"{"type": "send_tcp_data", "data": "HELLO BACK"}"#,
            ),
        );

        // DNS defaults
        instructions.insert(
            "dns_query".to_string(),
            EventInstructions::new(
                "Resolve the DNS query based on the server's instruction.",
            )
            .with_example(
                r#"{"query": "example.com", "query_type": "A"}"#,
                r#"{"type": "dns_response", "answers": [{"name": "example.com", "type": "A", "data": "93.184.216.34"}]}"#,
            ),
        );

        // WebSocket defaults
        instructions.insert(
            "websocket_message".to_string(),
            EventInstructions::new(
                "Handle the WebSocket message and respond as configured.",
            )
            .with_example(
                r#"{"type": "text", "data": "ping"}"#,
                r#"{"type": "send_websocket_message", "message": "pong"}"#,
            ),
        );

        // MQTT defaults
        instructions.insert(
            "mqtt_publish".to_string(),
            EventInstructions::new(
                "Handle MQTT publish message according to the server's configuration.",
            )
            .with_example(
                r#"{"topic": "sensors/temp", "payload": "22.5"}"#,
                r#"{"type": "mqtt_publish", "topic": "sensors/temp/ack", "payload": "received"}"#,
            ),
        );

        // Redis defaults
        instructions.insert(
            "redis_command".to_string(),
            EventInstructions::new(
                "Execute the Redis command and return appropriate response.",
            )
            .with_example(
                "GET key1",
                r#"{"type": "redis_response", "response": "value1"}"#,
            )
            .with_example(
                "SET key2 value2",
                r#"{"type": "redis_response", "response": "OK"}"#,
            ),
        );

        // MySQL defaults
        instructions.insert(
            "mysql_query".to_string(),
            EventInstructions::new(
                "Execute or simulate the MySQL query based on server configuration.",
            )
            .with_example(
                "SELECT * FROM users",
                r#"{"type": "mysql_result", "rows": [{"id": 1, "name": "Alice"}]}"#,
            ),
        );

        // S3 defaults
        instructions.insert(
            "s3_request".to_string(),
            EventInstructions::new(
                "Handle S3 API request according to the server's configuration.",
            )
            .with_example(
                r#"{"method": "GET", "bucket": "mybucket", "key": "file.txt"}"#,
                r#"{"type": "s3_response", "status": 200, "body": "file contents"}"#,
            ),
        );

        Self { instructions }
    }

    /// Get default instructions for an event type
    pub fn get(&self, event_type: &str) -> Option<&EventInstructions> {
        self.instructions.get(event_type)
    }

    /// Get all registered event types
    pub fn event_types(&self) -> Vec<&str> {
        self.instructions.keys().map(|s| s.as_str()).collect()
    }

    /// Add or update default instructions (useful for testing)
    pub fn register(&mut self, event_type: String, instructions: EventInstructions) {
        self.instructions.insert(event_type, instructions);
    }
}

/// Resolve instructions for an event using the priority chain
///
/// Priority order:
/// 1. Script (if configured)
/// 2. Event-specific override
/// 3. Default instructions
/// 4. Global instructions only
pub fn resolve_instructions(
    event_type: &str,
    config: &crate::llm::event_instructions::ServerInstructionConfig,
) -> crate::llm::event_instructions::InstructionSource {
    use crate::llm::event_instructions::InstructionSource;

    // Check for script first (highest priority)
    if config.has_script_for_event(event_type) {
        return InstructionSource::Script(format!("Script configured for {}", event_type));
    }

    // Check for event override
    if let Some(override_instructions) = config.get_instructions_for_event(event_type) {
        return InstructionSource::EventOverride(override_instructions.clone());
    }

    // Check for defaults
    if let Some(default_instructions) = DEFAULT_INSTRUCTIONS.get(event_type) {
        return InstructionSource::Default(default_instructions.clone());
    }

    // Fall back to global instructions
    if let Some(global) = &config.global_instructions {
        return InstructionSource::GlobalOnly(global.clone());
    }

    InstructionSource::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry() {
        let registry = DefaultInstructionsRegistry::new();

        // Check some defaults exist
        assert!(registry.get("http_request").is_some());
        assert!(registry.get("ssh_auth").is_some());
        assert!(registry.get("tcp_data_received").is_some());

        // Check non-existent
        assert!(registry.get("unknown_event").is_none());
    }

    #[test]
    fn test_instruction_resolution() {
        use crate::llm::event_instructions::{InstructionSource, ServerInstructionConfig};

        let mut config = ServerInstructionConfig::new();
        config.global_instructions = Some("Be helpful".to_string());

        // Test default resolution
        match resolve_instructions("http_request", &config) {
            InstructionSource::Default(_) => {}
            _ => panic!("Expected default instructions"),
        }

        // Test with override
        config.event_overrides.insert(
            "http_request".to_string(),
            EventInstructions::new("Custom HTTP handler"),
        );

        match resolve_instructions("http_request", &config) {
            InstructionSource::EventOverride(_) => {}
            _ => panic!("Expected override instructions"),
        }

        // Test unknown event with global fallback
        match resolve_instructions("unknown_event", &config) {
            InstructionSource::GlobalOnly(_) => {}
            _ => panic!("Expected global instructions"),
        }
    }
}