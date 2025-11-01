//! OpenAPI protocol actions for LLM integration

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::metadata::{ProtocolMetadata, DevelopmentState};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::Value as JsonValue;
use std::sync::LazyLock;
use tracing::{debug, error, warn};

/// OpenAPI request event - triggered when client sends an HTTP request to OpenAPI server
pub static OPENAPI_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openapi_request",
        "HTTP request received by OpenAPI server"
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (GET, POST, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path".to_string(),
            required: true,
        },
        Parameter {
            name: "uri".to_string(),
            type_hint: "string".to_string(),
            description: "Full request URI".to_string(),
            required: true,
        },
        Parameter {
            name: "headers".to_string(),
            type_hint: "object".to_string(),
            description: "Request headers as key-value pairs".to_string(),
            required: false,
        },
        Parameter {
            name: "body".to_string(),
            type_hint: "string".to_string(),
            description: "Request body".to_string(),
            required: false,
        },
        Parameter {
            name: "spec_info".to_string(),
            type_hint: "object".to_string(),
            description: "Information about loaded OpenAPI spec".to_string(),
            required: false,
        },
        Parameter {
            name: "matched_route".to_string(),
            type_hint: "object".to_string(),
            description: "Matched route information (only present if route matched). Contains: operation_id, path_template, path_params (extracted parameters like {id}), and operation (full OpenAPI operation spec)".to_string(),
            required: false,
        },
    ])
});

/// OpenAPI protocol implementation
#[derive(Clone)]
pub struct OpenApiProtocol;

