//! Redis protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::debug;

/// Redis protocol action handler
pub struct RedisProtocol {
    #[allow(dead_code)]
    connection_id: ConnectionId,
    #[allow(dead_code)]
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
}

impl RedisProtocol {
    pub fn new(
        connection_id: ConnectionId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        Self {
            connection_id,
            app_state,
            status_tx,
        }
    }
}

impl ProtocolActions for RedisProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_redis_connections_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            redis_simple_string_action(),
            redis_bulk_string_action(),
            redis_array_action(),
            redis_integer_action(),
            redis_error_action(),
            redis_null_action(),
            close_this_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "redis_simple_string" => self.execute_redis_simple_string(action),
            "redis_bulk_string" => self.execute_redis_bulk_string(action),
            "redis_array" => self.execute_redis_array(action),
            "redis_integer" => self.execute_redis_integer(action),
            "redis_error" => self.execute_redis_error(action),
            "redis_null" => self.execute_redis_null(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "list_redis_connections" => self.execute_list_redis_connections(action),
            _ => Err(anyhow::anyhow!("Unknown Redis action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Redis"
    }
}

impl RedisProtocol {
    fn execute_redis_simple_string(&self, action: serde_json::Value) -> Result<ActionResult> {
        let value = action
            .get("value")
            .and_then(|v| v.as_str())
            .context("Missing 'value' field")?;

        debug!("Redis simple string response: {}", value);
        let _ = self.status_tx.send(format!(
            "[DEBUG] Redis → Simple string: {}",
            value
        ));

        Ok(ActionResult::RedisSimpleString {
            value: value.to_string(),
        })
    }

    fn execute_redis_bulk_string(&self, action: serde_json::Value) -> Result<ActionResult> {
        let value = action.get("value");

        let result = if let Some(v) = value {
            if v.is_null() {
                None
            } else if let Some(s) = v.as_str() {
                Some(s.as_bytes().to_vec())
            } else {
                Some(v.to_string().as_bytes().to_vec())
            }
        } else {
            None
        };

        debug!("Redis bulk string response: {:?}", result);
        let _ = self.status_tx.send(format!(
            "[DEBUG] Redis → Bulk string: {} bytes",
            result.as_ref().map(|v| v.len()).unwrap_or(0)
        ));

        Ok(ActionResult::RedisBulkString { value: result })
    }

    fn execute_redis_array(&self, action: serde_json::Value) -> Result<ActionResult> {
        let values = action
            .get("values")
            .and_then(|v| v.as_array())
            .context("Missing 'values' array")?;

        debug!("Redis array response: {} elements", values.len());
        let _ = self.status_tx.send(format!(
            "[DEBUG] Redis → Array: {} elements",
            values.len()
        ));

        Ok(ActionResult::RedisArray {
            values: values.clone(),
        })
    }

    fn execute_redis_integer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let value = action
            .get("value")
            .and_then(|v| v.as_i64())
            .context("Missing 'value' field")?;

        debug!("Redis integer response: {}", value);
        let _ = self.status_tx.send(format!(
            "[DEBUG] Redis → Integer: {}",
            value
        ));

        Ok(ActionResult::RedisInteger { value })
    }

    fn execute_redis_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' field")?;

        debug!("Redis error response: {}", message);
        let _ = self.status_tx.send(format!(
            "[DEBUG] Redis ✗ Error: {}",
            message
        ));

        Ok(ActionResult::RedisError {
            message: message.to_string(),
        })
    }

    fn execute_redis_null(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Redis null response");
        let _ = self.status_tx.send("[DEBUG] Redis → Null".to_string());

        Ok(ActionResult::RedisNull)
    }

    fn execute_list_redis_connections(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Redis list connections");

        Ok(ActionResult::Custom {
            name: "list_redis_connections".to_string(),
            data: json!({"connections": []}),
        })
    }
}

/// Action definition: Send Redis simple string response
pub fn redis_simple_string_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_simple_string".to_string(),
        description: "Send a simple string response (e.g., '+OK\\r\\n')".to_string(),
        parameters: vec![Parameter {
            name: "value".to_string(),
            type_hint: "string".to_string(),
            description: "The string value to send (without RESP encoding)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "redis_simple_string",
            "value": "OK"
        }),
    }
}

/// Action definition: Send Redis bulk string response
pub fn redis_bulk_string_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_bulk_string".to_string(),
        description: "Send a bulk string response (e.g., '$5\\r\\nhello\\r\\n'). Use null for nil bulk string".to_string(),
        parameters: vec![Parameter {
            name: "value".to_string(),
            type_hint: "string".to_string(),
            description: "The string value to send, or null for nil bulk string".to_string(),
            required: false,
        }],
        example: json!({
            "type": "redis_bulk_string",
            "value": "hello world"
        }),
    }
}

/// Action definition: Send Redis array response
pub fn redis_array_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_array".to_string(),
        description: "Send an array response. Each element will be encoded as bulk string".to_string(),
        parameters: vec![Parameter {
            name: "values".to_string(),
            type_hint: "array".to_string(),
            description: "Array of values. Each will be encoded as bulk string".to_string(),
            required: true,
        }],
        example: json!({
            "type": "redis_array",
            "values": ["value1", "value2", "value3"]
        }),
    }
}

/// Action definition: Send Redis integer response
pub fn redis_integer_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_integer".to_string(),
        description: "Send an integer response (e.g., ':42\\r\\n')".to_string(),
        parameters: vec![Parameter {
            name: "value".to_string(),
            type_hint: "integer".to_string(),
            description: "The integer value to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "redis_integer",
            "value": 42
        }),
    }
}

/// Action definition: Send Redis error response
pub fn redis_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_error".to_string(),
        description: "Send an error response (e.g., '-ERR message\\r\\n')".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "The error message to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "redis_error",
            "message": "ERR unknown command 'foobar'"
        }),
    }
}

/// Action definition: Send Redis null response
pub fn redis_null_action() -> ActionDefinition {
    ActionDefinition {
        name: "redis_null".to_string(),
        description: "Send a null response ('$-1\\r\\n')".to_string(),
        parameters: vec![],
        example: json!({
            "type": "redis_null"
        }),
    }
}

/// Action definition: Close current Redis connection
pub fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current Redis connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
}

/// Action definition: List Redis connections
pub fn list_redis_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_redis_connections".to_string(),
        description: "List all active Redis connections".to_string(),
        parameters: vec![],
        example: json!({"type": "list_redis_connections"}),
    }
}
