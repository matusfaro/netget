//! OpenAI client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OpenAI client connected event
pub static OPENAI_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openai_connected",
        "OpenAI client initialized and ready to make API requests"
    )
    .with_parameters(vec![
        Parameter {
            name: "api_endpoint".to_string(),
            type_hint: "string".to_string(),
            description: "OpenAI API endpoint URL".to_string(),
            required: true,
        },
    ])
});

/// OpenAI client response received event
pub static OPENAI_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openai_response_received",
        "Response received from OpenAI API"
    )
    .with_parameters(vec![
        Parameter {
            name: "response_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of response (chat_completion, embedding, etc.)".to_string(),
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
        Parameter {
            name: "usage".to_string(),
            type_hint: "object".to_string(),
            description: "Token usage statistics".to_string(),
            required: false,
        },
    ])
});

/// OpenAI client protocol action handler
pub struct OpenAiClientProtocol;

impl OpenAiClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for OpenAiClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::openai::OpenAiClient;
            OpenAiClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            )
            .await
        })
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "api_key".to_string(),
                description: "OpenAI API key for authentication".to_string(),
                type_hint: "string".to_string(),
                required: true,
                example: json!("sk-..."),
            },
            ParameterDefinition {
                name: "default_model".to_string(),
                description: "Default model to use for requests".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("gpt-4"),
            },
            ParameterDefinition {
                name: "organization".to_string(),
                description: "OpenAI organization ID".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("org-..."),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_chat_completion".to_string(),
                description: "Send a chat completion request to OpenAI".to_string(),
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
                        description: "Model to use (e.g., gpt-4, gpt-3.5-turbo)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "temperature".to_string(),
                        type_hint: "number".to_string(),
                        description: "Sampling temperature (0.0 to 2.0)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "max_tokens".to_string(),
                        type_hint: "number".to_string(),
                        description: "Maximum tokens to generate".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "functions".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of function definitions for function calling".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_chat_completion",
                    "messages": [
                        {"role": "user", "content": "Hello!"}
                    ],
                    "model": "gpt-3.5-turbo"
                }),
            },
            ActionDefinition {
                name: "send_embedding_request".to_string(),
                description: "Generate embeddings for text".to_string(),
                parameters: vec![
                    Parameter {
                        name: "input".to_string(),
                        type_hint: "string or array".to_string(),
                        description: "Text or array of texts to embed".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "model".to_string(),
                        type_hint: "string".to_string(),
                        description: "Embedding model to use (e.g., text-embedding-ada-002)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_embedding_request",
                    "input": "The quick brown fox",
                    "model": "text-embedding-ada-002"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close the OpenAI client connection".to_string(),
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
                name: "send_chat_completion".to_string(),
                description: "Send another chat completion in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "messages".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of message objects".to_string(),
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
                    "type": "send_chat_completion",
                    "messages": [
                        {"role": "user", "content": "Follow-up question"}
                    ]
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
            "send_chat_completion" => {
                let messages = action
                    .get("messages")
                    .context("Missing 'messages' field")?
                    .clone();

                let model = action
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let temperature = action
                    .get("temperature")
                    .and_then(|v| v.as_f64());

                let max_tokens = action
                    .get("max_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);

                let functions = action
                    .get("functions")
                    .cloned();

                Ok(ClientActionResult::Custom {
                    name: "openai_chat_completion".to_string(),
                    data: json!({
                        "messages": messages,
                        "model": model,
                        "temperature": temperature,
                        "max_tokens": max_tokens,
                        "functions": functions,
                    }),
                })
            }
            "send_embedding_request" => {
                let input = action
                    .get("input")
                    .context("Missing 'input' field")?
                    .clone();

                let model = action
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "openai_embedding".to_string(),
                    data: json!({
                        "input": input,
                        "model": model,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown OpenAI client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "OpenAI"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "openai_connected".to_string(),
                description: "Triggered when OpenAI client is initialized".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "openai_response_received".to_string(),
                description: "Triggered when OpenAI client receives a response".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>TLS>HTTPS>OpenAI"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["openai", "openai client", "gpt", "chatgpt", "openai api"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("async-openai library for OpenAI API access")
            .llm_control("Full control over chat completions, embeddings, and function calling")
            .e2e_testing("OpenAI API with test key or mock server")
            .build()
    }

    fn description(&self) -> &'static str {
        "OpenAI API client for LLM interactions"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to OpenAI and ask GPT-4 to explain quantum computing"
    }

    fn group_name(&self) -> &'static str {
        "AI & API"
    }
}
