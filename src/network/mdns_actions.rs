//! mDNS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use tracing::debug;

/// mDNS protocol action handler
pub struct MdnsProtocol;

impl MdnsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for MdnsProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            register_mdns_service_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        // mDNS is advertisement-based, no sync actions needed
        Vec::new()
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "register_mdns_service" => {
                // This action is handled in mdns.rs during server startup
                debug!("mDNS service registration action received");
                // Return empty since this action doesn't produce protocol output
                Ok(ActionResult::Output(Vec::new()))
            }
            _ => Err(anyhow::anyhow!("Unknown mDNS action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "mDNS"
    }
}

// Action definitions

fn register_mdns_service_action() -> ActionDefinition {
    ActionDefinition {
        name: "register_mdns_service".to_string(),
        description: "Register an mDNS/DNS-SD service for network discovery".to_string(),
        parameters: vec![
            Parameter {
                name: "service_type".to_string(),
                type_hint: "string".to_string(),
                description: "Service type (e.g., '_http._tcp.local.', '_ftp._tcp.local.')".to_string(),
                required: true,
            },
            Parameter {
                name: "instance_name".to_string(),
                type_hint: "string".to_string(),
                description: "Service instance name (e.g., 'My Web Server')".to_string(),
                required: true,
            },
            Parameter {
                name: "port".to_string(),
                type_hint: "number".to_string(),
                description: "Port number where service is available".to_string(),
                required: true,
            },
            Parameter {
                name: "properties".to_string(),
                type_hint: "object".to_string(),
                description: "TXT record properties (key-value pairs)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "register_mdns_service",
            "service_type": "_http._tcp.local.",
            "instance_name": "My Web Server",
            "port": 8080,
            "properties": {
                "path": "/",
                "version": "1.0"
            }
        }),
    }
}
