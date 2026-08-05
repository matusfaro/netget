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
    /// Turn an action's `data` field into the bytes to put on the wire.
    ///
    /// `encoding` selects the interpretation:
    /// - `"text"`  - the string's UTF-8 bytes, verbatim
    /// - `"hex"`   - hex-decoded, and an error if it is not valid hex
    /// - absent or `"auto"` - hex if the string happens to parse as hex, otherwise text
    ///
    /// `auto` is the historical behaviour and stays the default so existing prompts and
    /// handlers keep working, but it is genuinely ambiguous and worth avoiding: any
    /// even-length string of hex digits is taken as hex. `{"data": "1234"}` puts two bytes
    /// (0x12 0x34) on the wire, not the four characters "1234"; so do "abcd", "DEADBEEF" and
    /// "0000". Pass `"encoding": "text"` whenever the payload is text.
    fn decode_payload(action: &serde_json::Value) -> Result<Vec<u8>> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        match action.get("encoding").and_then(|v| v.as_str()) {
            Some("text") => Ok(data.as_bytes().to_vec()),
            Some("hex") => hex::decode(data)
                .context("encoding is 'hex' but 'data' is not valid hex")
                .map_err(Into::into),
            Some(other) if other != "auto" => Err(anyhow::anyhow!(
                "Unknown encoding '{}': expected 'text', 'hex' or 'auto'",
                other
            )),
            _ => Ok(hex::decode(data).unwrap_or_else(|_| data.as_bytes().to_vec())),
        }
    }

    /// Execute send_to_address async action
    fn execute_send_to_address(&self, action: serde_json::Value) -> Result<ActionResult> {
        let address = action
            .get("address")
            .and_then(|v| v.as_str())
            .context("Missing 'address' parameter")?;

        let _addr: SocketAddr = address.parse().context("Invalid socket address format")?;

        // NOTE: the parsed address is discarded. The caller in mod.rs sends every Output back
        // to the peer that sent the current datagram, so this action cannot actually target a
        // different address despite its name and documentation.
        Ok(ActionResult::Output(Self::decode_payload(&action)?))
    }

    /// Execute send_udp_response sync action
    fn execute_send_udp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::Output(Self::decode_payload(&action)?))
    }
}

/// The `encoding` parameter shared by the two sending actions.
fn encoding_parameter() -> Parameter {
    Parameter {
        name: "encoding".to_string(),
        type_hint: "string".to_string(),
        description: "How to read 'data': 'text' for literal UTF-8, 'hex' for hex-decoded \
                      binary, 'auto' (default) to guess. Prefer being explicit - under 'auto' \
                      any even-length run of hex digits is taken as hex, so \"1234\" sends two \
                      bytes rather than four characters."
            .to_string(),
        required: false,
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
                description: "Data to send (see 'encoding')".to_string(),
                required: true,
            },
            encoding_parameter(),
        ],
        example: json!({
            "type": "send_to_address",
            "address": "127.0.0.1:8080",
            "data": "Hello from UDP",
            "encoding": "text"
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
        parameters: vec![
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Response payload (see 'encoding')".to_string(),
                required: true,
            },
            encoding_parameter(),
        ],
        example: json!({
            "type": "send_udp_response",
            "data": "Response data",
            "encoding": "text"
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
        log_template: Some(LogTemplate::new().with_debug("UDP ignore_datagram")),
    }
}

// ============================================================================
// UDP Event Type Constants
// ============================================================================

pub static UDP_DATAGRAM_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "udp_datagram_received",
        "UDP datagram received from a peer. Reply in the same encoding the event reports.",
        json!({
            "type": "send_udp_response",
            "data": "PONG",
            "encoding": "text"
        }),
    )
    .with_alternative_example(json!({
        "type": "send_udp_response",
        "data": "48656c6c6f",
        "encoding": "hex"
    }))
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
            name: "data_encoding".to_string(),
            type_hint: "string".to_string(),
            description: "How data_preview is rendered: 'text' if the payload is printable \
                          ASCII, otherwise 'hex'. Use the same value when replying."
                .to_string(),
            required: true,
        },
        Parameter {
            name: "data_preview".to_string(),
            type_hint: "string".to_string(),
            description: "The received payload, as text or hex per data_encoding, truncated \
                          to the first 200 bytes with a trailing '...'"
                .to_string(),
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
