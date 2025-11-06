//! HTTP client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// HTTP client protocol action handler
pub struct HttpClientProtocol;

impl HttpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for HttpClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::http::HttpClient;
            HttpClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "default_headers".to_string(),
                description: "Default headers to include in all requests".to_string(),
                type_hint: "object".to_string(),
                required: false,
                example: Some(json!({
                    "User-Agent": "NetGet/1.0",
                    "Accept": "application/json"
                })),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_http_request".to_string(),
                description: "Send an HTTP request to the server".to_string(),
                parameters: vec![
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "HTTP method (GET, POST, PUT, DELETE, etc.)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request path (e.g., /api/users)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "headers".to_string(),
                        type_hint: "object".to_string(),
                        description: "Request headers".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request body".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_http_request",
                    "method": "GET",
                    "path": "/api/status",
                    "headers": {
                        "Accept": "application/json"
                    }
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the HTTP server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_http_request".to_string(),
                description: "Send another HTTP request in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "HTTP method".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request path".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "headers".to_string(),
                        type_hint: "object".to_string(),
                        description: "Request headers".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request body".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_http_request",
                    "method": "POST",
                    "path": "/api/data",
                    "body": "{\"key\": \"value\"}"
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_http_request" => {
                let method = action
                    .get("method")
                    .and_then(|v| v.as_str())
                    .context("Missing 'method' field")?
                    .to_string();

                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing 'path' field")?
                    .to_string();

                let headers = action
                    .get("headers")
                    .and_then(|v| v.as_object())
                    .cloned();

                let body = action
                    .get("body")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Return custom result with request data
                Ok(ClientActionResult::Custom {
                    name: "http_request".to_string(),
                    data: json!({
                        "method": method,
                        "path": path,
                        "headers": headers,
                        "body": body,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown HTTP client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "HTTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "http_connected".to_string(),
                description: "Triggered when HTTP client is initialized".to_string(),
                available_actions: vec!["send_http_request".to_string()],
            },
            EventType {
                id: "http_response_received".to_string(),
                description: "Triggered when HTTP client receives a response".to_string(),
                available_actions: vec!["send_http_request".to_string()],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["http", "http client", "connect to http", "https"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("reqwest HTTP client library")
            .llm_control("Full control over requests (method, path, headers, body)")
            .e2e_testing("httpbin.org or local HTTP server")
            .build()
    }

    fn description(&self) -> &'static str {
        "HTTP client for making web requests"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to http://example.com and fetch /api/status"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}
