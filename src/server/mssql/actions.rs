//! MSSQL protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc;
use tracing::debug;

/// MSSQL protocol action handler
pub struct MssqlProtocol {
    _connection_id: ConnectionId,
    _app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
}

impl MssqlProtocol {
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

// Implement Protocol trait (common functionality)
impl Protocol for MssqlProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![crate::llm::actions::ParameterDefinition {
            name: "send_first".to_string(),
            type_hint: "boolean".to_string(),
            description:
                "Whether the server should send the first message after connection (not typically needed for this protocol)"
                    .to_string(),
            required: false,
            example: serde_json::json!(false),
        }]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_mssql_connections_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            mssql_query_response_action(),
            mssql_error_response_action(),
            mssql_ok_response_action(),
            close_this_connection_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "MSSQL"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_mssql_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>TDS>MSSQL"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["mssql", "sql server", "tds"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual TDS protocol implementation (simplified)")
            .llm_control("Query responses (result sets, errors, completion)")
            .e2e_testing("tiberius client crate")
            .notes("No authentication, simplified TDS handshake, basic query support")
            .build()
    }
    fn description(&self) -> &'static str {
        "Microsoft SQL Server (MSSQL) database server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an MSSQL server on port 1433"
    }
    fn group_name(&self) -> &'static str {
        "Database"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for MssqlProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::mssql::MssqlServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            MssqlServer::spawn_with_llm_actions(
                ctx.listen_addr,
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
            "mssql_query_response" => self.execute_mssql_query_response(action),
            "mssql_error_response" => self.execute_mssql_error_response(action),
            "mssql_ok_response" => self.execute_mssql_ok_response(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "list_mssql_connections" => self.execute_list_mssql_connections(action),
            _ => Err(anyhow::anyhow!("Unknown MSSQL action: {}", action_type)),
        }
    }
}

impl MssqlProtocol {
    fn execute_mssql_query_response(&self, action: serde_json::Value) -> Result<ActionResult> {
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
            "MSSQL query response: {} columns, {} rows",
            columns.len(),
            rows.len()
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] MSSQL → Result set: {} columns, {} rows",
            columns.len(),
            rows.len()
        ));

        // Return a custom action result with the query response data
        Ok(ActionResult::Custom {
            name: "mssql_query_response".to_string(),
            data: json!({
                "columns": columns,
                "rows": rows
            }),
        })
    }

    fn execute_mssql_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_number = action
            .get("error_number")
            .and_then(|v| v.as_u64())
            .unwrap_or(50000) as u32;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        let severity = action
            .get("severity")
            .and_then(|v| v.as_u64())
            .unwrap_or(16) as u8;

        debug!("MSSQL error response: {} - {}", error_number, message);

        let _ = self.status_tx.send(format!(
            "[DEBUG] MSSQL ✗ Error {}: {}",
            error_number, message
        ));

        Ok(ActionResult::Custom {
            name: "mssql_error".to_string(),
            data: json!({
                "error_number": error_number,
                "message": message,
                "severity": severity
            }),
        })
    }

    fn execute_mssql_ok_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let rows_affected = action
            .get("rows_affected")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        debug!("MSSQL OK response: rows_affected={}", rows_affected);

        let _ = self
            .status_tx
            .send(format!("[DEBUG] MSSQL → OK: {} rows affected", rows_affected));

        Ok(ActionResult::Custom {
            name: "mssql_ok".to_string(),
            data: json!({
                "rows_affected": rows_affected
            }),
        })
    }

    fn execute_list_mssql_connections(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("MSSQL list connections");

        // This is a placeholder - in a real implementation, we'd track connections
        Ok(ActionResult::Custom {
            name: "list_mssql_connections".to_string(),
            data: json!({"connections": []}),
        })
    }
}

/// Action definition: Send MSSQL query response
pub fn mssql_query_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mssql_query_response".to_string(),
        description: "Send a result set in response to a SELECT query".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Array of column definitions. Each column should have 'name' and 'type' (e.g. 'NVARCHAR', 'INT', 'BIGINT')".to_string(),
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
            "type": "mssql_query_response",
            "columns": [{"name": "id", "type": "INT"}, {"name": "name", "type": "NVARCHAR"}],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
}

