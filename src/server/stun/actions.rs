//! STUN protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct StunProtocol;

impl StunProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl crate::llm::actions::protocol_trait::Protocol for StunProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new() // STUN server is purely reactive
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_stun_binding_response_action(),
            send_stun_error_response_action(),
            ignore_request_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "STUN"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_stun_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>STUN"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["stun"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual STUN protocol (RFC 8489)")
            .llm_control("Binding responses with XOR-MAPPED-ADDRESS")
            .e2e_testing("stuntman-client / WebRTC")
            .notes("IPv4 only, stateless UDP")
            .build()
    }

    fn description(&self) -> &'static str {
        "STUN server for NAT traversal"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a STUN server for NAT traversal on port 3478"
    }

    fn group_name(&self) -> &'static str {
        "Proxy & Network"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles all STUN responses
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "stun",
                "instruction": "STUN server for NAT traversal - respond with client's external IP"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "stun",
                "event_handlers": [{
                    "event_pattern": "stun_binding_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<stun_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "stun",
                "event_handlers": [{
                    "event_pattern": "stun_binding_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_stun_binding_response",
                            "transaction_id": "000000000000",
                            "client_address": "0.0.0.0",
                            "client_port": 0,
                            "xor_mapped": true
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for StunProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::stun::StunServer;
            StunServer::spawn_with_llm_actions(
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
            "send_stun_binding_response" => self.execute_send_binding_response(action),
            "send_stun_error_response" => self.execute_send_error_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown STUN action: {}", action_type)),
        }
    }
}

impl StunProtocol {
    /// Execute STUN binding response action
    fn execute_send_binding_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract parameters
        let mapped_address = action
            .get("mapped_address")
            .and_then(|v| v.as_str())
            .context("Missing 'mapped_address' field")?;

        let transaction_id = action
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .context("Missing 'transaction_id' field")?;

        // Optional parameters
        let xor_mapped_address = action
            .get("xor_mapped_address")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let software = action
            .get("software")
            .and_then(|v| v.as_str())
            .unwrap_or("NetGet/1.0");

        let message_integrity = action
            .get("message_integrity")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Parse transaction ID from hex
        let transaction_id_bytes =
            hex::decode(transaction_id).context("Invalid transaction_id hex")?;

        if transaction_id_bytes.len() != 12 {
            return Err(anyhow::anyhow!("Transaction ID must be 12 bytes"));
        }

        // Parse mapped address
        let addr: std::net::SocketAddr = mapped_address
            .parse()
            .context("Invalid mapped_address format")?;

        // Build STUN binding response
        let packet = Self::build_binding_response(
            &transaction_id_bytes,
            addr,
            xor_mapped_address,
            software,
            message_integrity,
        )?;

        Ok(ActionResult::Output(packet))
    }

    /// Execute STUN error response action
    fn execute_send_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(400) as u16;

        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Bad Request");

        let transaction_id = action
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .context("Missing 'transaction_id' field")?;

        // Parse transaction ID from hex
        let transaction_id_bytes =
            hex::decode(transaction_id).context("Invalid transaction_id hex")?;

        if transaction_id_bytes.len() != 12 {
            return Err(anyhow::anyhow!("Transaction ID must be 12 bytes"));
        }

        // Build STUN error response
        let packet = Self::build_error_response(&transaction_id_bytes, error_code, reason)?;

        Ok(ActionResult::Output(packet))
    }

    /// Build STUN binding response packet
    fn build_binding_response(
        transaction_id: &[u8],
        mapped_addr: std::net::SocketAddr,
        use_xor: bool,
        software: &str,
        _with_integrity: bool,
    ) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: 0x0101 (Binding Success Response)
        packet.extend_from_slice(&0x0101u16.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie: 0x2112A442
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID (12 bytes)
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add MAPPED-ADDRESS or XOR-MAPPED-ADDRESS attribute
        if use_xor {
            Self::add_xor_mapped_address_attribute(&mut packet, mapped_addr, transaction_id)?;
        } else {
            Self::add_mapped_address_attribute(&mut packet, mapped_addr)?;
        }

        // Add SOFTWARE attribute
        Self::add_software_attribute(&mut packet, software)?;

        // Update message length (attributes length, excluding 20-byte header)
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Build STUN error response packet
    fn build_error_response(
        transaction_id: &[u8],
        error_code: u16,
        reason: &str,
    ) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: 0x0111 (Binding Error Response)
        packet.extend_from_slice(&0x0111u16.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie: 0x2112A442
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID (12 bytes)
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add ERROR-CODE attribute
        Self::add_error_code_attribute(&mut packet, error_code, reason)?;

        // Update message length
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Add MAPPED-ADDRESS attribute
    fn add_mapped_address_attribute(
        packet: &mut Vec<u8>,
        addr: std::net::SocketAddr,
    ) -> Result<()> {
        // Attribute Type: 0x0001 (MAPPED-ADDRESS)
        packet.extend_from_slice(&0x0001u16.to_be_bytes());

        // Attribute Length
        let attr_start = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes()); // Placeholder

        let value_start = packet.len();

        // Reserved byte + family
        match addr {
            std::net::SocketAddr::V4(addr_v4) => {
                packet.push(0x00); // Reserved
                packet.push(0x01); // IPv4
                packet.extend_from_slice(&addr_v4.port().to_be_bytes());
                packet.extend_from_slice(&addr_v4.ip().octets());
            }
            std::net::SocketAddr::V6(addr_v6) => {
                packet.push(0x00); // Reserved
                packet.push(0x02); // IPv6
                packet.extend_from_slice(&addr_v6.port().to_be_bytes());
                packet.extend_from_slice(&addr_v6.ip().octets());
            }
        }

        let value_length = (packet.len() - value_start) as u16;
        packet[attr_start..attr_start + 2].copy_from_slice(&value_length.to_be_bytes());

        // Add padding to align to 4-byte boundary
        Self::add_padding(packet);

        Ok(())
    }

    /// Add XOR-MAPPED-ADDRESS attribute
    fn add_xor_mapped_address_attribute(
        packet: &mut Vec<u8>,
        addr: std::net::SocketAddr,
        transaction_id: &[u8],
    ) -> Result<()> {
        // Attribute Type: 0x0020 (XOR-MAPPED-ADDRESS)
        packet.extend_from_slice(&0x0020u16.to_be_bytes());

        // Attribute Length
        let attr_start = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes()); // Placeholder

        let value_start = packet.len();

        // Magic cookie for XOR operations
        let magic_cookie = 0x2112A442u32;

        match addr {
            std::net::SocketAddr::V4(addr_v4) => {
                packet.push(0x00); // Reserved
                packet.push(0x01); // IPv4

                // XOR port with upper 16 bits of magic cookie
                let xor_port = addr_v4.port() ^ (magic_cookie >> 16) as u16;
                packet.extend_from_slice(&xor_port.to_be_bytes());

                // XOR address with magic cookie
                let ip_bytes = addr_v4.ip().octets();
                let magic_bytes = magic_cookie.to_be_bytes();
                for i in 0..4 {
                    packet.push(ip_bytes[i] ^ magic_bytes[i]);
                }
            }
            std::net::SocketAddr::V6(addr_v6) => {
                packet.push(0x00); // Reserved
                packet.push(0x02); // IPv6

                // XOR port with upper 16 bits of magic cookie
                let xor_port = addr_v6.port() ^ (magic_cookie >> 16) as u16;
                packet.extend_from_slice(&xor_port.to_be_bytes());

                // XOR address with magic cookie + transaction ID
                let ip_bytes = addr_v6.ip().octets();
                let magic_bytes = magic_cookie.to_be_bytes();

                // First 4 bytes XORed with magic cookie
                for i in 0..4 {
                    packet.push(ip_bytes[i] ^ magic_bytes[i]);
                }

                // Remaining 12 bytes XORed with transaction ID
                for i in 0..12 {
                    packet.push(ip_bytes[i + 4] ^ transaction_id[i]);
                }
            }
        }

        let value_length = (packet.len() - value_start) as u16;
        packet[attr_start..attr_start + 2].copy_from_slice(&value_length.to_be_bytes());

        // Add padding to align to 4-byte boundary
        Self::add_padding(packet);

        Ok(())
    }

    /// Add SOFTWARE attribute
    fn add_software_attribute(packet: &mut Vec<u8>, software: &str) -> Result<()> {
        // Attribute Type: 0x8022 (SOFTWARE)
        packet.extend_from_slice(&0x8022u16.to_be_bytes());

        let software_bytes = software.as_bytes();
        let length = software_bytes.len() as u16;

        // Attribute Length
        packet.extend_from_slice(&length.to_be_bytes());

        // Attribute Value
        packet.extend_from_slice(software_bytes);

        // Add padding to align to 4-byte boundary
        Self::add_padding(packet);

        Ok(())
    }

    /// Add ERROR-CODE attribute
    fn add_error_code_attribute(packet: &mut Vec<u8>, error_code: u16, reason: &str) -> Result<()> {
        // Attribute Type: 0x0009 (ERROR-CODE)
        packet.extend_from_slice(&0x0009u16.to_be_bytes());

        // Attribute Length
        let attr_start = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes()); // Placeholder

        let value_start = packet.len();

        // Reserved (2 bytes) + Class (1 byte) + Number (1 byte)
        packet.extend_from_slice(&0u16.to_be_bytes()); // Reserved
        let class = (error_code / 100) as u8;
        let number = (error_code % 100) as u8;
        packet.push(class);
        packet.push(number);

        // Reason phrase
        packet.extend_from_slice(reason.as_bytes());

        let value_length = (packet.len() - value_start) as u16;
        packet[attr_start..attr_start + 2].copy_from_slice(&value_length.to_be_bytes());

        // Add padding to align to 4-byte boundary
        Self::add_padding(packet);

        Ok(())
    }

    /// Add padding to align to 4-byte boundary
    fn add_padding(packet: &mut Vec<u8>) {
        let remainder = packet.len() % 4;
        if remainder != 0 {
            let padding = 4 - remainder;
            packet.extend_from_slice(&vec![0u8; padding]);
        }
    }
}