impl OpenApiProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenApiProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Server for OpenApiProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::openapi::OpenApiServer;
            OpenApiServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            ).await
        })
    }

    fn protocol_name(&self) -> &'static str {
        "OpenAPI"
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OPENAPI"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["openapi", "rest", "rest api", "api", "swagger"]
    }

    fn metadata(&self) -> ProtocolMetadata {
        ProtocolMetadata::with_notes(
            DevelopmentState::Alpha,
            "OpenAPI 3.1 spec-driven HTTP server with runtime request validation"
        )
    }

    fn description(&self) -> &'static str {
        "OpenAPI specification server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an OpenAPI server for a TODO API on port 8080"
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "reload_spec".to_string(),
                description: "Reload or update the OpenAPI specification at runtime".to_string(),
                parameters: vec![
                    Parameter {
                        name: "spec".to_string(),
                        type_hint: "string".to_string(),
                        description: "OpenAPI 3.1 specification in YAML or JSON format".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "reload_spec", "spec": "openapi: 3.1.0\ninfo:\n  title: My API\n  version: 1.0.0\npaths:\n  /users:\n    get:\n      responses:\n        '200':\n          description: List users"}),
            },
            ActionDefinition {
                name: "get_spec_info".to_string(),
                description: "Get summary information about the loaded OpenAPI specification".to_string(),
                parameters: vec![],
                example: serde_json::json!({"type": "get_spec_info"}),
            },
            ActionDefinition {
                name: "configure_error_handling".to_string(),
                description: "Configure whether to ask LLM for invalid requests (404/405/400). By default, these errors are handled immediately without LLM. Enable this to let LLM customize error responses for honeypot purposes.".to_string(),
                parameters: vec![
                    Parameter {
                        name: "llm_on_invalid".to_string(),
                        type_hint: "boolean".to_string(),
                        description: "If true, ask LLM for 404/405/400 errors. If false (default), return immediate RFC-compliant error responses.".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "configure_error_handling", "llm_on_invalid": false}),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "provide_openapi_spec".to_string(),
                description: "Provide the OpenAPI specification (used during server startup)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "spec".to_string(),
                        type_hint: "string".to_string(),
                        description: "OpenAPI 3.1 specification in YAML or JSON format".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "provide_openapi_spec", "spec": "openapi: 3.1.0\ninfo:\n  title: TODO API\n  version: 1.0.0\npaths:\n  /todos:\n    get:\n      operationId: listTodos\n      responses:\n        '200':\n          description: List of todos\n          content:\n            application/json:\n              schema:\n                type: array\n                items:\n                  type: object\n                  properties:\n                    id:\n                      type: integer\n                    title:\n                      type: string\n                    done:\n                      type: boolean"}),
            },
            ActionDefinition {
                name: "send_openapi_response".to_string(),
                description: "Send an HTTP response for an OpenAPI request".to_string(),
                parameters: vec![
                    Parameter {
                        name: "status_code".to_string(),
                        type_hint: "number".to_string(),
                        description: "HTTP status code (e.g., 200, 404, 500)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "headers".to_string(),
                        type_hint: "object".to_string(),
                        description: "HTTP response headers as key-value pairs".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Response body (JSON, XML, plain text, etc.)".to_string(),
                        required: false,
                    },
                ],
                example: serde_json::json!({"type": "send_openapi_response", "status_code": 200, "headers": {"content-type": "application/json"}, "body": "[{\"id\": 1, \"title\": \"Buy milk\", \"done\": false}]"}),
            },
            ActionDefinition {
                name: "send_validation_error".to_string(),
                description: "Send an HTTP error response for invalid requests (400, 405, 415, etc.)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "status_code".to_string(),
                        type_hint: "number".to_string(),
                        description: "HTTP error status code (400=Bad Request, 405=Method Not Allowed, 415=Unsupported Media Type, etc.)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "message".to_string(),
                        type_hint: "string".to_string(),
                        description: "Error message explaining the validation failure".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "send_validation_error", "status_code": 405, "message": "Method GET not allowed for path /users, expected POST"}),
            },
        ]
    }

    fn execute_action(&self, action: JsonValue) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "provide_openapi_spec" => {
                let spec = action["spec"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing spec parameter"))?;

                debug!("OpenAPI spec provided: {} bytes", spec.len());

                Ok(ActionResult::Custom {
                    name: "load_openapi_spec".to_string(),
                    data: serde_json::json!({
                        "spec": spec
                    }),
                })
            }
            "send_openapi_response" => {
                let status_code = action["status_code"]
                    .as_i64()
                    .ok_or_else(|| anyhow!("Missing status_code parameter"))? as u16;

                let headers = action["headers"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();

                let body = action["body"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let spec_compliant = action["spec_compliant"]
                    .as_bool()
                    .unwrap_or(true);

                let compliance_status = if spec_compliant { "compliant" } else { "non-compliant (intentional)" };
                debug!(
                    "OpenAPI response: {} {} bytes, spec: {}",
                    status_code,
                    body.len(),
                    compliance_status
                );

                Ok(ActionResult::Custom {
                    name: "send_openapi_response".to_string(),
                    data: serde_json::json!({
                        "status_code": status_code,
                        "headers": headers,
                        "body": body,
                        "spec_compliant": spec_compliant
                    }),
                })
            }
            "send_validation_error" => {
                let status_code = action["status_code"]
                    .as_i64()
                    .ok_or_else(|| anyhow!("Missing status_code parameter"))? as u16;

                let message = action["message"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing message parameter"))?;

                warn!("OpenAPI validation error: {} - {}", status_code, message);

                Ok(ActionResult::Custom {
                    name: "send_validation_error".to_string(),
                    data: serde_json::json!({
                        "status_code": status_code,
                        "message": message
                    }),
                })
            }
            "reload_spec" => {
                let spec = action["spec"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing spec parameter"))?;

                debug!("OpenAPI spec reload requested: {} bytes", spec.len());

                Ok(ActionResult::Custom {
                    name: "reload_spec".to_string(),
                    data: serde_json::json!({
                        "spec": spec,
                        "reload": true
                    }),
                })
            }
            "get_spec_info" => {
                debug!("OpenAPI spec info requested");
                Ok(ActionResult::NoAction)
            }
            "configure_error_handling" => {
                let llm_on_invalid = action["llm_on_invalid"]
                    .as_bool()
                    .ok_or_else(|| anyhow!("Missing llm_on_invalid parameter"))?;

                debug!("OpenAPI error handling configured: llm_on_invalid={}", llm_on_invalid);

                Ok(ActionResult::Custom {
                    name: "configure_error_handling".to_string(),
                    data: serde_json::json!({
                        "llm_on_invalid": llm_on_invalid
                    }),
                })
            }
            _ => {
                error!("Unknown OpenAPI action: {}", action_type);
                Err(anyhow!("Unknown action type: {}", action_type))
            }
        }
    }

    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        use crate::llm::actions::ParameterDefinition;
        vec![
            ParameterDefinition {
                name: "spec".to_string(),
                type_hint: "string".to_string(),
                description: "OpenAPI 3.x specification in YAML or JSON format (inline)".to_string(),
                required: false,
                example: serde_json::json!("openapi: 3.1.0\ninfo:\n  title: My API\n  version: 1.0.0"),
            },
            ParameterDefinition {
                name: "spec_file".to_string(),
                type_hint: "string".to_string(),
                description: "Path to OpenAPI specification file (YAML or JSON)".to_string(),
                required: false,
                example: serde_json::json!("/path/to/openapi.yaml"),
            },
        ]
    }
}
