//! HTTP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;

/// HTTP protocol action handler
pub struct HttpProtocol;

impl HttpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for HttpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // HTTP has no async actions - it's purely request-response
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_http_response_action()]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_http_response" => self.execute_send_http_response(action),
            _ => Err(anyhow::anyhow!("Unknown HTTP action: {action_type}")),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "HTTP"
    }
}

impl HttpProtocol {
    /// Execute send_http_response sync action
    fn execute_send_http_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action
            .get("status")
            .and_then(|v| v.as_u64())
            .context("Missing or invalid 'status' parameter")? as u16;

        let headers = action
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .context("Missing 'body' parameter")?;

        // For HTTP, we need to return structured data
        // The caller will handle converting this to an actual HTTP response
        // For now, we'll encode this as JSON in the output
        let response_data = json!({
            "status": status,
            "headers": headers,
            "body": body
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize HTTP response")?,
        ))
    }
}

/// Action definition for send_http_response (sync)
fn send_http_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_http_response".to_string(),
        description: "Send an HTTP response to the current request".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (e.g., 200, 404, 500)".to_string(),
                required: true,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Response headers as key-value pairs".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Response body".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_http_response",
            "status": 200,
            "headers": {
                "Content-Type": "text/html"
            },
            "body": "<html><body>Hello World</body></html>"
        }),
    }
}
