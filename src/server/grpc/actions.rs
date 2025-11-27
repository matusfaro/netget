//! gRPC protocol actions implementation

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

/// gRPC protocol action handler
pub struct GrpcProtocol;

impl GrpcProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_grpc_unary_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .context("Missing 'message' parameter in grpc_unary_response")?;

        debug!("gRPC unary response: {}", serde_json::to_string(message)?);

        // Return as Custom action result so server can encode to protobuf
        Ok(ActionResult::Custom {
            name: "grpc_unary_response".to_string(),
            data: json!({ "message": message }),
        })
    }

    fn execute_grpc_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("INTERNAL");

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter in grpc_error")?;

        debug!("gRPC error response: {} - {}", code, message);

        // Return as Custom action result so server can construct proper gRPC error
        Ok(ActionResult::Custom {
            name: "grpc_error".to_string(),
            data: json!({
                "code": code,
                "message": message
            }),
        })
    }

    fn execute_reload_schema(&self, action: serde_json::Value) -> Result<ActionResult> {
        let proto_schema = action
            .get("proto_schema")
            .and_then(|v| v.as_str())
            .context("Missing 'proto_schema' parameter")?;

        debug!(
            "gRPC reload schema request (length: {} bytes)",
            proto_schema.len()
        );

        // Return as Custom action result so server can reload schema
        Ok(ActionResult::Custom {
            name: "reload_schema".to_string(),
            data: json!({ "proto_schema": proto_schema }),
        })
    }

    fn execute_list_services(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("gRPC list services request");

        // Return as Custom action result so server can list services from descriptor pool
        Ok(ActionResult::Custom {
            name: "list_services".to_string(),
            data: json!({}),
        })
    }

    fn execute_describe_method(&self, action: serde_json::Value) -> Result<ActionResult> {
        let service = action
            .get("service")
            .and_then(|v| v.as_str())
            .context("Missing 'service' parameter")?;

        let method = action
            .get("method")
            .and_then(|v| v.as_str())
            .context("Missing 'method' parameter")?;

        debug!("gRPC describe method: {}/{}", service, method);

        // Return as Custom action result so server can describe method from descriptor pool
        Ok(ActionResult::Custom {
            name: "describe_method".to_string(),
            data: json!({
                "service": service,
                "method": method
            }),
        })
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for GrpcProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        use crate::llm::actions::ParameterDefinition;
        vec![
                ParameterDefinition {
                    name: "proto_schema".to_string(),
                    type_hint: "string".to_string(),
                    description: "Protobuf schema definition. IMPORTANT: For LLM responses, use inline .proto text (proto3 syntax). LLMs should NOT use base64-encoded FileDescriptorSet (truncation issues). Alternatively, provide path to .proto file on disk.".to_string(),
                    required: true,
                    example: json!("syntax = \"proto3\"; package test; service UserService { rpc GetUser(UserId) returns (User); } message UserId { int32 id = 1; } message User { int32 id = 1; string name = 2; string email = 3; }"),
                },
                ParameterDefinition {
                    name: "enable_reflection".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Enable gRPC server reflection (allows clients to discover schema dynamically)".to_string(),
                    required: false,
                    example: json!(true),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            reload_schema_action(),
            list_services_action(),
            describe_method_action(),
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![grpc_unary_response_action(), grpc_error_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "gRPC"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_grpc_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP2>GRPC"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["grpc", "grpcserver", "protobuf"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("prost-reflect dynamic schema, tonic, hyper HTTP/2")
            .llm_control("All RPC request/response handling, dynamic schema loading")
            .e2e_testing("grpcurl / gRPC clients")
            .notes("Unary RPCs only, no streaming, dynamic protobuf via prost-reflect")
            .build()
    }
    fn description(&self) -> &'static str {
        "gRPC server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a gRPC server on port 50051 with this schema: service UserService { rpc GetUser(UserId) returns (User); }"
    }
    fn group_name(&self) -> &'static str {
        "AI & API"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for GrpcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::grpc::GrpcServer;
            GrpcServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
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
            "grpc_unary_response" => self.execute_grpc_unary_response(action),
            "grpc_error" => self.execute_grpc_error(action),
            "reload_schema" => self.execute_reload_schema(action),
            "list_services" => self.execute_list_services(action),
            "describe_method" => self.execute_describe_method(action),
            _ => Err(anyhow::anyhow!("Unknown gRPC action: {}", action_type)),
        }
    }
}