// Action definitions

fn send_stun_binding_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_stun_binding_response".to_string(),
        description: "Send STUN binding response with mapped address".to_string(),
        parameters: vec![
            Parameter {
                name: "mapped_address".to_string(),
                type_hint: "string".to_string(),
                description:
                    "Client's public IP:port as seen by server (e.g., \"203.0.113.45:54321\")"
                        .to_string(),
                required: true,
            },
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID from request (hex string)".to_string(),
                required: true,
            },
            Parameter {
                name: "xor_mapped_address".to_string(),
                type_hint: "boolean".to_string(),
                description:
                    "Use XOR-MAPPED-ADDRESS (true) or MAPPED-ADDRESS (false). Default: true"
                        .to_string(),
                required: false,
            },
            Parameter {
                name: "software".to_string(),
                type_hint: "string".to_string(),
                description: "Software version string. Default: \"NetGet/1.0\"".to_string(),
                required: false,
            },
            Parameter {
                name: "message_integrity".to_string(),
                type_hint: "boolean".to_string(),
                description: "Include MESSAGE-INTEGRITY attribute. Default: false".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_stun_binding_response",
            "mapped_address": "203.0.113.45:54321",
            "transaction_id": "0123456789abcdef01234567",
            "xor_mapped_address": true,
            "software": "NetGet STUN/1.0"
        }),
        log_template: None,
    }
}

