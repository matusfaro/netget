//! Cassandra protocol actions implementation

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

/// Cassandra protocol action handler
pub struct CassandraProtocol {
    _connection_id: ConnectionId,
    _app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
}

impl CassandraProtocol {
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

impl ProtocolActions for CassandraProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_cassandra_connections_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            cassandra_ready_action(),
            cassandra_supported_action(),
            cassandra_result_rows_action(),
            cassandra_prepared_action(),
            cassandra_auth_success_action(),
            cassandra_error_action(),
            close_this_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "cassandra_ready" => self.execute_cassandra_ready(),
            "cassandra_supported" => self.execute_cassandra_supported(action),
            "cassandra_result_rows" => self.execute_cassandra_result_rows(action),
            "cassandra_prepared" => self.execute_cassandra_prepared(action),
            "cassandra_auth_success" => self.execute_cassandra_auth_success(),
            "cassandra_error" => self.execute_cassandra_error(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "list_cassandra_connections" => self.execute_list_cassandra_connections(action),
            _ => Err(anyhow::anyhow!("Unknown Cassandra action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Cassandra"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_cassandra_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>Cassandra"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["cassandra", "cql"]
    }

    fn metadata(&self) -> crate::protocol::base_stack::ProtocolMetadata {
        crate::protocol::base_stack::ProtocolMetadata::new(
            crate::protocol::base_stack::ProtocolState::Alpha
        )
    }
}

impl CassandraProtocol {
    fn execute_cassandra_ready(&self) -> Result<ActionResult> {
        debug!("Cassandra READY response");
        let _ = self.status_tx.send(format!("[DEBUG] Cassandra → READY"));

        Ok(ActionResult::Custom {
            name: "cassandra_ready".to_string(),
            data: json!({}),
        })
    }

    fn execute_cassandra_supported(&self, action: serde_json::Value) -> Result<ActionResult> {
        let options = action
            .get("options")
            .and_then(|v| v.as_object())
            .map(|o| o.clone());

        debug!("Cassandra SUPPORTED response with options");
        let _ = self.status_tx.send(format!("[DEBUG] Cassandra → SUPPORTED"));

        Ok(ActionResult::Custom {
            name: "cassandra_supported".to_string(),
            data: json!({
                "options": options.unwrap_or_default()
            }),
        })
    }

    fn execute_cassandra_result_rows(&self, action: serde_json::Value) -> Result<ActionResult> {
        let columns = action
            .get("columns")
            .and_then(|v| v.as_array())
            .context("Missing 'columns' array")?;

        let rows = action
            .get("rows")
            .and_then(|v| v.as_array())
            .context("Missing 'rows' array")?;

        debug!(
            "Cassandra result rows: {} columns, {} rows",
            columns.len(),
            rows.len()
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] Cassandra → Result set: {} columns, {} rows",
            columns.len(),
            rows.len()
        ));

        Ok(ActionResult::Custom {
            name: "cassandra_result_rows".to_string(),
            data: json!({
                "columns": columns,
                "rows": rows
            }),
        })
    }

    fn execute_cassandra_prepared(&self, action: serde_json::Value) -> Result<ActionResult> {
        let columns = action
            .get("columns")
            .and_then(|v| v.as_array())
            .context("Missing 'columns' array")?;

        debug!(
            "Cassandra prepared statement: {} result columns",
            columns.len()
        );

        let _ = self.status_tx.send(format!(
            "[DEBUG] Cassandra → Prepared statement ({} columns)",
            columns.len()
        ));

        Ok(ActionResult::Custom {
            name: "cassandra_prepared".to_string(),
            data: json!({
                "columns": columns
            }),
        })
    }

    fn execute_cassandra_auth_success(&self) -> Result<ActionResult> {
        debug!("Cassandra authentication successful");

        let _ = self.status_tx.send(format!(
            "[DEBUG] Cassandra → AUTH_SUCCESS"
        ));

        Ok(ActionResult::Custom {
            name: "cassandra_auth_success".to_string(),
            data: json!({}),
        })
    }

    fn execute_cassandra_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0x0000) as u32;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        debug!("Cassandra error response: 0x{:04X} - {}", error_code, message);

        let _ = self.status_tx.send(format!(
            "[DEBUG] Cassandra ✗ Error 0x{:04X}: {}",
            error_code, message
        ));

        Ok(ActionResult::Custom {
            name: "cassandra_error".to_string(),
            data: json!({
                "error_code": error_code,
                "message": message
            }),
        })
    }

    fn execute_list_cassandra_connections(
        &self,
        _action: serde_json::Value,
    ) -> Result<ActionResult> {
        debug!("Listing Cassandra connections");
        let _ = self
            .status_tx
            .send(format!("[DEBUG] List Cassandra connections"));

        Ok(ActionResult::NoAction)
    }
}

// Action definitions

fn list_cassandra_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_cassandra_connections".to_string(),
        description: "List all active Cassandra connections".to_string(),
        parameters: vec![],
        example: json!({"type": "list_cassandra_connections"}),
    }
}

fn cassandra_ready_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_ready".to_string(),
        description: "Send READY response after successful STARTUP".to_string(),
        parameters: vec![],
        example: json!({"type": "cassandra_ready"}),
    }
}

