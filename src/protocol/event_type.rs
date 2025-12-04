//! Event type definitions for protocol-specific events
//!
//! Each protocol defines a set of event types that can trigger LLM calls or script execution.
//! Event types have unique IDs and associated actions that can be used to respond to the event.

use crate::llm::actions::{ActionDefinition, Parameter};
use crate::protocol::log_template::LogTemplate;
use serde_json::Value as JsonValue;

/// Represents a type of event that a protocol can emit
///
/// Events are the triggers for LLM calls or script execution.
/// Each event has a unique ID and a list of actions that can be used to respond.
#[derive(Clone, Debug)]
pub struct EventType {
    /// Unique identifier for this event type (e.g., "http_request", "ssh_auth")
    pub id: String,

    /// Human-readable description of when this event occurs
    pub description: String,

    /// Actions that can be used to respond to this event
    /// These are protocol-specific sync actions
    pub actions: Vec<ActionDefinition>,

    /// Parameters describing the expected structure of event data
    /// This documents what fields should be present in the event data JSON
    /// Uses the same Parameter structure as actions
    pub parameters: Vec<Parameter>,

    /// Required response example for this event
    /// Used in prompt templates to show the expected action response
    /// This MUST use protocol-specific action types (e.g., send_dns_a_response, not send_data)
    ///
    /// This field is required - every event type must have at least one valid response example.
    /// This ensures prompts always show relevant, protocol-specific examples.
    pub response_example: JsonValue,

    /// Optional alternative response examples
    /// Shows other valid ways to respond to this event
    /// Displayed after the primary response_example
    pub alternative_examples: Vec<JsonValue>,

    /// Log template for this event type
    /// Defines protocol-specific log formats at INFO/DEBUG/TRACE levels
    pub log_template: Option<LogTemplate>,
}

impl EventType {
    /// Create a new event type with a required response example
    ///
    /// # Arguments
    /// * `id` - Unique identifier for this event type (e.g., "http_request", "dns_query")
    /// * `description` - Human-readable description of when this event occurs
    /// * `response_example` - Required example showing how to respond to this event
    ///
    /// The response_example MUST use protocol-specific action types (e.g., send_dns_a_response)
    /// not generic actions like send_data. This ensures prompts show relevant examples.
    ///
    /// # Example
    /// ```rust,ignore
    /// EventType::new(
    ///     "dns_query",
    ///     "DNS query received from client",
    ///     json!({"type": "send_dns_a_response", "query_id": 0, "domain": "example.com", "ip": "1.2.3.4"})
    /// )
    /// ```
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        response_example: JsonValue,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            actions: Vec::new(),
            parameters: Vec::new(),
            response_example,
            alternative_examples: Vec::new(),
            log_template: None,
        }
    }

    /// Add an action to this event type
    pub fn with_action(mut self, action: ActionDefinition) -> Self {
        self.actions.push(action);
        self
    }

    /// Add multiple actions to this event type
    pub fn with_actions(mut self, actions: Vec<ActionDefinition>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// Add a parameter describing expected event data field
    pub fn with_parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(parameter);
        self
    }

    /// Add multiple parameters describing expected event data
    pub fn with_parameters(mut self, parameters: Vec<Parameter>) -> Self {
        self.parameters.extend(parameters);
        self
    }

    /// Add an alternative response example for this event
    ///
    /// This shows another valid way to respond to this event.
    /// The primary response_example is set in the constructor;
    /// use this method to add additional alternatives.
    ///
    /// # Example
    /// ```rust,ignore
    /// .with_alternative_example(json!({
    ///     "type": "disconnect"
    /// }))
    /// ```
    pub fn with_alternative_example(mut self, example: JsonValue) -> Self {
        self.alternative_examples.push(example);
        self
    }

    /// Add a log template for this event type
    ///
    /// The log template defines protocol-specific log formats at INFO/DEBUG/TRACE levels.
    /// This enables standardized, centralized logging without per-protocol logging code.
    ///
    /// # Example
    /// ```rust,ignore
    /// EventType::new("http_request", "HTTP request received", json!({...}))
    ///     .with_log_template(
    ///         LogTemplate::new()
    ///             .with_info("{client_ip} {method} {path} -> {status}")
    ///             .with_debug("HTTP {method} {path} from {client_ip}:{client_port}")
    ///             .with_trace("HTTP request: {json_pretty(.)}")
    ///     )
    /// ```
    pub fn with_log_template(mut self, template: LogTemplate) -> Self {
        self.log_template = Some(template);
        self
    }

    /// Get action names for this event type
    pub fn action_names(&self) -> Vec<String> {
        self.actions.iter().map(|a| a.name.clone()).collect()
    }

    /// Convert this event type to a prompt description
    ///
    /// This creates a formatted string that describes the event and what actions
    /// are available to respond to it. Used in LLM prompts.
    ///
    /// # Returns
    /// A formatted string describing the event type, its context, and available actions
    pub fn to_prompt_description(&self) -> String {
        let mut result = String::new();

        // Event type header
        result.push_str(&format!("Event Type: {}\n", self.id));
        result.push_str(&format!("Description: {}\n\n", self.description));

        // Event input parameters (if available)
        if !self.parameters.is_empty() {
            result.push_str("Event Input Data:\n");
            for param in &self.parameters {
                result.push_str(&format!(
                    "  - {} ({}){}: {}\n",
                    param.name,
                    param.type_hint,
                    if param.required {
                        ", required"
                    } else {
                        ", optional"
                    },
                    param.description
                ));
            }
            result.push('\n');
        }

        // Available actions for this event
        if !self.actions.is_empty() {
            result.push_str("Available actions for this event:\n\n");
            for (i, action) in self.actions.iter().enumerate() {
                result.push_str(&format!("{}. {}\n\n", i + 1, action.to_prompt_text()));
            }
        } else {
            result.push_str("No specific actions available for this event.\n");
        }

        result
    }
}