fn send_stun_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_stun_error_response".to_string(),
        description: "Send STUN error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "STUN error code (e.g., 400, 401, 420, 438, 500)".to_string(),
                required: true,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Error reason phrase".to_string(),
                required: true,
            },
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID from request (hex string)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_stun_error_response",
            "error_code": 401,
            "reason": "Unauthorized",
            "transaction_id": "0123456789abcdef01234567"
        }),
        log_template: None,
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Silently ignore the STUN request (no response)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
        log_template: None,
    }
}

// Event types

pub static STUN_BINDING_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "stun_binding_request",
        "STUN binding request received from client",
        json!({
            "type": "send_stun_binding_response",
            "mapped_address": "203.0.113.45:54321",
            "transaction_id": "0123456789abcdef01234567"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Client's IP:port address (public address as seen by server)".to_string(),
            required: true,
        },
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Server's listening IP:port address".to_string(),
            required: true,
        },
        Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "STUN transaction ID (hex-encoded, 12 bytes = 24 hex chars)".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "STUN message type (e.g., BindingRequest, BindingResponse)".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Number of bytes in the STUN request".to_string(),
            required: true,
        },
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("STUN {client_ip} binding request")
            .with_debug("STUN binding request from {client_ip}:{client_port}")
            .with_trace("STUN: {json_pretty(.)}"),
    )
});

fn get_stun_event_types() -> Vec<EventType> {
    vec![STUN_BINDING_REQUEST_EVENT.clone()]
}
