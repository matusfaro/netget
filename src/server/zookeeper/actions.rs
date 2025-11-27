//! ZooKeeper server protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

// Event type constants
pub static ZOOKEEPER_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_request",
        "ZooKeeper client sent a request (create, delete, getData, setData, etc.)",
        json!({
            "type": "zookeeper_response",
            "xid": 1,
            "zxid": 100,
            "error_code": 0,
            "data_hex": "0000000000000064"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "Operation type (create, delete, getData, setData, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "ZNode path (e.g., /myapp/config)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Request data in hex format".to_string(),
            required: false,
        },
    ])
});

/// ZooKeeper protocol implementation
pub struct ZookeeperProtocol;

impl ZookeeperProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for ZookeeperProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "zookeeper_response".to_string(),
                description: "Send a ZooKeeper response to the client".to_string(),
                parameters: vec![
                    Parameter {
                        name: "xid".to_string(),
                        type_hint: "integer".to_string(),
                        description: "Transaction ID".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "zxid".to_string(),
                        type_hint: "integer".to_string(),
                        description: "ZooKeeper transaction ID".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "error_code".to_string(),
                        type_hint: "integer".to_string(),
                        description: "Error code (0 = success)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Response data in hex format".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "zookeeper_response",
                    "xid": 1,
                    "zxid": 100,
                    "error_code": 0,
                    "data_hex": "0000000000000064"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "ZooKeeper"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![(*ZOOKEEPER_REQUEST_EVENT).clone()]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>ZooKeeper"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["zookeeper", "zk"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual ZooKeeper binary protocol parsing")
            .llm_control("ZNode operations (create, delete, getData, setData, getChildren)")
            .e2e_testing("zookeeper-async Rust client")
            .notes("Binary protocol with Jute serialization, no persistent storage")
            .build()
    }

    fn description(&self) -> &'static str {
        "ZooKeeper distributed coordination server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a ZooKeeper server on port 2181"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for ZookeeperProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::zookeeper::ZookeeperServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            ZookeeperServer::spawn_with_llm_actions(
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
            "zookeeper_response" => {
                let xid = action
                    .get("xid")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let zxid = action
                    .get("zxid")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i64;
                let error_code = action
                    .get("error_code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Build response: xid (4) + zxid (8) + error_code (4) + data
                let mut response = Vec::new();
                response.extend_from_slice(&xid.to_be_bytes());
                response.extend_from_slice(&zxid.to_be_bytes());
                response.extend_from_slice(&error_code.to_be_bytes());

                if !data_hex.is_empty() {
                    if let Ok(data_bytes) = hex::decode(data_hex) {
                        response.extend_from_slice(&data_bytes);
                    }
                }

                Ok(ActionResult::Custom {
                    name: "zookeeper_response".to_string(),
                    data: json!({
                        "response_hex": hex::encode(&response)
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
