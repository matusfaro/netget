//! ZooKeeper client protocol actions

use crate::llm::actions::client_trait::{
    ActionDefinition, Client, ClientActionResult, ConnectContext,
};
use crate::protocol::metadata::EventParameter;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

// Event type constants
pub static ZOOKEEPER_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_connected",
        "ZooKeeper client connected to server",
    )
});

pub static ZOOKEEPER_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_data_received",
        "ZooKeeper client received data from server",
    )
    .with_parameters(vec![
        EventParameter::new("path", "string", "ZNode path"),
        EventParameter::new("data", "string", "ZNode data"),
        EventParameter::new("version", "integer", "Data version"),
    ])
});

pub static ZOOKEEPER_CLIENT_CHILDREN_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_children_received",
        "ZooKeeper client received children list",
    )
    .with_parameters(vec![
        EventParameter::new("path", "string", "ZNode path"),
        EventParameter::new("children", "array", "Array of child node names"),
    ])
});

/// ZooKeeper client protocol implementation
pub struct ZookeeperClientProtocol;

impl ZookeeperClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for ZookeeperClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::zookeeper::ZookeeperClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "create_znode".to_string(),
                description: "Create a ZNode at the specified path".to_string(),
                parameters: vec![
                    ("path".to_string(), "string".to_string()),
                    ("data".to_string(), "string".to_string()),
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
                parameters: vec![("path".to_string(), "string".to_string())],
                example: json!({
                    "type": "get_data",
                    "path": "/myapp/config"
                }),
            },
            ActionDefinition {
                name: "set_data".to_string(),
                description: "Set data for a ZNode".to_string(),
                parameters: vec![
                    ("path".to_string(), "string".to_string()),
                    ("data".to_string(), "string".to_string()),
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
                parameters: vec![("path".to_string(), "string".to_string())],
                example: json!({
                    "type": "delete_znode",
                    "path": "/myapp/config"
                }),
            },
            ActionDefinition {
                name: "get_children".to_string(),
                description: "Get children of a ZNode".to_string(),
                parameters: vec![("path".to_string(), "string".to_string())],
                example: json!({
                    "type": "get_children",
                    "path": "/myapp"
                }),
            },
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
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

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "create_znode" => {
                let path = action
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing path"))?;
                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing data"))?;

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
                    .ok_or_else(|| anyhow!("Missing path"))?;

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
                    .ok_or_else(|| anyhow!("Missing path"))?;
                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing data"))?;

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
                    .ok_or_else(|| anyhow!("Missing path"))?;

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
                    .ok_or_else(|| anyhow!("Missing path"))?;

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

    fn get_event_types(&self) -> Vec<&'static EventType> {
        vec![
            &ZOOKEEPER_CLIENT_CONNECTED_EVENT,
            &ZOOKEEPER_CLIENT_DATA_RECEIVED_EVENT,
            &ZOOKEEPER_CLIENT_CHILDREN_RECEIVED_EVENT,
        ]
    }

    fn protocol_name(&self) -> &str {
        "ZooKeeper"
    }

    fn stack_name(&self) -> &str {
        "Application"
    }

    fn get_startup_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }
}
