//! HTTP/2 protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// HTTP/2 protocol action handler
pub struct Http2Protocol;

impl Http2Protocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Http2Protocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        crate::server::tls_cert_manager::get_tls_startup_parameters()
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // HTTP/2 has no async actions - it's purely request-response
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_http2_response_action(), push_resource_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "HTTP2"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_http2_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP/2"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "http2",
            "http/2",
            "http 2",
            "http2 server",
            "http/2 server",
            "via http2",
            "via http/2",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.0 HTTP/2 server library")
            .llm_control("Response content (status, headers, body)")
            .e2e_testing("reqwest HTTP/2 client - 6 LLM calls")
            .build()
    }
    fn description(&self) -> &'static str {
        "Web server serving HTTP/2 traffic with multiplexing and header compression"
    }
    fn example_prompt(&self) -> &'static str {
        "HTTP/2 server on port 8443 serving JSON API with fast multiplexed responses"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for Http2Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::http2::H2Server;

            // Parse TLS configuration from startup_params
            let tls_config = if let Some(ref params) = ctx.startup_params {
                match crate::server::tls_cert_manager::extract_tls_config_from_params(params) {
                    Ok(config) => config,
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to create TLS config: {}", e));
                    }
                }
            } else {
                None
            };

            // Use h2-based server for full server push support
            H2Server::spawn_with_push_support(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                tls_config,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_http2_response" => self.execute_send_http2_response(action),
            "push_resource" => self.execute_push_resource(action),
            _ => Err(anyhow::anyhow!("Unknown HTTP/2 action: {action_type}")),
        }
    }
}

impl Http2Protocol {
    /// Execute send_http2_response sync action
    fn execute_send_http2_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Use shared action execution logic
        crate::server::http_common::execute_http_response_action(action)
    }

    /// Execute push_resource sync action (server push)
    fn execute_push_resource(&self, action: serde_json::Value) -> Result<ActionResult> {
        use anyhow::Context;
        use serde_json::json;

        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;

        let headers = action
            .get("headers")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let body = action.get("body").and_then(|v| v.as_str()).unwrap_or("");

        let method = action
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");

        // Return push data as structured JSON that h2_server will recognize
        let push_data = json!({
            "_push_directive": true,
            "path": path,
            "method": method,
            "status": status,
            "headers": headers,
            "body": body
        });

        tracing::debug!("Queued server push for {}", path);

        Ok(ActionResult::Output(
            serde_json::to_vec(&push_data).context("Failed to serialize push data")?,
        ))
    }
}

/// Action definition for send_http2_response (sync)
fn send_http2_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_http2_response".to_string(),
        description: "Send an HTTP/2 response to the current request".to_string(),
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
            "type": "send_http2_response",
            "status": 200,
            "headers": {
                "Content-Type": "application/json"
            },
            "body": "{\"message\": \"Hello from HTTP/2!\"}"
        }),
    }
}

/// Action definition for push_resource (sync) - HTTP/2 server push
fn push_resource_action() -> ActionDefinition {
    ActionDefinition {
        name: "push_resource".to_string(),
        description: "Push a resource to the client proactively (HTTP/2 server push - not yet fully implemented)".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Resource path to push (e.g., /style.css)".to_string(),
                required: true,
            },
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 200)".to_string(),
                required: false,
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
                description: "Resource content to push".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "push_resource",
            "path": "/style.css",
            "status": 200,
            "headers": {
                "Content-Type": "text/css"
            },
            "body": "body { margin: 0; }"
        }),
    }
}

// ============================================================================
// HTTP/2 Action Constants
// ============================================================================

pub static SEND_HTTP2_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_http2_response_action());
pub static PUSH_RESOURCE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| push_resource_action());

// ============================================================================
// HTTP/2 Event Type Constants
// ============================================================================

/// HTTP/2 request event - triggered when client sends an HTTP/2 request
pub static HTTP2_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("http2_request", "HTTP/2 request received from client", json!({"type": "placeholder", "event_id": "http2_request"}))
        .with_parameters(vec![
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "HTTP method (GET, POST, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "uri".to_string(),
                type_hint: "string".to_string(),
                description: "Request URI".to_string(),
                required: true,
            },
            Parameter {
                name: "version".to_string(),
                type_hint: "string".to_string(),
                description: "HTTP version (HTTP/2.0)".to_string(),
                required: true,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Request headers as key-value pairs".to_string(),
                required: true,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Request body".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            SEND_HTTP2_RESPONSE_ACTION.clone(),
            PUSH_RESOURCE_ACTION.clone(),
        ])
});

/// Get HTTP/2 event types
pub fn get_http2_event_types() -> Vec<EventType> {
    vec![HTTP2_REQUEST_EVENT.clone()]
}
