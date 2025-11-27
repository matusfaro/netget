//! PostgreSQL client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::{ConnectContext, EventType};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// PostgreSQL client connected event
pub static POSTGRESQL_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "postgresql_connected",
        "PostgreSQL client successfully connected to server",
        json!({
            "type": "execute_query",
            "query": "SELECT * FROM users WHERE id = 1"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "PostgreSQL server address".to_string(),
            required: true,
        },
        Parameter {
            name: "database".to_string(),
            type_hint: "string".to_string(),
            description: "Database name".to_string(),
            required: true,
        },
        Parameter {
            name: "user".to_string(),
            type_hint: "string".to_string(),
            description: "Username".to_string(),
            required: true,
        },
    ])
});

/// PostgreSQL client query result event
pub static POSTGRESQL_CLIENT_QUERY_RESULT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "postgresql_query_result",
        "Query result received from PostgreSQL server",
        json!({
            "type": "execute_query",
            "query": "INSERT INTO logs VALUES ('result processed')"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The SQL query executed".to_string(),
            required: true,
        },
        Parameter {
            name: "rows".to_string(),
            type_hint: "array".to_string(),
            description: "Array of row objects".to_string(),
            required: true,
        },
        Parameter {
            name: "row_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of rows returned".to_string(),
            required: true,
        },
    ])
});

/// PostgreSQL client protocol action handler
#[derive(Default)]
pub struct PostgresqlClientProtocol;

impl PostgresqlClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for PostgresqlClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_query".to_string(),
                description: "Execute a SQL query (SELECT, INSERT, UPDATE, DELETE, etc.)"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "query".to_string(),
                    type_hint: "string".to_string(),
                    description: "SQL query to execute".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "execute_query",
                    "query": "SELECT * FROM users WHERE id = 1"
                }),
            },
            ActionDefinition {
                name: "begin_transaction".to_string(),
                description: "Begin a new transaction".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "begin_transaction"
                }),
            },
            ActionDefinition {
                name: "commit_transaction".to_string(),
                description: "Commit the current transaction".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "commit_transaction"
                }),
            },
            ActionDefinition {
                name: "rollback_transaction".to_string(),
                description: "Roll back the current transaction".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "rollback_transaction"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the PostgreSQL server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![ActionDefinition {
            name: "execute_query".to_string(),
            description: "Execute a SQL query in response to query results".to_string(),
            parameters: vec![Parameter {
                name: "query".to_string(),
                type_hint: "string".to_string(),
                description: "SQL query to execute".to_string(),
                required: true,
            }],
            example: json!({
                "type": "execute_query",
                "query": "INSERT INTO logs VALUES ('result processed')"
            }),
        }]
    }
    fn protocol_name(&self) -> &'static str {
        "PostgreSQL"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("postgresql_connected", "Triggered when PostgreSQL client connects to server", json!({"type": "placeholder", "event_id": "postgresql_connected"})),
            EventType::new("postgresql_query_result", "Triggered when PostgreSQL client receives query results", json!({"type": "placeholder", "event_id": "postgresql_query_result"})),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>PostgreSQL"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "postgresql",
            "postgres",
            "postgresql client",
            "postgres client",
            "connect to postgresql",
            "connect to postgres",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("tokio-postgres library with LLM integration")
            .llm_control("Full control over SQL queries and transactions")
            .e2e_testing("Docker PostgreSQL container")
            .build()
    }
    fn description(&self) -> &'static str {
        "PostgreSQL client for relational database operations"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to PostgreSQL at localhost:5432 and select all users from the users table"
    }
    fn group_name(&self) -> &'static str {
        "Database"
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "database".to_string(),
                type_hint: "string".to_string(),
                description: "Database name (default: postgres)".to_string(),
                required: false,
                example: json!("mydb"),
            },
            ParameterDefinition {
                name: "user".to_string(),
                type_hint: "string".to_string(),
                description: "Username (default: postgres)".to_string(),
                required: false,
                example: json!("admin"),
            },
            ParameterDefinition {
                name: "password".to_string(),
                type_hint: "string".to_string(),
                description: "Password (default: empty)".to_string(),
                required: false,
                example: json!("secret123"),
            },
        ]
    }
}

// Implement Client trait (client-specific functionality)
impl Client for PostgresqlClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::postgresql::PostgresqlClient;
            PostgresqlClient::connect_with_llm_actions(
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
            "execute_query" => {
                let query = action
                    .get("query")
                    .and_then(|v| v.as_str())
                    .context("Missing 'query' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "pg_query".to_string(),
                    data: json!({
                        "query": query,
                    }),
                })
            }
            "begin_transaction" => Ok(ClientActionResult::Custom {
                name: "pg_query".to_string(),
                data: json!({
                    "query": "BEGIN",
                }),
            }),
            "commit_transaction" => Ok(ClientActionResult::Custom {
                name: "pg_query".to_string(),
                data: json!({
                    "query": "COMMIT",
                }),
            }),
            "rollback_transaction" => Ok(ClientActionResult::Custom {
                name: "pg_query".to_string(),
                data: json!({
                    "query": "ROLLBACK",
                }),
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!(
                "Unknown PostgreSQL client action: {}",
                action_type
            )),
        }
    }
}
