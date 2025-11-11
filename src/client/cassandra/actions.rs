//! Cassandra client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Cassandra client connected event
pub static CASSANDRA_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_connected",
        "Cassandra client successfully connected to server",
    )
    .with_parameters(vec![Parameter {
        name: "remote_addr".to_string(),
        type_hint: "string".to_string(),
        description: "Cassandra server address".to_string(),
        required: true,
    }])
});

/// Cassandra client query result received event
pub static CASSANDRA_CLIENT_RESULT_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "cassandra_result_received",
        "Query result received from Cassandra server",
    )
    .with_parameters(vec![
        Parameter {
            name: "rows".to_string(),
            type_hint: "array".to_string(),
            description: "Query result rows".to_string(),
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

/// Cassandra client protocol action handler
pub struct CassandraClientProtocol;

impl CassandraClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for CassandraClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_cql_query".to_string(),
                description: "Execute a CQL query".to_string(),
                parameters: vec![
                    Parameter {
                        name: "query".to_string(),
                        type_hint: "string".to_string(),
                        description: "CQL query (e.g., SELECT * FROM keyspace.table WHERE id = 1)"
                            .to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "consistency".to_string(),
                        type_hint: "string".to_string(),
                        description: "Consistency level (ONE, QUORUM, ALL, etc.)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "execute_cql_query",
                    "query": "SELECT * FROM system.local",
                    "consistency": "ONE"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the Cassandra server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more results or do nothing".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_cql_query".to_string(),
                description: "Execute a CQL query in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "query".to_string(),
                        type_hint: "string".to_string(),
                        description: "CQL query".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "consistency".to_string(),
                        type_hint: "string".to_string(),
                        description: "Consistency level".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "execute_cql_query",
                    "query": "SELECT * FROM users WHERE id = 1"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more results".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Cassandra"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "cassandra_connected".to_string(),
                description: "Triggered when Cassandra client connects to server".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "cassandra_result_received".to_string(),
                description: "Triggered when Cassandra client receives query results".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>CASSANDRA"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "cassandra",
            "cassandra client",
            "connect to cassandra",
            "cql",
            "scylla",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("ScyllaDB Rust driver (scylla crate)")
            .llm_control("Full control over CQL queries with consistency levels")
            .e2e_testing("Docker Cassandra container")
            .build()
    }
    fn description(&self) -> &'static str {
        "Cassandra/ScyllaDB client for CQL queries"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to Cassandra at localhost:9042 and query system.local table"
    }
    fn group_name(&self) -> &'static str {
        "Database"
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "keyspace".to_string(),
                type_hint: "string".to_string(),
                description: "Default keyspace to use".to_string(),
                required: false,
                example: json!("my_keyspace"),
            },
            ParameterDefinition {
                name: "username".to_string(),
                type_hint: "string".to_string(),
                description: "Username for authentication".to_string(),
                required: false,
                example: json!("cassandra"),
            },
            ParameterDefinition {
                name: "password".to_string(),
                type_hint: "string".to_string(),
                description: "Password for authentication".to_string(),
                required: false,
                example: json!("cassandra"),
            },
        ]
    }
}

// Implement Client trait (client-specific functionality)
impl Client for CassandraClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::cassandra::CassandraClient;
            CassandraClient::connect_with_llm_actions(
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
            "execute_cql_query" => {
                let query = action
                    .get("query")
                    .and_then(|v| v.as_str())
                    .context("Missing 'query' field")?
                    .to_string();

                let consistency = action
                    .get("consistency")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "cql_query".to_string(),
                    data: json!({
                        "query": query,
                        "consistency": consistency,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown Cassandra client action: {}",
                action_type
            )),
        }
    }
}