/// Action definition: Send MSSQL error response
pub fn mssql_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mssql_error_response".to_string(),
        description: "Send an error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "error_number".to_string(),
                type_hint: "number".to_string(),
                description: "MSSQL error number (e.g. 207 for invalid column, 208 for invalid object)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message to display to the client".to_string(),
                required: true,
            },
            Parameter {
                name: "severity".to_string(),
                type_hint: "number".to_string(),
                description: "Error severity level (1-25, typically 16 for user errors)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "mssql_error_response",
            "error_number": 208,
            "message": "Invalid object name 'table_name'",
            "severity": 16
        }),
    }
}

/// Action definition: Send MSSQL OK response
pub fn mssql_ok_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mssql_ok_response".to_string(),
        description:
            "Send a completion response for INSERT, UPDATE, DELETE, or other non-SELECT queries"
                .to_string(),
        parameters: vec![Parameter {
            name: "rows_affected".to_string(),
            type_hint: "number".to_string(),
            description: "Number of rows affected by the query".to_string(),
            required: false,
        }],
        example: json!({
            "type": "mssql_ok_response",
            "rows_affected": 1
        }),
    }
}

/// Action definition: Close current MSSQL connection
pub fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current MSSQL connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
}

/// Action definition: List MSSQL connections
pub fn list_mssql_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_mssql_connections".to_string(),
        description: "List all active MSSQL connections".to_string(),
        parameters: vec![],
        example: json!({"type": "list_mssql_connections"}),
    }
}

// ============================================================================
// MSSQL Action Constants
// ============================================================================

/// MSSQL query response action constant
pub static MSSQL_QUERY_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mssql_query_response".to_string(),
        description: "Send a result set in response to a SELECT query".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Array of column definitions. Each column should have 'name' and 'type'".to_string(),
                required: true,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of rows".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mssql_query_response",
            "columns": [{"name": "id", "type": "INT"}, {"name": "name", "type": "NVARCHAR"}],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
});

/// MSSQL error response action constant
pub static MSSQL_ERROR_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mssql_error_response".to_string(),
        description: "Send an error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "error_number".to_string(),
                type_hint: "number".to_string(),
                description: "MSSQL error number".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
            Parameter {
                name: "severity".to_string(),
                type_hint: "number".to_string(),
                description: "Error severity level (1-25)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "mssql_error_response",
            "error_number": 208,
            "message": "Invalid object name"
        }),
    }
});

/// MSSQL OK response action constant
pub static MSSQL_OK_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "mssql_ok_response".to_string(),
        description: "Send a completion response for non-SELECT queries".to_string(),
        parameters: vec![Parameter {
            name: "rows_affected".to_string(),
            type_hint: "number".to_string(),
            description: "Number of rows affected".to_string(),
            required: false,
        }],
        example: json!({
            "type": "mssql_ok_response",
            "rows_affected": 1
        }),
    }
});

/// MSSQL close connection action constant
pub static MSSQL_CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current MSSQL connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
});

// ============================================================================
// MSSQL Event Type Constants
// ============================================================================

/// MSSQL query event - triggered when client sends a query
pub static MSSQL_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("mssql_query", "MSSQL query received from client", json!({"type": "placeholder", "event_id": "mssql_query"}))
        .with_parameters(vec![Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The SQL query string sent by the client".to_string(),
            required: true,
        }])
        .with_actions(vec![
            MSSQL_QUERY_RESPONSE_ACTION.clone(),
            MSSQL_ERROR_RESPONSE_ACTION.clone(),
            MSSQL_OK_RESPONSE_ACTION.clone(),
            MSSQL_CLOSE_CONNECTION_ACTION.clone(),
        ])
});

/// Get MSSQL event types
pub fn get_mssql_event_types() -> Vec<EventType> {
    vec![MSSQL_QUERY_EVENT.clone()]
}
