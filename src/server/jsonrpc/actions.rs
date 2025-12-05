//! JSON-RPC protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// JSON-RPC protocol action handler
pub struct JsonRpcProtocol {}

impl JsonRpcProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for JsonRpcProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
                crate::llm::actions::ParameterDefinition {
                    name: "send_first".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Whether the server should send the first message after connection (not typically needed for JSON-RPC over HTTP)".to_string(),
                    required: false,
                    example: json!(false),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_rpc_methods_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![jsonrpc_success_action(), jsonrpc_error_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "JSON-RPC"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_jsonrpc_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>JSONRPC"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["jsonrpc", "json-rpc", "json rpc", "rpc"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual JSON-RPC 2.0")
            .llm_control("Method responses")
            .e2e_testing("JSON-RPC client libs")
            .notes("RPC over JSON")
            .build()
    }
    fn description(&self) -> &'static str {
        "JSON-RPC 2.0 server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a JSON-RPC 2.0 server on port 8000"
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
                "port": 8000,
                "base_stack": "jsonrpc",
                "instruction": "JSON-RPC 2.0 server. Implement add(a,b), subtract(a,b), and echo(message) methods. Return -32601 for unknown methods"
            }),
            // Script mode: event_handlers with script handler
            json!({
                "type": "open_server",
                "port": 8000,
                "base_stack": "jsonrpc",
                "event_handlers": [{
                    "event_pattern": "jsonrpc_method_call",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "method = event.get('method', '')\nparams = event.get('params', [])\nif method == 'add' and len(params) >= 2:\n    action('jsonrpc_success', result=params[0] + params[1])\nelse:\n    action('jsonrpc_error', code=-32601, message='Method not found')"
                    }
                }]
            }),
            // Static mode: event_handlers with static actions
            json!({
                "type": "open_server",
                "port": 8000,
                "base_stack": "jsonrpc",
                "event_handlers": [{
                    "event_pattern": "jsonrpc_method_call",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "jsonrpc_success",
                            "result": {"status": "ok", "version": "1.0.0"}
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for JsonRpcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::jsonrpc::JsonRpcServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            JsonRpcServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
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
            "jsonrpc_success" => self.execute_jsonrpc_success(action),
            "jsonrpc_error" => self.execute_jsonrpc_error(action),
            "list_rpc_methods" => self.execute_list_rpc_methods(action),
            _ => Err(anyhow::anyhow!("Unknown JSON-RPC action: {}", action_type)),
        }
    }
}

impl JsonRpcProtocol {
    fn execute_jsonrpc_success(&self, action: serde_json::Value) -> Result<ActionResult> {
        let result = action
            .get("result")
            .context("Missing 'result' field")?
            .clone();

        let id = action.get("id").cloned().unwrap_or(serde_json::Value::Null);

        debug!(
            "JSON-RPC success response: result={:?}, id={:?}",
            result, id
        );

        // Build JSON-RPC 2.0 success response
        let response = json!({
            "jsonrpc": "2.0",
            "result": result,
            "id": id
        });

        Ok(ActionResult::Custom {
            name: "jsonrpc_response".to_string(),
            data: response,
        })
    }

    fn execute_jsonrpc_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action
            .get("code")
            .and_then(|v| v.as_i64())
            .context("Missing 'code' field")? as i32;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' field")?;

        let data = action.get("data").cloned();

        let id = action.get("id").cloned().unwrap_or(serde_json::Value::Null);

        debug!(
            "JSON-RPC error response: code={}, message={}, id={:?}",
            code, message, id
        );

        // Build JSON-RPC 2.0 error response
        let mut error = json!({
            "code": code,
            "message": message
        });

        if let Some(data_val) = data {
            error
                .as_object_mut()
                .unwrap()
                .insert("data".to_string(), data_val);
        }

        let response = json!({
            "jsonrpc": "2.0",
            "error": error,
            "id": id
        });

        Ok(ActionResult::Custom {
            name: "jsonrpc_response".to_string(),
            data: response,
        })
    }

    fn execute_list_rpc_methods(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("JSON-RPC list methods");

        // This is an async action - LLM can decide what methods to list
        // Return a placeholder that shows no predefined methods
        Ok(ActionResult::Custom {
            name: "list_rpc_methods".to_string(),
            data: json!({
                "methods": [],
                "note": "Methods are dynamically handled by the LLM"
            }),
        })
    }
}