/// Represents a specific event instance with type and data
///
/// This combines an EventType (which defines what can happen) with
/// the actual event data (what did happen). It's the complete package
/// that gets passed to call_llm().
///
/// # Example
/// ```rust,ignore
/// // Create an event instance for HTTP request
/// let event = Event::new(
///     &HTTP_REQUEST_EVENT,  // EventType constant
///     json!({
///         "method": "GET",
///         "path": "/api/users",
///         "headers": {"User-Agent": "curl/7.0"}
///     })
/// );
///
/// call_llm(&llm_client, &state, server_id, conn_id, &event, &protocol).await?;
/// ```
#[derive(Clone, Debug)]
pub struct Event {
    /// The type of event (reference to EventType constant)
    pub event_type: &'static EventType,

    /// The event-specific data (e.g., HTTP headers, SSH username, etc.)
    pub data: JsonValue,
}

impl Event {
    /// Create a new event instance
    ///
    /// # Arguments
    /// * `event_type` - Reference to the EventType constant
    /// * `data` - JSON data with event-specific context
    ///
    /// # Example
    /// ```rust,ignore
    /// let event = Event::new(
    ///     &SSH_AUTH_EVENT,
    ///     json!({"username": "alice", "auth_type": "password"})
    /// );
    /// ```
    pub fn new(event_type: &'static EventType, data: JsonValue) -> Self {
        Self { event_type, data }
    }

    /// Get the event type ID (for script routing)
    pub fn id(&self) -> &str {
        &self.event_type.id
    }

    /// Get the event description for prompts
    pub fn to_prompt_description(&self) -> String {
        self.event_type.to_prompt_description()
    }
}

/// Format event types for inclusion in LLM prompts
pub fn format_event_types_for_prompt(event_types: &[EventType]) -> String {
    if event_types.is_empty() {
        return String::new();
    }

    let mut result = String::from("\nEVENT TYPES:\n");
    result.push_str("This protocol can emit the following event types:\n\n");

    for event_type in event_types {
        result.push_str(&format!(
            "• {} - {}\n",
            event_type.id, event_type.description
        ));
        result.push_str(&format!(
            "  Available actions: {}\n",
            event_type.action_names().join(", ")
        ));
    }

    result.push('\n');
    result
}

/// Generate script template instructions for event types
pub fn format_script_template_for_prompt(event_types: &[EventType]) -> String {
    if event_types.is_empty() {
        return String::new();
    }

    let event_ids: Vec<String> = event_types
        .iter()
        .map(|e| format!("\"{}\"", e.id))
        .collect();

    format!(
        r#"
SCRIPT TEMPLATE for this protocol:
When creating a script, structure it with a switch/case on the event type:

Python example:
import json, sys
data = json.load(sys.stdin)
event_type = data['event_type_id']

if event_type == "event_id_1":
    # Handle this event type
    result = {{"actions": [{{"type": "action_name", "param": value}}]}}
elif event_type == "event_id_2":
    # Handle another event type
    result = {{"actions": [{{"type": "other_action", "param": value}}]}}
else:
    # Unknown event - fallback to LLM
    result = {{"fallback_to_llm": true, "fallback_reason": "Unknown event type"}}

print(json.dumps(result))

JavaScript example:
const data = JSON.parse(require('fs').readFileSync(0, 'utf-8'));
const eventType = data.event_type_id;
let result;

switch (eventType) {{
  case "event_id_1":
    result = {{"actions": [{{"type": "action_name", "param": value}}]}};
    break;
  case "event_id_2":
    result = {{"actions": [{{"type": "other_action", "param": value}}]}};
    break;
  default:
    result = {{"fallback_to_llm": true, "fallback_reason": "Unknown event type"}};
}}

console.log(JSON.stringify(result));

Event types for this protocol: {}
"#,
        event_ids.join(", ")
    )
}