// ============================================================================
// Action Definitions
// ============================================================================

fn grpc_unary_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "grpc_unary_response".to_string(),
        description: "Send gRPC unary response with JSON message".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "object".to_string(),
            description: "Response message as JSON object matching protobuf schema".to_string(),
            required: true,
        }],
        example: json!({
            "type": "grpc_unary_response",
            "message": {
                "id": 123,
                "name": "Alice",
                "email": "alice@example.com"
            }
        }),
    }
}

fn grpc_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "grpc_error".to_string(),
        description: "Return gRPC error with status code and message".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description:
                    "gRPC status code (OK, CANCELLED, INVALID_ARGUMENT, NOT_FOUND, INTERNAL, etc.)"
                        .to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "grpc_error",
            "code": "NOT_FOUND",
            "message": "User not found"
        }),
    }
}

fn reload_schema_action() -> ActionDefinition {
    ActionDefinition {
        name: "reload_schema".to_string(),
        description: "Reload protobuf schema with new definition".to_string(),
        parameters: vec![Parameter {
            name: "proto_schema".to_string(),
            type_hint: "string".to_string(),
            description: "New protobuf schema definition".to_string(),
            required: true,
        }],
        example: json!({
            "type": "reload_schema",
            "proto_schema": "service UserService { rpc GetUser(UserId) returns (User); }"
        }),
    }
}

fn list_services_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_services".to_string(),
        description: "List all available gRPC services and methods".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_services"
        }),
    }
}

fn describe_method_action() -> ActionDefinition {
    ActionDefinition {
        name: "describe_method".to_string(),
        description: "Describe a specific gRPC method's request/response schema".to_string(),
        parameters: vec![
            Parameter {
                name: "service".to_string(),
                type_hint: "string".to_string(),
                description: "Service name (e.g., 'UserService')".to_string(),
                required: true,
            },
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "Method name (e.g., 'GetUser')".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "describe_method",
            "service": "UserService",
            "method": "GetUser"
        }),
    }
}

// ============================================================================
// gRPC Action Constants
// ============================================================================

pub static GRPC_UNARY_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| grpc_unary_response_action());
pub static GRPC_ERROR_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| grpc_error_action());
pub static RELOAD_SCHEMA_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| reload_schema_action());
pub static LIST_SERVICES_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| list_services_action());
pub static DESCRIBE_METHOD_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| describe_method_action());

// ============================================================================
// gRPC Event Type Constants
// ============================================================================

/// gRPC unary request event - triggered when client makes a unary RPC call
pub static GRPC_UNARY_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "grpc_unary_request",
        "gRPC unary RPC request received from client",
        json!({
            "type": "grpc_unary_response",
            "message": {
                "id": 123,
                "name": "Alice",
                "email": "alice@example.com"
            }
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "service".to_string(),
            type_hint: "string".to_string(),
            description: "Service name (e.g., 'UserService')".to_string(),
            required: true,
        },
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "Method name (e.g., 'GetUser')".to_string(),
            required: true,
        },
        Parameter {
            name: "request".to_string(),
            type_hint: "object".to_string(),
            description: "Request message as JSON object".to_string(),
            required: true,
        },
        Parameter {
            name: "expected_response_schema".to_string(),
            type_hint: "object".to_string(),
            description: "Expected response schema as JSON Schema".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        GRPC_UNARY_RESPONSE_ACTION.clone(),
        GRPC_ERROR_ACTION.clone(),
    ])
});

/// Get gRPC event types
pub fn get_grpc_event_types() -> Vec<EventType> {
    vec![GRPC_UNARY_REQUEST_EVENT.clone()]
}
