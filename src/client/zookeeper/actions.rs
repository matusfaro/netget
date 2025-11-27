//! ZooKeeper client protocol actions

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

// Event type constants
pub static ZOOKEEPER_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_connected",
        "ZooKeeper client connected to server",
        json!({
            "type": "wait_for_more"
        }),
    )
});

pub static ZOOKEEPER_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_data_received",
        "ZooKeeper client received data from server",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "ZNode path".to_string(),
            required: true,
        },
        Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "ZNode data".to_string(),
            required: true,
        },
        Parameter {
            name: "version".to_string(),
            type_hint: "integer".to_string(),
            description: "Data version".to_string(),
            required: true,
        },
    ])
});

pub static ZOOKEEPER_CLIENT_CHILDREN_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_children_received",
        "ZooKeeper client received children list",
        json!({
            "type": "disconnect"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "ZNode path".to_string(),
            required: true,
        },
        Parameter {
            name: "children".to_string(),
            type_hint: "array".to_string(),
            description: "Array of child node names".to_string(),
            required: true,
        },
    ])
});

/// ZooKeeper client protocol implementation
pub struct ZookeeperClientProtocol;

impl ZookeeperClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for ZookeeperClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "create_znode".to_string(),
                description: "Create a ZNode at the specified path".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "ZNode path".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "ZNode data".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "create_znode",
                    "path": "/myapp/config",
                    "data": "configuration_data"
                }),
            },
            ActionDefinition {
                name: "get_data".to_string(),
                description: "Get data from a ZNode".to_string(),
                parameters: vec![Parameter {
                    name: "path".to_string(),
                    type_hint: "string".to_string(),
                    description: "ZNode path".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "get_data",
                    "path": "/myapp/config"
                }),
            },
            ActionDefinition {
                name: "set_data".to_string(),
                description: "Set data for a ZNode".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "ZNode path".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "New ZNode data".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "set_data",
                    "path": "/myapp/config",
                    "data": "new_configuration_data"
                }),
            },
            ActionDefinition {
                name: "delete_znode".to_string(),
                description: "Delete a ZNode".to_string(),
                parameters: vec![Parameter {
                    name: "path".to_string(),
                    type_hint: "string".to_string(),
                    description: "ZNode path".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "delete_znode",
                    "path": "/myapp/config"
                }),
            },
            ActionDefinition {
                name: "get_children".to_string(),
                description: "Get children of a ZNode".to_string(),
                parameters: vec![Parameter {
                    name: "path".to_string(),
                    type_hint: "string".to_string(),
                    description: "ZNode path".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "get_children",
                    "path": "/myapp"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more events from ZooKeeper".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the ZooKeeper server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "ZooKeeper"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("zookeeper_connected", "Triggered when ZooKeeper client connects", json!({"type": "placeholder", "event_id": "zookeeper_connected"})),
            EventType::new("zookeeper_data_received", "Triggered when ZooKeeper client receives data", json!({"type": "placeholder", "event_id": "zookeeper_data_received"})),
            EventType::new("zookeeper_children_received", "Triggered when ZooKeeper client receives children list", json!({"type": "placeholder", "event_id": "zookeeper_children_received"})),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>ZooKeeper"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["zookeeper", "zk", "zookeeper client", "connect to zookeeper"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("zookeeper-async v5.0 client library")
            .llm_control("ZNode operations (create, get, set, delete, getChildren)")
            .e2e_testing("Docker ZooKeeper container")
            .notes("Simplified implementation, no watch mechanism")
            .build()
    }

    fn description(&self) -> &'static str {
        "ZooKeeper client for distributed coordination"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to ZooKeeper at localhost:2181 and read /config/database"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for ZookeeperClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::zookeeper::ZookeeperClient;

            ZookeeperClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
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
            "create_znode" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing path")?;
                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .context("Missing data")?;

                Ok(ClientActionResult::Custom {
                    name: "create_znode".to_string(),
                    data: json!({
                        "path": path,
                        "data": data
                    }),
                })
            }
            "get_data" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing path")?;

                Ok(ClientActionResult::Custom {
                    name: "get_data".to_string(),
                    data: json!({
                        "path": path
                    }),
                })
            }
            "set_data" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing path")?;
                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .context("Missing data")?;

                Ok(ClientActionResult::Custom {
                    name: "set_data".to_string(),
                    data: json!({
                        "path": path,
                        "data": data
                    }),
                })
            }
            "delete_znode" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing path")?;

                Ok(ClientActionResult::Custom {
                    name: "delete_znode".to_string(),
                    data: json!({
                        "path": path
                    }),
                })
            }
            "get_children" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing path")?;

                Ok(ClientActionResult::Custom {
                    name: "get_children".to_string(),
                    data: json!({
                        "path": path
                    }),
                })
            }
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