/// Action definition: Send JSON-RPC success response
pub fn jsonrpc_success_action() -> ActionDefinition {
    ActionDefinition {
        name: "jsonrpc_success".to_string(),
        description: "Send a JSON-RPC 2.0 success response".to_string(),
        parameters: vec![
            Parameter {
                name: "result".to_string(),
                type_hint: "any".to_string(),
                description: "The result value (can be any JSON type: object, array, string, number, boolean, null)".to_string(),
                required: true,
            },
            Parameter {
                name: "id".to_string(),
                type_hint: "string|number|null".to_string(),
                description: "Optional: Request ID (will be automatically set from the event context if not provided). Only set this explicitly if you need to override the default behavior.".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "jsonrpc_success",
            "result": {"status": "ok", "data": [1, 2, 3]}
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> JSON-RPC success id={id}")
                .with_debug("JSON-RPC jsonrpc_success: id={id}"),
        ),
    }
}

/// Action definition: Send JSON-RPC error response
pub fn jsonrpc_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "jsonrpc_error".to_string(),
        description: "Send a JSON-RPC 2.0 error response".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "Error code (JSON-RPC standard: -32700 Parse error, -32600 Invalid Request, -32601 Method not found, -32602 Invalid params, -32603 Internal error, -32000 to -32099 Server error)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error message".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "any".to_string(),
                description: "Optional additional error data".to_string(),
                required: false,
            },
            Parameter {
                name: "id".to_string(),
                type_hint: "string|number|null".to_string(),
                description: "Optional: Request ID (will be automatically set from the event context if not provided). Only set this explicitly if you need to override the default behavior.".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "jsonrpc_error",
            "code": -32601,
            "message": "Method not found"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> JSON-RPC error {code}: {message}")
                .with_debug("JSON-RPC jsonrpc_error: code={code} message={message}"),
        ),
    }
}

/// Action definition: List available RPC methods
pub fn list_rpc_methods_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_rpc_methods".to_string(),
        description: "List all available RPC methods (async action, no network context needed)"
            .to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_rpc_methods"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> JSON-RPC list methods")
                .with_debug("JSON-RPC list_rpc_methods"),
        ),
    }
}

/// JSON-RPC method call event - triggered when client sends a JSON-RPC request
pub static JSONRPC_METHOD_CALL_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("jsonrpc_method_call", "JSON-RPC 2.0 method call received", json!({"type": "placeholder", "event_id": "jsonrpc_method_call"}))
        .with_parameters(vec![
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "The RPC method name being called".to_string(),
                required: true,
            },
            Parameter {
                name: "params".to_string(),
                type_hint: "any".to_string(),
                description: "Method parameters (can be array, object, or omitted)".to_string(),
                required: false,
            },
            Parameter {
                name: "id".to_string(),
                type_hint: "string|number|null".to_string(),
                description: "Request ID (null for notifications)".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![jsonrpc_success_action(), jsonrpc_error_action()])
        .with_log_template(
            LogTemplate::new()
                .with_info("JSON-RPC {method}")
                .with_debug("JSON-RPC method={method}, id={id}")
                .with_trace("JSON-RPC: {json_pretty(.)}"),
        )
});

/// Get JSON-RPC event types
pub fn get_jsonrpc_event_types() -> Vec<EventType> {
    vec![JSONRPC_METHOD_CALL_EVENT.clone()]
}
