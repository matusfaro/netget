//! Ollama protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// Ollama generate request event
pub static OLLAMA_GENERATE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ollama_generate_request", "Received /api/generate request", json!({"type": "placeholder", "event_id": "ollama_generate_request"})).with_parameters(
        vec![
            Parameter {
                name: "model".to_string(),
                type_hint: "string".to_string(),
                description: "Model name requested".to_string(),
                required: true,
            },
            Parameter {
                name: "prompt".to_string(),
                type_hint: "string".to_string(),
                description: "Prompt text".to_string(),
                required: true,
            },
            Parameter {
                name: "stream".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether streaming is requested".to_string(),
                required: false,
            },
        ],
    )
});

/// Ollama chat request event
pub static OLLAMA_CHAT_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ollama_chat_request", "Received /api/chat request", json!({"type": "placeholder", "event_id": "ollama_chat_request"})).with_parameters(vec![
        Parameter {
            name: "model".to_string(),
            type_hint: "string".to_string(),
            description: "Model name requested".to_string(),
            required: true,
        },
        Parameter {
            name: "messages".to_string(),
            type_hint: "array".to_string(),
            description: "Chat messages".to_string(),
            required: true,
        },
        Parameter {
            name: "stream".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether streaming is requested".to_string(),
            required: false,
        },
    ])
});

/// Ollama models request event
pub static OLLAMA_MODELS_REQUEST_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("ollama_models_request", "Received /api/tags request", json!({"type": "placeholder", "event_id": "ollama_models_request"})));

/// Ollama protocol action handler
pub struct OllamaProtocol {}

impl OllamaProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OllamaProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ollama_generate_response_action(),
            ollama_chat_response_action(),
            ollama_models_response_action(),
            ollama_error_response_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "Ollama"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_ollama_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OLLAMA"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ollama", "llm", "ai"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper with Ollama-compatible HTTP endpoints")
            .llm_control("LLM controls responses to API requests (generate, chat, embeddings)")
            .e2e_testing("ollama Python library and reqwest Rust client")
            .notes("Mock Ollama API server for testing and honeypot purposes")
            .build()
    }

    fn description(&self) -> &'static str {
        "Ollama-compatible API server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an Ollama-compatible API server on port 11435"
    }

    fn group_name(&self) -> &'static str {
        "AI & API"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: instruction-based
            json!({
                "type": "open_server",
                "port": 11435,
                "base_stack": "ollama",
                "instruction": "Ollama-compatible API server. Respond to /api/generate and /api/chat requests with helpful LLM responses, and list models on /api/tags"
            }),
            // Script mode: event_handlers with script handler
            json!({
                "type": "open_server",
                "port": 11435,
                "base_stack": "ollama",
                "event_handlers": [{
                    "event_pattern": "ollama_chat_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "model = event.get('model', 'llama2')\naction('ollama_chat_response', message_content=f'Hello from {model}!')"
                    }
                }]
            }),
            // Static mode: event_handlers with static actions
            json!({
                "type": "open_server",
                "port": 11435,
                "base_stack": "ollama",
                "event_handlers": [{
                    "event_pattern": "ollama_models_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "ollama_models_response",
                            "models": ["llama2", "codellama", "mistral"]
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for OllamaProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ollama::OllamaServer;
            OllamaServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                false,
                ctx.server_id,
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
            "ollama_generate_response" => self.execute_ollama_generate_response(action),
            "ollama_chat_response" => self.execute_ollama_chat_response(action),
            "ollama_models_response" => self.execute_ollama_models_response(action),
            "ollama_error_response" => self.execute_ollama_error_response(action),
            _ => Err(anyhow::anyhow!("Unknown Ollama action: {}", action_type)),
        }
    }
}

impl OllamaProtocol {
    fn execute_ollama_generate_response(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Execute: ollama_generate_response");
        Ok(ActionResult::Custom {
            name: "ollama_generate_response".to_string(),
            data: json!({"status": "acknowledged"}),
        })
    }

    fn execute_ollama_chat_response(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Execute: ollama_chat_response");
        Ok(ActionResult::Custom {
            name: "ollama_chat_response".to_string(),
            data: json!({"status": "acknowledged"}),
        })
    }

    fn execute_ollama_models_response(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Execute: ollama_models_response");
        Ok(ActionResult::Custom {
            name: "ollama_models_response".to_string(),
            data: json!({"status": "acknowledged"}),
        })
    }

    fn execute_ollama_error_response(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Execute: ollama_error_response");
        Ok(ActionResult::Custom {
            name: "ollama_error_response".to_string(),
            data: json!({"status": "acknowledged"}),
        })
    }
}

// Action definitions
fn ollama_generate_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ollama_generate_response".to_string(),
        description: "Respond to /api/generate request with LLM-generated text".to_string(),
        parameters: vec![Parameter {
            name: "response_text".to_string(),
            type_hint: "string".to_string(),
            description: "Generated text response".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ollama_generate_response",
            "response_text": "The capital of France is Paris."
        }),
    }
}

fn ollama_chat_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ollama_chat_response".to_string(),
        description: "Respond to /api/chat request with chat message".to_string(),
        parameters: vec![Parameter {
            name: "message_content".to_string(),
            type_hint: "string".to_string(),
            description: "Chat message content".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ollama_chat_response",
            "message_content": "Hello! How can I help you today?"
        }),
    }
}

fn ollama_models_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ollama_models_response".to_string(),
        description: "Respond to /api/tags with list of models".to_string(),
        parameters: vec![Parameter {
            name: "models".to_string(),
            type_hint: "array".to_string(),
            description: "List of model names".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ollama_models_response",
            "models": ["llama2", "codellama", "mistral"]
        }),
    }
}

fn ollama_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ollama_error_response".to_string(),
        description: "Respond with error message".to_string(),
        parameters: vec![Parameter {
            name: "error_message".to_string(),
            type_hint: "string".to_string(),
            description: "Error message to return".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ollama_error_response",
            "error_message": "Model not found"
        }),
    }
}

fn get_ollama_event_types() -> Vec<EventType> {
    vec![
        EventType::new("ollama_generate_request", "Received /api/generate request", json!({"type": "placeholder", "event_id": "ollama_generate_request"}))
            .with_parameters(vec![
                Parameter {
                    name: "model".to_string(),
                    type_hint: "string".to_string(),
                    description: "Model name requested".to_string(),
                    required: true,
                },
                Parameter {
                    name: "prompt".to_string(),
                    type_hint: "string".to_string(),
                    description: "Prompt text".to_string(),
                    required: true,
                },
            ]),
        EventType::new("ollama_chat_request", "Received /api/chat request", json!({"type": "placeholder", "event_id": "ollama_chat_request"})).with_parameters(vec![
            Parameter {
                name: "model".to_string(),
                type_hint: "string".to_string(),
                description: "Model name requested".to_string(),
                required: true,
            },
            Parameter {
                name: "messages".to_string(),
                type_hint: "array".to_string(),
                description: "Chat messages".to_string(),
                required: true,
            },
        ]),
        EventType::new("ollama_models_request", "Received /api/tags request", json!({"type": "placeholder", "event_id": "ollama_models_request"})),
    ]
}
