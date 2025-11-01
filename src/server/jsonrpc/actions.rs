//! JSON-RPC protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use tracing::debug;

/// JSON-RPC protocol action handler
pub struct JsonRpcProtocol {}

impl JsonRpcProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

impl Server for JsonRpcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::jsonrpc::JsonRpcServer;
            let send_first = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get("send_first"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            JsonRpcServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_rpc_methods_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            jsonrpc_success_action(),
            jsonrpc_error_action(),
        ]
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

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Alpha
        )
    }
}

impl JsonRpcProtocol {
    fn execute_jsonrpc_success(&self, action: serde_json::Value) -> Result<ActionResult> {
        let result = action
            .get("result")
            .context("Missing 'result' field")?
            .clone();

        let id = action
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        debug!("JSON-RPC success response: result={:?}, id={:?}", result, id);

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

        let id = action
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        debug!("JSON-RPC error response: code={}, message={}, id={:?}", code, message, id);

        // Build JSON-RPC 2.0 error response
        let mut error = json!({
            "code": code,
            "message": message
        });

        if let Some(data_val) = data {
            error.as_object_mut().unwrap().insert("data".to_string(), data_val);
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
    }
}

/// Action definition: List available RPC methods
pub fn list_rpc_methods_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_rpc_methods".to_string(),
        description: "List all available RPC methods (async action, no network context needed)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_rpc_methods"
        }),
    }
}

/// Get JSON-RPC event types
pub fn get_jsonrpc_event_types() -> Vec<EventType> {
    vec![
        EventType::new(
            "jsonrpc_method_call",
            "JSON-RPC 2.0 method call received"
        )
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
        .with_actions(vec![
            jsonrpc_success_action(),
            jsonrpc_error_action(),
        ]),
    ]
}
