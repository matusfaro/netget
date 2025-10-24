//! PostgreSQL protocol actions implementation

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

/// PostgreSQL protocol action handler
pub struct PostgresqlProtocol {
    #[allow(dead_code)]
    connection_id: ConnectionId,
    #[allow(dead_code)]
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
}

impl PostgresqlProtocol {
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

impl ProtocolActions for PostgresqlProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_postgresql_connections_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            postgresql_query_response_action(),
            postgresql_error_response_action(),
            postgresql_ok_response_action(),
            close_this_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "postgresql_query_response" => self.execute_postgresql_query_response(action),
            "postgresql_error_response" => self.execute_postgresql_error_response(action),
            "postgresql_ok_response" => self.execute_postgresql_ok_response(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "list_postgresql_connections" => self.execute_list_postgresql_connections(action),
            _ => Err(anyhow::anyhow!("Unknown PostgreSQL action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "PostgreSQL"
    }
}

impl PostgresqlProtocol {
    fn execute_postgresql_query_response(&self, action: serde_json::Value) -> Result<ActionResult> {
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
            "PostgreSQL query response: {} columns, {} rows",
            columns.len(),
            rows.len()
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] PostgreSQL → Result set: {} columns, {} rows",
            columns.len(),
            rows.len()
        ));

        Ok(ActionResult::PostgresqlQueryResponse {
            columns: columns.clone(),
            rows: rows.clone(),
        })
    }

    fn execute_postgresql_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let severity = action
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("ERROR");

        let code = action
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("XX000");

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        debug!("PostgreSQL error response: {} {} - {}", severity, code, message);

        let _ = self.status_tx.send(format!(
            "[DEBUG] PostgreSQL ✗ {} {}: {}",
            severity, code, message
        ));

        Ok(ActionResult::PostgresqlError {
            severity: severity.to_string(),
            code: code.to_string(),
            message: message.to_string(),
        })
    }

    fn execute_postgresql_ok_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let tag = action
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("OK");

        debug!("PostgreSQL OK response: {}", tag);

        let _ = self.status_tx.send(format!(
            "[DEBUG] PostgreSQL → OK: {}",
            tag
        ));

        Ok(ActionResult::PostgresqlOk {
            tag: tag.to_string(),
        })
    }

    fn execute_list_postgresql_connections(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("PostgreSQL list connections");

        // Placeholder - in a real implementation, we'd track connections
        Ok(ActionResult::Custom {
            name: "list_postgresql_connections".to_string(),
            data: json!({"connections": []}),
        })
    }
}

/// Action definition: Send PostgreSQL query response
pub fn postgresql_query_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "postgresql_query_response".to_string(),
        description: "Send a result set in response to a SELECT query".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Array of column definitions. Each column should have 'name' and 'type' (e.g. 'text', 'int4', 'int8', 'float8', 'bool')".to_string(),
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
            "type": "postgresql_query_response",
            "columns": [{"name": "id", "type": "int4"}, {"name": "name", "type": "text"}],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
}

/// Action definition: Send PostgreSQL error response
pub fn postgresql_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "postgresql_error_response".to_string(),
        description: "Send an error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "severity".to_string(),
                type_hint: "string".to_string(),
                description: "Error severity (ERROR, FATAL, PANIC, WARNING, NOTICE, DEBUG, INFO, LOG)".to_string(),
                required: false,
            },
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description: "PostgreSQL error code (e.g. '42P01' for undefined_table, '42601' for syntax_error)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message to display to the client".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "postgresql_error_response",
            "severity": "ERROR",
            "code": "42P01",
            "message": "relation \"table_name\" does not exist"
        }),
    }
}

/// Action definition: Send PostgreSQL command complete response
pub fn postgresql_ok_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "postgresql_ok_response".to_string(),
        description: "Send a command complete response for INSERT, UPDATE, DELETE, or other non-SELECT queries".to_string(),
        parameters: vec![
            Parameter {
                name: "tag".to_string(),
                type_hint: "string".to_string(),
                description: "Command tag (e.g. 'INSERT 0 1', 'UPDATE 3', 'DELETE 2', 'CREATE TABLE')".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "postgresql_ok_response",
            "tag": "INSERT 0 1"
        }),
    }
}

/// Action definition: Close current PostgreSQL connection
pub fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current PostgreSQL connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
}

/// Action definition: List PostgreSQL connections
pub fn list_postgresql_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_postgresql_connections".to_string(),
        description: "List all active PostgreSQL connections".to_string(),
        parameters: vec![],
        example: json!({"type": "list_postgresql_connections"}),
    }
}
