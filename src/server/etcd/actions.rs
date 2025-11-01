//! etcd protocol action definitions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// etcd protocol handler
pub struct EtcdProtocol {}

impl EtcdProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Event type IDs
pub static ETCD_RANGE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "etcd_range_request",
        "Triggered when a client sends a Range (get) request to query keys"
    )
    .with_parameters(vec![
        Parameter {
            name: "key".to_string(),
            type_hint: "string".to_string(),
            description: "Key to query".to_string(),
            required: true,
        },
        Parameter {
            name: "range_end".to_string(),
            type_hint: "string".to_string(),
            description: "End of key range (for prefix/range queries)".to_string(),
            required: false,
        },
        Parameter {
            name: "limit".to_string(),
            type_hint: "number".to_string(),
            description: "Maximum number of keys to return".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        etcd_range_response_action(),
        etcd_error_action(),
    ])
});

pub static ETCD_PUT_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "etcd_put_request",
        "Triggered when a client sends a Put request to store a key-value pair"
    )
});

pub static ETCD_DELETE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "etcd_delete_request",
        "Triggered when a client sends a DeleteRange request"
    )
});

pub static ETCD_TXN_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "etcd_txn_request",
        "Triggered when a client sends a transaction request"
    )
});

// Action definitions
fn etcd_range_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "etcd_range_response".to_string(),
        description: "Return key-value pairs for a Range request".to_string(),
        parameters: vec![
            Parameter {
                name: "kvs".to_string(),
                type_hint: "array".to_string(),
                description: "Array of key-value objects with 'key', 'value', 'create_revision', 'mod_revision', 'version', 'lease' fields".to_string(),
                required: true,
            },
            Parameter {
                name: "more".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether there are more keys to return".to_string(),
                required: false,
            },
            Parameter {
                name: "count".to_string(),
                type_hint: "number".to_string(),
                description: "Total count of keys matching the range".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "etcd_range_response",
            "kvs": [
                {"key": "foo", "value": "bar", "create_revision": 1, "mod_revision": 1, "version": 1, "lease": 0}
            ],
            "more": false,
            "count": 1
        }),
    }
}

fn etcd_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "etcd_error".to_string(),
        description: "Return an error response".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description: "Error code (e.g., 'KEY_NOT_FOUND', 'INVALID_ARGUMENT')".to_string(),
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
            "type": "etcd_error",
            "code": "KEY_NOT_FOUND",
            "message": "etcdserver: key not found"
        }),
    }
}

impl Server for EtcdProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::etcd::EtcdServer;
            EtcdServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            ).await
        })
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "cluster_name".to_string(),
                type_hint: "string".to_string(),
                description: "Cluster identifier name (default: netget-cluster)".to_string(),
                required: false,
                example: json!("my-cluster"),
            },
            ParameterDefinition {
                name: "initial_cluster_state".to_string(),
                type_hint: "string".to_string(),
                description: "Initial cluster state: 'new' or 'existing' (default: new)".to_string(),
                required: false,
                example: json!("new"),
            },
            ParameterDefinition {
                name: "max_keys".to_string(),
                type_hint: "number".to_string(),
                description: "Maximum number of keys to store (default: 10000)".to_string(),
                required: false,
                example: json!(10000),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            etcd_range_response_action(),
            etcd_error_action(),
        ]
    }

    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action.get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;

        match action_type {
            "etcd_range_response" => {
                Ok(ActionResult::Custom {
                    name: "etcd_range_response".to_string(),
                    data: action,
                })
            }
            "etcd_error" => {
                Ok(ActionResult::Custom {
                    name: "etcd_error".to_string(),
                    data: action,
                })
            }
            _ => anyhow::bail!("Unknown etcd action type: {}", action_type),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "etcd"
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>GRPC>ETCD"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["etcd", "etcd3", "etcdv3", "etcd server"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::with_notes(
            crate::protocol::metadata::DevelopmentState::Alpha,
            "etcd v3 KV service only - no Watch, Lease, or Auth services yet"
        )
    }

    fn description(&self) -> &'static str {
        "etcd v3 distributed key-value store server"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            (*ETCD_RANGE_REQUEST_EVENT).clone(),
            (*ETCD_PUT_REQUEST_EVENT).clone(),
            (*ETCD_DELETE_REQUEST_EVENT).clone(),
            (*ETCD_TXN_REQUEST_EVENT).clone(),
        ]
    }

    fn example_prompt(&self) -> &'static str {
        r#"listen on port 2379 via etcd

Store configuration under /config/ prefix.
When clients PUT /config/database with value "localhost:5432", store it (revision 1).
When clients GET /config/database, return "localhost:5432" with revision metadata.
For unknown keys, return empty kvs array.

Examples:
- PUT /config/timeout = "30" → Success, increment revision
- GET /config/timeout → Return "30" with create_revision, mod_revision, version
- DELETE /config/timeout → Remove key, return deleted count
- Range query /config/ → Return all keys with /config/ prefix

Track revisions for MVCC."#
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}
