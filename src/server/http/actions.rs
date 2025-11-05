//! HTTP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// HTTP protocol action handler
pub struct HttpProtocol;

impl HttpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for HttpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::http::HttpServer;
            HttpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

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

    fn get_event_types(&self) -> Vec<EventType> {
        get_http_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["http", "http server", "http stack", "via http", "hyper"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .implementation("hyper v1.0 web server library")
            .llm_control("Response content (status, headers, body)")
            .e2e_testing("reqwest HTTP client - 14 LLM calls")
            .build()
    }

    fn description(&self) -> &'static str {
        "Web server serving HTTP traffic"
    }

    fn example_prompt(&self) -> &'static str {
        "Pretend to be a sassy HTTP server on port 8080 serving cooking recipes"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

impl HttpProtocol {
    /// Execute send_http_response sync action
    fn execute_send_http_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Use shared action execution logic
        crate::server::http_common::execute_http_response_action(action)
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

// ============================================================================
// HTTP Action Constants
// ============================================================================

pub static SEND_HTTP_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_http_response_action());

// ============================================================================
// HTTP Event Type Constants
// ============================================================================

/// HTTP request event - triggered when client sends an HTTP request
pub static HTTP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "http_request",
        "HTTP request received from client"
    )
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
        SEND_HTTP_RESPONSE_ACTION.clone(),
    ])
});

/// Get HTTP event types
pub fn get_http_event_types() -> Vec<EventType> {
    vec![
        HTTP_REQUEST_EVENT.clone(),
    ]
}
