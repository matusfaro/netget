//! gRPC client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// gRPC client connected event
pub static GRPC_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "grpc_connected",
        "gRPC client initialized and ready to call RPC methods",
    )
    .with_parameters(vec![
        Parameter {
            name: "server_addr".to_string(),
            type_hint: "string".to_string(),
            description: "gRPC server address".to_string(),
            required: true,
        },
        Parameter {
            name: "services".to_string(),
            type_hint: "array".to_string(),
            description: "Available service names from schema".to_string(),
            required: true,
        },
    ])
});

/// gRPC client response received event
pub static GRPC_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "grpc_response_received",
        "gRPC response received from server",
    )
    .with_parameters(vec![
        Parameter {
            name: "service".to_string(),
            type_hint: "string".to_string(),
            description: "Service name".to_string(),
            required: true,
        },
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "Method name".to_string(),
            required: true,
        },
        Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Response message as JSON".to_string(),
            required: true,
        },
    ])
});

/// gRPC client error event
pub static GRPC_CLIENT_ERROR_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("grpc_error", "gRPC error received from server").with_parameters(vec![
        Parameter {
            name: "service".to_string(),
            type_hint: "string".to_string(),
            description: "Service name".to_string(),
            required: true,
        },
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "Method name".to_string(),
            required: true,
        },
        Parameter {
            name: "code".to_string(),
            type_hint: "string".to_string(),
            description: "gRPC status code".to_string(),
            required: true,
        },
        Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Error message".to_string(),
            required: true,
        },
    ])
});

/// gRPC client protocol action handler
pub struct GrpcClientProtocol;

impl GrpcClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for GrpcClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
                ParameterDefinition {
                    name: "proto_schema".to_string(),
                    description: "Protobuf schema definition (base64 FileDescriptorSet, .proto file path, or inline .proto text)".to_string(),
                    type_hint: "string".to_string(),
                    required: true,
                    example: json!("CpUCCg9jYWxjdWxhdG9yLnByb3RvEgpjYWxjdWxhdG9yIikKCkFkZFJlcXVlc3QSCwoDYQgBIAEoBVIBYRILCgNiCAIgASgFUgFiIiIKC0FkZFJlc3BvbnNlEhMKBnJlc3VsdBgBIAEoBVIGcmVzdWx0MkIKCkNhbGN1bGF0b3ISNAoDQWRkEhYuY2FsY3VsYXRvci5BZGRSZXF1ZXN0Gh0uY2FsY3VsYXRvci5BZGRSZXNwb25zZSIAYgZwcm90bzM="),
                },
                ParameterDefinition {
                    name: "use_tls".to_string(),
                    description: "Whether to use TLS for connection (default: false)".to_string(),
                    type_hint: "boolean".to_string(),
                    required: false,
                    example: json!(false),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "call_grpc_method".to_string(),
                description: "Call a gRPC method with the given request".to_string(),
                parameters: vec![
                    Parameter {
                        name: "service".to_string(),
                        type_hint: "string".to_string(),
                        description: "Fully qualified service name (e.g., 'calculator.Calculator')"
                            .to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "Method name (e.g., 'Add')".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "request".to_string(),
                        type_hint: "object".to_string(),
                        description: "Request message as JSON object".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "metadata".to_string(),
                        type_hint: "object".to_string(),
                        description: "Optional gRPC metadata (headers)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "call_grpc_method",
                    "service": "calculator.Calculator",
                    "method": "Add",
                    "request": {"a": 5, "b": 3},
                    "metadata": {"auth-token": "secret"}
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the gRPC server".to_string(),
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
                name: "call_grpc_method".to_string(),
                description: "Call another gRPC method in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "service".to_string(),
                        type_hint: "string".to_string(),
                        description: "Fully qualified service name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "Method name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "request".to_string(),
                        type_hint: "object".to_string(),
                        description: "Request message as JSON object".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "metadata".to_string(),
                        type_hint: "object".to_string(),
                        description: "Optional gRPC metadata (headers)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "call_grpc_method",
                    "service": "calculator.Calculator",
                    "method": "Multiply",
                    "request": {"a": 2, "b": 3}
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait without making another call".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "gRPC"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "grpc_connected".to_string(),
                description: "Triggered when gRPC client connects to server".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "grpc_response_received".to_string(),
                description: "Triggered when gRPC client receives a response".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "grpc_error".to_string(),
                description: "Triggered when gRPC client receives an error".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP/2>gRPC"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["grpc", "grpc client", "connect to grpc", "rpc"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("tonic gRPC client with dynamic protobuf schema support")
            .llm_control("Full control over RPC calls (service, method, request data)")
            .e2e_testing("Local gRPC server or public gRPC APIs")
            .build()
    }
    fn description(&self) -> &'static str {
        "gRPC client for calling RPC services"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to gRPC server at localhost:50051 and call Calculator.Add with a=5, b=3"
    }
    fn group_name(&self) -> &'static str {
        "RPC & API"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for GrpcClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::grpc::GrpcClient;
            GrpcClient::connect_with_llm_actions(
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
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "call_grpc_method" => {
                let service = action
                    .get("service")
                    .and_then(|v| v.as_str())
                    .context("Missing 'service' field")?
                    .to_string();

                let method = action
                    .get("method")
                    .and_then(|v| v.as_str())
                    .context("Missing 'method' field")?
                    .to_string();

                let request = action
                    .get("request")
                    .context("Missing 'request' field")?
                    .clone();

                let metadata = action.get("metadata").and_then(|v| v.as_object()).cloned();

                // Return custom result with RPC call data
                Ok(ClientActionResult::Custom {
                    name: "grpc_call".to_string(),
                    data: json!({
                        "service": service,
                        "method": method,
                        "request": request,
                        "metadata": metadata,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown gRPC client action: {}",
                action_type
            )),
        }
    }
}