fn cassandra_supported_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_supported".to_string(),
        description: "Send SUPPORTED response with server capabilities".to_string(),
        parameters: vec![Parameter {
            name: "options".to_string(),
            type_hint: "object".to_string(),
            description: "Map of supported options (e.g., CQL_VERSION, COMPRESSION)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "cassandra_supported",
            "options": {
                "CQL_VERSION": ["3.0.0"],
                "COMPRESSION": []
            }
        }),
    }
}

fn cassandra_result_rows_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_result_rows".to_string(),
        description: "Send query result with rows of data".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Column definitions with name and type".to_string(),
                required: true,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of row arrays".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "cassandra_result_rows",
            "columns": [
                {"name": "id", "type": "int"},
                {"name": "name", "type": "varchar"}
            ],
            "rows": [[1, "Alice"], [2, "Bob"]]
        }),
    }
}

fn cassandra_prepared_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_prepared".to_string(),
        description: "Send prepared statement response with result column metadata".to_string(),
        parameters: vec![
            Parameter {
                name: "columns".to_string(),
                type_hint: "array".to_string(),
                description: "Column definitions for the result set that the prepared query will return".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "cassandra_prepared",
            "columns": [
                {"name": "id", "type": "int"},
                {"name": "name", "type": "varchar"}
            ]
        }),
    }
}

fn cassandra_auth_success_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_auth_success".to_string(),
        description: "Accept authentication and send AUTH_SUCCESS".to_string(),
        parameters: vec![],
        example: json!({"type": "cassandra_auth_success"}),
    }
}

fn cassandra_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "cassandra_error".to_string(),
        description: "Send error response to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "Cassandra error code (e.g., 0x2200 for syntax error)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "cassandra_error",
            "error_code": 0x2200,
            "message": "Syntax error in CQL query"
        }),
    }
}

fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current Cassandra connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_this_connection"}),
    }
}

// Event types

pub static CASSANDRA_STARTUP_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_startup",
        "Client sends STARTUP frame with protocol version and options"
    )
    .with_parameters(vec![
        Parameter {
            name: "protocol_version".to_string(),
            type_hint: "number".to_string(),
            description: "CQL protocol version".to_string(),
            required: true,
        },
        Parameter {
            name: "options".to_string(),
            type_hint: "object".to_string(),
            description: "Startup options (e.g., CQL_VERSION)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        cassandra_ready_action(),
        cassandra_error_action(),
    ])
});

pub static CASSANDRA_OPTIONS_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_options",
        "Client requests supported protocol options"
    )
    .with_actions(vec![
        cassandra_supported_action(),
        cassandra_error_action(),
    ])
});

pub static CASSANDRA_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_query",
        "Client sends CQL query to execute"
    )
    .with_parameters(vec![
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The CQL query string".to_string(),
            required: true,
        },
        Parameter {
            name: "consistency".to_string(),
            type_hint: "string".to_string(),
            description: "Consistency level (ONE, QUORUM, ALL, etc.)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        cassandra_result_rows_action(),
        cassandra_error_action(),
        close_this_connection_action(),
    ])
});

pub static CASSANDRA_PREPARE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_prepare",
        "Client sends PREPARE frame to prepare a parameterized query"
    )
    .with_parameters(vec![
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The parameterized CQL query with ? placeholders".to_string(),
            required: true,
        },
        Parameter {
            name: "statement_id".to_string(),
            type_hint: "string".to_string(),
            description: "Generated statement ID (hex encoded)".to_string(),
            required: true,
        },
        Parameter {
            name: "param_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of parameters in the query".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        cassandra_prepared_action(),
        cassandra_error_action(),
    ])
});

pub static CASSANDRA_EXECUTE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_execute",
        "Client sends EXECUTE frame to execute a prepared statement with parameters"
    )
    .with_parameters(vec![
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The original prepared query".to_string(),
            required: true,
        },
        Parameter {
            name: "statement_id".to_string(),
            type_hint: "string".to_string(),
            description: "Statement ID (hex encoded)".to_string(),
            required: true,
        },
        Parameter {
            name: "parameters".to_string(),
            type_hint: "array".to_string(),
            description: "Bound parameter values".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        cassandra_result_rows_action(),
        cassandra_error_action(),
    ])
});

pub static CASSANDRA_AUTH_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_auth",
        "Client sends AUTH_RESPONSE with credentials (SASL PLAIN)"
    )
    .with_parameters(vec![
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Username from SASL PLAIN authentication".to_string(),
            required: true,
        },
        Parameter {
            name: "password".to_string(),
            type_hint: "string".to_string(),
            description: "Password from SASL PLAIN authentication".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        cassandra_auth_success_action(),
        cassandra_error_action(),
        close_this_connection_action(),
    ])
});

pub fn get_cassandra_event_types() -> Vec<EventType> {
    vec![
        CASSANDRA_STARTUP_EVENT.clone(),
        CASSANDRA_OPTIONS_EVENT.clone(),
        CASSANDRA_QUERY_EVENT.clone(),
        CASSANDRA_PREPARE_EVENT.clone(),
        CASSANDRA_EXECUTE_EVENT.clone(),
        CASSANDRA_AUTH_EVENT.clone(),
    ]
}
