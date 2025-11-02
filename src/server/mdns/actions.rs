//! mDNS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// mDNS protocol action handler
pub struct MdnsProtocol;

impl MdnsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for MdnsProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::mdns::MdnsServer;
            MdnsServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![register_mdns_service_action()]
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

    fn get_event_types(&self) -> Vec<EventType> {
        get_mdns_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>mDNS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["mdns", "bonjour", "dns-sd", "zeroconf"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hickory-proto for multicast DNS")
            .llm_control("Service announcements + responses")
            .e2e_testing("mdns-sd or avahi")
            .notes("Multicast service discovery")
            .build()
    }

    fn description(&self) -> &'static str {
        "Multicast DNS service discovery server"
    }

    fn example_prompt(&self) -> &'static str {
        "Advertise a web service via mDNS on port 8080"
    }

    fn group_name(&self) -> &'static str {
        "Application"
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
                description: "Service type (e.g., '_http._tcp.local.', '_ftp._tcp.local.')"
                    .to_string(),
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

// ============================================================================
// mDNS Action Constants
// ============================================================================

pub static REGISTER_MDNS_SERVICE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| register_mdns_service_action());

// ============================================================================
// mDNS Event Type Constants
// ============================================================================

/// mDNS server startup event - triggered when mDNS server starts
pub static MDNS_SERVER_STARTUP_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "mdns_server_startup",
        "mDNS server starting - register services for network discovery"
    )
    // No parameters - just startup notification
    .with_actions(vec![
        REGISTER_MDNS_SERVICE_ACTION.clone(),
    ])
});

/// Get mDNS event types
pub fn get_mdns_event_types() -> Vec<EventType> {
    vec![
        MDNS_SERVER_STARTUP_EVENT.clone(),
    ]
}
