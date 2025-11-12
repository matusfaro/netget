//! ZooKeeper server protocol actions

use crate::llm::actions::protocol_trait::{ActionDefinition, ActionResult, Server};
use crate::protocol::metadata::{EventParameter, EventType, ProtocolMetadataV2, ProtocolState};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::sync::LazyLock;

// Event type constants
pub static ZOOKEEPER_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_request",
        "ZooKeeper client sent a request (create, delete, getData, setData, etc.)",
    )
    .with_parameters(vec![
        EventParameter::new("operation", "string", "Operation type (create, delete, getData, setData, etc.)"),
        EventParameter::new("path", "string", "ZNode path (e.g., /myapp/config)"),
        EventParameter::new("data_hex", "string", "Request data in hex format"),
    ])
});

/// ZooKeeper protocol implementation
pub struct ZookeeperProtocol;

impl ZookeeperProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for ZookeeperProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            // Response action
            ActionDefinition {
                name: "zookeeper_response".to_string(),
                description: "Send a ZooKeeper response to the client".to_string(),
                parameters: vec![
                    ("xid".to_string(), "integer".to_string()),
                    ("zxid".to_string(), "integer".to_string()),
                    ("error_code".to_string(), "integer".to_string()),
                    ("data_hex".to_string(), "string".to_string()),
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

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing action type"))?;

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

    fn get_event_types(&self) -> Vec<&'static EventType> {
        vec![&ZOOKEEPER_REQUEST_EVENT]
    }

    fn protocol_name(&self) -> &str {
        "ZooKeeper"
    }

    fn stack_name(&self) -> &str {
        "Application"
    }

    fn keywords(&self) -> Vec<&str> {
        vec!["zookeeper", "zk"]
    }

    fn default_port(&self) -> u16 {
        2181
    }

    fn metadata_v2(&self) -> ProtocolMetadataV2 {
        ProtocolMetadataV2::builder()
            .state(ProtocolState::Experimental)
            .implementation("Manual ZooKeeper binary protocol parsing")
            .llm_control("ZNode operations (create, delete, getData, setData, getChildren)")
            .e2e_testing("zookeeper-async Rust client")
            .notes("Binary protocol with Jute serialization, no persistent storage")
            .build()
    }

    fn get_startup_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }
}
