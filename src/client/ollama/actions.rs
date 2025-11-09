//! Ollama client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::ConnectContext;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// Ollama client connected event
pub static OLLAMA_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ollama_connected",
        "Ollama client initialized and ready to make API requests"
    )
    .with_parameters(vec![
        Parameter {
            name: "api_endpoint".to_string(),
            type_hint: "string".to_string(),
            description: "Ollama API endpoint URL".to_string(),
            required: true,
        },
    ])
});

/// Ollama client response received event
pub static OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ollama_response_received",
        "Response received from Ollama API"
    )
    .with_parameters(vec![
        Parameter {
            name: "response_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of response (generate, chat, models, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "content".to_string(),
            type_hint: "string".to_string(),
            description: "Response content or error message".to_string(),
            required: true,
        },
        Parameter {
            name: "model".to_string(),
            type_hint: "string".to_string(),
            description: "Model used for the request".to_string(),
            required: false,
        },
    ])
});

/// Ollama client protocol action handler
pub struct OllamaClientProtocol;

impl Default for OllamaClientProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OllamaClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "default_model".to_string(),
                description: "Default model to use for requests".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("llama2"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_generate_request".to_string(),
                description: "Send a generation request to Ollama".to_string(),
                parameters: vec![
                    Parameter {
                        name: "prompt".to_string(),
                        type_hint: "string".to_string(),
                        description: "Text prompt for generation".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "model".to_string(),
                        type_hint: "string".to_string(),
                        description: "Model to use (e.g., llama2, codellama)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_generate_request",
                    "prompt": "What is the capital of France?",
                    "model": "llama2"
                }),
            },
            ActionDefinition {
                name: "send_chat_request".to_string(),
                description: "Send a chat request to Ollama".to_string(),
                parameters: vec![
                    Parameter {
                        name: "messages".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of message objects with role and content".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "model".to_string(),
                        type_hint: "string".to_string(),
                        description: "Model to use".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_chat_request",
                    "messages": [
                        {"role": "user", "content": "Hello!"}
                    ],
                    "model": "llama2"
                }),
            },
            ActionDefinition {
                name: "list_models".to_string(),
                description: "List available models from Ollama".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_models"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close the Ollama client connection".to_string(),
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
                name: "send_generate_request".to_string(),
                description: "Send another generation request in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "prompt".to_string(),
                        type_hint: "string".to_string(),
                        description: "Text prompt for generation".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "model".to_string(),
                        type_hint: "string".to_string(),
                        description: "Model to use".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_generate_request",
                    "prompt": "Tell me more"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more responses without taking action".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from Ollama".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "Ollama"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            OLLAMA_CLIENT_CONNECTED_EVENT.clone(),
            OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OLLAMA"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ollama", "llm", "ai"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("reqwest HTTP client with Ollama API")
            .llm_control("LLM decides when to send requests and what to do with responses")
            .e2e_testing("Ollama API server or mock server")
            .notes("Client for Ollama HTTP API endpoints")
            .build()
    }

    fn description(&self) -> &'static str {
        "Ollama API client"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to Ollama at http://localhost:11434 and ask it to generate a poem"
    }

    fn group_name(&self) -> &'static str {
        "AI & API"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for OllamaClientProtocol {
    fn connect(&self, ctx: ConnectContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            use crate::client::ollama::OllamaClientImpl;
            OllamaClientImpl::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_generate_request" => {
                let prompt = action
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .context("Missing 'prompt' parameter")?
                    .to_string();

                let model = action
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "send_generate_request".to_string(),
                    data: json!({
                        "prompt": prompt,
                        "model": model,
                    }),
                })
            }
            "send_chat_request" => {
                let messages = action
                    .get("messages")
                    .context("Missing 'messages' parameter")?
                    .clone();

                let model = action
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "send_chat_request".to_string(),
                    data: json!({
                        "messages": messages,
                        "model": model,
                    }),
                })
            }
            "list_models" => {
                Ok(ClientActionResult::Custom {
                    name: "list_models".to_string(),
                    data: json!({}),
                })
            }
            "wait_for_more" => {
                Ok(ClientActionResult::WaitForMore)
            }
            "disconnect" => {
                Ok(ClientActionResult::Disconnect)
            }
            _ => Err(anyhow::anyhow!("Unknown Ollama client action: {}", action_type)),
        }
    }
}
