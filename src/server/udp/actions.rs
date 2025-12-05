//! UDP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use tokio::net::UdpSocket;

/// UDP protocol action handler
pub struct UdpProtocol {
    /// Shared UDP socket for async actions
    #[allow(dead_code)]
    socket: Option<Arc<UdpSocket>>,
}

impl Default for UdpProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl UdpProtocol {
    pub fn new() -> Self {
        Self { socket: None }
    }

    pub fn with_socket(socket: Arc<UdpSocket>) -> Self {
        Self {
            socket: Some(socket),
        }
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for UdpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![send_to_address_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_udp_response_action(), ignore_datagram_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "UDP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_udp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["udp"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .implementation("Manual UDP socket handling with tokio")
            .llm_control("Full datagram control - all sent/received data")
            .e2e_testing("std::net::UdpSocket")
            .notes("Stateless, used by DNS/DHCP/NTP")
            .build()
    }
    fn description(&self) -> &'static str {
        "UDP datagram server"
    }
    fn example_prompt(&self) -> &'static str {
        "Listen on port 5000 via UDP"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            json!({
                "type": "open_server",
                "port": 9000,
                "base_stack": "udp",
                "instruction": "UDP echo server that responds to datagrams"
            }),
            json!({
                "type": "open_server",
                "port": 9000,
                "base_stack": "udp",
                "event_handlers": [{
                    "event_pattern": "udp_datagram_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<udp_handler>"
                    }
                }]
            }),
            json!({
                "type": "open_server",
                "port": 9000,
                "base_stack": "udp",
                "event_handlers": [{
                    "event_pattern": "udp_datagram_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_udp_response",
                            "data": "PONG"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for UdpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::udp::UdpServer;
            UdpServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
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
            "send_to_address" => self.execute_send_to_address(action),
            "send_udp_response" => self.execute_send_udp_response(action),
            "ignore_datagram" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown UDP action: {}", action_type)),
        }
    }
}

impl UdpProtocol {
    /// Execute send_to_address async action
    fn execute_send_to_address(&self, action: serde_json::Value) -> Result<ActionResult> {
        let address = action
            .get("address")
            .and_then(|v| v.as_str())
            .context("Missing 'address' parameter")?;

        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        let _addr: SocketAddr = address.parse().context("Invalid socket address format")?;

        // For async actions, we need to return the data and let the caller handle sending
        // This is because we need the socket reference from the network handler
        // We'll encode the target address in the result
        // Try to decode as hex first, fall back to raw string
        let bytes = hex::decode(data).unwrap_or_else(|_| data.as_bytes().to_vec());
        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_udp_response sync action
    fn execute_send_udp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first, fall back to raw string
        let bytes = hex::decode(data).unwrap_or_else(|_| data.as_bytes().to_vec());

        Ok(ActionResult::Output(bytes))
    }
}

/// Action definition for send_to_address
fn send_to_address_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_address".to_string(),
        description: "Send UDP datagram to a specific address (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "address".to_string(),
                type_hint: "string".to_string(),
                description: "Target address in format 'IP:port' (e.g., '127.0.0.1:8080')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Data to send".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_to_address",
            "address": "127.0.0.1:8080",
            "data": "Hello from UDP"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> UDP to {address}")
                .with_debug("UDP send_to_address: address={address}"),
        ),
    }
}

/// Action definition for send_udp_response
fn send_udp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_udp_response".to_string(),
        description: "Send UDP response back to the peer that sent the current datagram"
            .to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Response data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_udp_response",
            "data": "Response data"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> UDP response {output_bytes}B")
                .with_debug("UDP send_udp_response: {output_bytes}B")
                .with_trace("UDP response: {preview(data,200)}"),
        ),
    }
}

/// Action definition for ignore_datagram
fn ignore_datagram_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_datagram".to_string(),
        description: "Ignore this datagram and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_datagram"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_debug("UDP ignore_datagram"),
        ),
    }
}

// ============================================================================
// UDP Event Type Constants
// ============================================================================

pub static UDP_DATAGRAM_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("udp_datagram_received", "UDP datagram received from a peer", json!({"type": "placeholder", "event_id": "udp_datagram_received"}))
        .with_parameters(vec![
            Parameter {
                name: "peer_address".to_string(),
                type_hint: "string".to_string(),
                description: "Source address of the datagram (IP:port)".to_string(),
                required: true,
            },
            Parameter {
                name: "data_length".to_string(),
                type_hint: "number".to_string(),
                description: "Length of the received data in bytes".to_string(),
                required: true,
            },
            Parameter {
                name: "data_preview".to_string(),
                type_hint: "string".to_string(),
                description: "Preview of the received data".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![send_udp_response_action(), ignore_datagram_action()])
        .with_log_template(
            LogTemplate::new()
                .with_info("UDP {data_length}B from {peer_address}")
                .with_debug("UDP datagram: {data_length}B from {peer_address}")
                .with_trace("UDP data: {preview(data_preview,200)}"),
        )
});

pub fn get_udp_event_types() -> Vec<EventType> {
    vec![UDP_DATAGRAM_RECEIVED_EVENT.clone()]
}
