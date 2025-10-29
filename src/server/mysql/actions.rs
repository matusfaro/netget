//! MySQL protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::server::connection::ConnectionId;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc;
use tracing::debug;

/// Connection data for MySQL protocol
pub struct MysqlConnectionData {
    pub database: Option<String>,
}

/// MySQL protocol action handler
pub struct MysqlProtocol {
    _connection_id: ConnectionId,
    _app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
}

impl MysqlProtocol {
    pub fn new(
        connection_id: ConnectionId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        Self {
            _connection_id: connection_id,
            _app_state: app_state,
            status_tx,
        }
    }
}

impl ProtocolActions for MysqlProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_mysql_connections_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            mysql_query_response_action(),
            mysql_error_response_action(),
            mysql_ok_response_action(),
            close_this_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "mysql_query_response" => self.execute_mysql_query_response(action),
            "mysql_error_response" => self.execute_mysql_error_response(action),
            "mysql_ok_response" => self.execute_mysql_ok_response(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "list_mysql_connections" => self.execute_list_mysql_connections(action),
            _ => Err(anyhow::anyhow!("Unknown MySQL action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "MySQL"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_mysql_event_types()
    }
}

impl MysqlProtocol {
    fn execute_mysql_query_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract columns and rows from the action
        let columns = action
            .get("columns")
            .and_then(|v| v.as_array())
            .context("Missing 'columns' array")?;

        let rows = action
            .get("rows")
            .and_then(|v| v.as_array())
            .context("Missing 'rows' array")?;

        debug!(
            "MySQL query response: {} columns, {} rows",
            columns.len(),
            rows.len()
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] MySQL → Result set: {} columns, {} rows",
            columns.len(),
            rows.len()
        ));

        // Return a custom action result with the query response data
        Ok(ActionResult::MysqlQueryResponse {
            columns: columns.clone(),
            rows: rows.clone(),
        })
    }

    fn execute_mysql_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(1064) as u16;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        debug!("MySQL error response: {} - {}", error_code, message);

        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL ✗ Error {}: {}", error_code, message));

        Ok(ActionResult::MysqlError {
            error_code,
            message: message.to_string(),
        })
    }

    fn execute_mysql_ok_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let affected_rows = action
            .get("affected_rows")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let last_insert_id = action
            .get("last_insert_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        debug!(
            "MySQL OK response: affected_rows={}, last_insert_id={}",
            affected_rows, last_insert_id
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] MySQL → OK: {} rows affected",
            affected_rows
        ));

        Ok(ActionResult::MysqlOk {
            affected_rows,
            last_insert_id,
        })
    }

    fn execute_list_mysql_connections(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("MySQL list connections");

        // This is a placeholder - in a real implementation, we'd track connections
        Ok(ActionResult::Custom {
            name: "list_mysql_connections".to_string(),
            data: json!({"connections": []}),
        })
    }
}

/// Action definition: Send MySQL query response
pub fn mysql_query_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mysql_query_response".to_string(),
        description: "Send a result set in response to a SELECT query".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Array of column definitions. Each column should have 'name' and 'type' (e.g. 'VARCHAR', 'INT', 'BIGINT')".to_string(),
                required: true,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of rows. Each row is an array of values matching the column order".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mysql_query_response",
            "columns": [{"name": "id", "type": "INT"}, {"name": "name", "type": "VARCHAR"}],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
}

/// Action definition: Send MySQL error response
pub fn mysql_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mysql_error_response".to_string(),
        description: "Send an error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description:
                    "MySQL error code (e.g. 1064 for syntax error, 1146 for table not found)"
                        .to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message to display to the client".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mysql_error_response",
            "error_code": 1146,
            "message": "Table 'database.table_name' doesn't exist"
        }),
    }
}

/// Action definition: Send MySQL OK response
pub fn mysql_ok_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mysql_ok_response".to_string(),
        description: "Send an OK response for INSERT, UPDATE, DELETE, or other non-SELECT queries"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "affected_rows".to_string(),
                type_hint: "number".to_string(),
                description: "Number of rows affected by the query".to_string(),
                required: false,
            },
            Parameter {
                name: "last_insert_id".to_string(),
                type_hint: "number".to_string(),
                description: "Last insert ID for INSERT queries with auto_increment".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "mysql_ok_response",
            "affected_rows": 1,
            "last_insert_id": 42
        }),
    }
}

/// Action definition: Close current MySQL connection
pub fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current MySQL connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
}

/// Action definition: List MySQL connections
pub fn list_mysql_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_mysql_connections".to_string(),
        description: "List all active MySQL connections".to_string(),
        parameters: vec![],
        example: json!({"type": "list_mysql_connections"}),
    }
}

/// Action definition: Close specific MySQL connection
pub fn mysql_close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "mysql_close_connection".to_string(),
        description: "Close a specific MySQL connection by ID".to_string(),
        parameters: vec![Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "The connection ID to close".to_string(),
            required: true,
        }],
        example: json!({
            "type": "mysql_close_connection",
            "connection_id": "conn-123"
        }),
    }
}

// ============================================================================
// MySQL Action Constants
// ============================================================================

/// MySQL query response action constant
pub static MYSQL_QUERY_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mysql_query_response".to_string(),
        description: "Send a result set in response to a SELECT query".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Array of column definitions. Each column should have 'name' and 'type' (e.g. 'VARCHAR', 'INT', 'BIGINT')".to_string(),
                required: true,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of rows. Each row is an array of values matching the column order".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mysql_query_response",
            "columns": [{"name": "id", "type": "INT"}, {"name": "name", "type": "VARCHAR"}],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
});

/// MySQL error response action constant
pub static MYSQL_ERROR_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mysql_error_response".to_string(),
        description: "Send an error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "MySQL error code (e.g. 1064 for syntax error, 1146 for table not found)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message to display to the client".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mysql_error_response",
            "error_code": 1146,
            "message": "Table 'database.table_name' doesn't exist"
        }),
    }
});

/// MySQL OK response action constant
pub static MYSQL_OK_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mysql_ok_response".to_string(),
        description: "Send an OK response for INSERT, UPDATE, DELETE, or other non-SELECT queries".to_string(),
        parameters: vec![
            Parameter {
                name: "affected_rows".to_string(),
                type_hint: "number".to_string(),
                description: "Number of rows affected by the query".to_string(),
                required: false,
            },
            Parameter {
                name: "last_insert_id".to_string(),
                type_hint: "number".to_string(),
                description: "Last insert ID for INSERT queries with auto_increment".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "mysql_ok_response",
            "affected_rows": 1,
            "last_insert_id": 42
        }),
    }
});

/// MySQL close connection action constant
pub static MYSQL_CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current MySQL connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
});

// ============================================================================
// MySQL Event Type Constants
// ============================================================================

/// MySQL query event - triggered when client sends a query
pub static MYSQL_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "mysql_query",
        "MySQL query received from client"
    )
    .with_parameters(vec![
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The SQL query string sent by the client".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        MYSQL_QUERY_RESPONSE_ACTION.clone(),
        MYSQL_ERROR_RESPONSE_ACTION.clone(),
        MYSQL_OK_RESPONSE_ACTION.clone(),
        MYSQL_CLOSE_CONNECTION_ACTION.clone(),
    ])
});

/// Get MySQL event types
pub fn get_mysql_event_types() -> Vec<EventType> {
    vec![
        MYSQL_QUERY_EVENT.clone(),
    ]
}
