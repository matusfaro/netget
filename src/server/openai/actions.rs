//! OpenAI protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use tracing::debug;

/// OpenAI protocol action handler
pub struct OpenAiProtocol {}

impl OpenAiProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

impl Server for OpenAiProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::openai::OpenAiServer;
            OpenAiServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                false,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_active_chats_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            openai_chat_response_action(),
            openai_models_response_action(),
            openai_error_response_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "openai_chat_response" => self.execute_openai_chat_response(action),
            "openai_models_response" => self.execute_openai_models_response(action),
            "openai_error_response" => self.execute_openai_error_response(action),
            "list_active_chats" => self.execute_list_active_chats(action),
            _ => Err(anyhow::anyhow!("Unknown OpenAI action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "OpenAI"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_openai_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OPENAI"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["openai"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Alpha
        )
    }
}

impl OpenAiProtocol {
    fn execute_openai_chat_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let content = action
            .get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' field")?;

        let model = action
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        debug!("OpenAI chat response: {} chars, model={}", content.len(), model);

        // Build OpenAI-compatible chat completion response
        let completion_id = format!("chatcmpl-{}", chrono::Utc::now().timestamp());
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let response = json!({
            "id": completion_id,
            "object": "chat.completion",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "total_tokens": 0
            }
        });

        Ok(ActionResult::Custom {
            name: "openai_response".to_string(),
            data: json!({
                "status": 200,
                "headers": vec![("Content-Type".to_string(), "application/json".to_string())],
                "body": response.to_string()
            }),
        })
    }

    fn execute_openai_models_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let models = action
            .get("models")
            .and_then(|v| v.as_array())
            .context("Missing 'models' array")?;

        debug!("OpenAI models response: {} models", models.len());

        // Convert to OpenAI format
        let openai_models: Vec<serde_json::Value> = models
            .iter()
            .map(|model_name| {
                let name = model_name.as_str().unwrap_or("unknown");
                json!({
                    "id": name,
                    "object": "model",
                    "created": 1686935002,
                    "owned_by": "ollama"
                })
            })
            .collect();

        let response = json!({
            "object": "list",
            "data": openai_models
        });

        Ok(ActionResult::Custom {
            name: "openai_response".to_string(),
            data: json!({
                "status": 200,
                "headers": vec![("Content-Type".to_string(), "application/json".to_string())],
                "body": response.to_string()
            }),
        })
    }

    fn execute_openai_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("An error occurred");

        let error_type = action
            .get("error_type")
            .and_then(|v| v.as_str())
            .unwrap_or("server_error");

        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(500) as u16;

        debug!("OpenAI error response: {} ({})", message, status);

        let response = json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": "error"
            }
        });

        Ok(ActionResult::Custom {
            name: "openai_response".to_string(),
            data: json!({
                "status": status,
                "headers": vec![("Content-Type".to_string(), "application/json".to_string())],
                "body": response.to_string()
            }),
        })
    }

    fn execute_list_active_chats(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("OpenAI list active chats");

        // This is a placeholder - in a real implementation, we'd track chat sessions
        Ok(ActionResult::Custom {
            name: "list_active_chats".to_string(),
            data: json!({"chats": []}),
        })
    }
}

/// Action definition: Send OpenAI chat completion response
pub fn openai_chat_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "openai_chat_response".to_string(),
        description: "Send a chat completion response in OpenAI format".to_string(),
        parameters: vec![
            Parameter {
                name: "content".to_string(),
                type_hint: "string".to_string(),
                description: "The assistant's response text".to_string(),
                required: true,
            },
            Parameter {
                name: "model".to_string(),
                type_hint: "string".to_string(),
                description: "Model name (e.g., 'gpt-3.5-turbo')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "openai_chat_response",
            "content": "Hello! How can I help you today?",
            "model": "gpt-3.5-turbo"
        }),
    }
}

/// Action definition: Send models list response
pub fn openai_models_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "openai_models_response".to_string(),
        description: "Respond to GET /v1/models with available models".to_string(),
        parameters: vec![Parameter {
            name: "models".to_string(),
            type_hint: "array".to_string(),
            description: "Array of model names (e.g., ['gpt-3.5-turbo', 'gpt-4'])".to_string(),
            required: true,
        }],
        example: json!({
            "type": "openai_models_response",
            "models": ["gpt-3.5-turbo", "gpt-4"]
        }),
    }
}

/// Action definition: Send error response
pub fn openai_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "openai_error_response".to_string(),
        description: "Send an error response in OpenAI format".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
            Parameter {
                name: "error_type".to_string(),
                type_hint: "string".to_string(),
                description: "Error type (e.g., 'invalid_request_error', 'server_error')".to_string(),
                required: false,
            },
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 500)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "openai_error_response",
            "message": "Invalid API key",
            "error_type": "invalid_request_error",
            "status": 401
        }),
    }
}

/// Action definition: List active chat sessions
pub fn list_active_chats_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_active_chats".to_string(),
        description: "List currently active chat sessions".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_active_chats"
        }),
    }
}

/// Get OpenAI event types
pub fn get_openai_event_types() -> Vec<EventType> {
    vec![
        EventType::new(
            "openai_request",
            "OpenAI API request received"
        )
        .with_parameters(vec![
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "HTTP method (GET, POST)".to_string(),
                required: true,
            },
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Request path (/v1/models, /v1/chat/completions)".to_string(),
                required: true,
            },
        ]),
    ]
}
