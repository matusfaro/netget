//! TURN protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct TurnProtocol;

impl TurnProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TurnProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // Deliberately empty. allocate_relay_address and revoke_allocation were
        // advertised here but execute_action had no arm for either, so calling
        // one only ever produced "Unknown TURN action".
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_turn_allocate_response_action(),
            send_turn_refresh_response_action(),
            send_turn_create_permission_response_action(),
            send_turn_error_response_action(),
            ignore_request_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "TURN"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_turn_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>TURN"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["turn"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .implementation("Manual TURN protocol (RFC 8656), allocation bookkeeping only")
            .llm_control("Allocate/Refresh/CreatePermission responses and error codes")
            .e2e_testing("turnutils_uclient / WebRTC")
            .notes("NOT a working relay: no relay socket is ever bound, so Send/Data indications are ignored and no client traffic is ever forwarded. Allocate/Refresh/CreatePermission responses are well-formed, which is enough for a honeypot or protocol probe but not for NAT traversal. Also no authentication (REALM/NONCE/MESSAGE-INTEGRITY), no channel binding, IPv4 relay addresses only")
            .build()
    }
    fn description(&self) -> &'static str {
        "TURN relay server for NAT traversal"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a TURN relay server on port 3478 with 10 minute allocations"
    }
    fn group_name(&self) -> &'static str {
        "Proxy & Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "turn",
                "instruction": "TURN relay server for NAT traversal. Allocate relay addresses on request. Grant 600 second lifetimes for allocations."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "turn",
                "event_handlers": [{
                    "event_pattern": "turn_allocate_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }, {
                    "event_pattern": "turn_refresh_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 3478,
                "base_stack": "turn",
                "event_handlers": [{
                    "event_pattern": "turn_allocate_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_turn_allocate_response",
                            "relay_address": "203.0.113.100:55000",
                            "transaction_id": "{{event.transaction_id}}",
                            "lifetime_seconds": 600
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for TurnProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::turn::TurnServer;
            TurnServer::spawn_with_llm_actions(
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
            "send_turn_allocate_response" => self.execute_send_allocate_response(action),
            "send_turn_refresh_response" => self.execute_send_refresh_response(action),
            "send_turn_create_permission_response" => self.execute_send_permission_response(action),
            "send_turn_error_response" => self.execute_send_error_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown TURN action: {}", action_type)),
        }
    }
}

impl TurnProtocol {
    /// Execute TURN allocate response
    fn execute_send_allocate_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let relay_address = action
            .get("relay_address")
            .and_then(|v| v.as_str())
            .context("Missing 'relay_address' field")?;

        let transaction_id = action
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .context("Missing 'transaction_id' field")?;

        let lifetime_seconds = action
            .get("lifetime_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(600) as u32;

        // Note: allocation_id is extracted by the TURN server's main loop
        // from the raw action JSON for allocation tracking

        // Parse transaction ID from hex
        let transaction_id_bytes =
            hex::decode(transaction_id).context("Invalid transaction_id hex")?;

        if transaction_id_bytes.len() != 12 {
            return Err(anyhow::anyhow!("Transaction ID must be 12 bytes"));
        }

        // Parse relay address
        let relay_addr: std::net::SocketAddr = relay_address
            .parse()
            .context("Invalid relay_address format")?;

        // Build TURN allocate response
        let packet =
            Self::build_allocate_response(&transaction_id_bytes, relay_addr, lifetime_seconds)?;

        // Note: Allocation metadata tracking is handled in the TURN server's main loop
        // The server maintains an allocations HashMap with all necessary state

        Ok(ActionResult::Output(packet))
    }

    /// Execute TURN refresh response
    fn execute_send_refresh_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let transaction_id = action
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .context("Missing 'transaction_id' field")?;

        let lifetime_seconds = action
            .get("lifetime_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(600) as u32;

        // Parse transaction ID from hex
        let transaction_id_bytes =
            hex::decode(transaction_id).context("Invalid transaction_id hex")?;

        if transaction_id_bytes.len() != 12 {
            return Err(anyhow::anyhow!("Transaction ID must be 12 bytes"));
        }

        // Build TURN refresh response
        let packet = Self::build_refresh_response(&transaction_id_bytes, lifetime_seconds)?;

        Ok(ActionResult::Output(packet))
    }

    /// Execute TURN create permission response
    fn execute_send_permission_response(&self, action: serde_json::Value) -> Result<ActionResult> {
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

        // Build TURN create permission response
        let packet = Self::build_permission_response(&transaction_id_bytes)?;

        Ok(ActionResult::Output(packet))
    }

    /// Execute TURN error response
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

        // The error response must carry the same method as the request it
        // answers; hardcoding Allocate meant a Refresh or CreatePermission
        // failure came back as an Allocate error, which clients ignore.
        let method_name = action
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("allocate");
        let method = match method_name.to_ascii_lowercase().as_str() {
            "allocate" => 3,
            "refresh" => 4,
            "create_permission" | "createpermission" => 8,
            other => {
                return Err(anyhow::anyhow!(
                    "Unknown 'method' value {other:?}. Valid values are \"allocate\", \
                     \"refresh\" and \"create_permission\"."
                ))
            }
        };

        let packet = Self::build_error_response(&transaction_id_bytes, method, error_code, reason)?;

        Ok(ActionResult::Output(packet))
    }

    /// Build TURN allocate response packet
    fn build_allocate_response(
        transaction_id: &[u8],
        relay_addr: std::net::SocketAddr,
        lifetime_seconds: u32,
    ) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: 0x0103 (Allocate Success Response)
        packet.extend_from_slice(&0x0103u16.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie: 0x2112A442
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID (12 bytes)
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add XOR-RELAYED-ADDRESS attribute (0x0016)
        Self::add_xor_address_attribute(&mut packet, 0x0016, relay_addr, transaction_id)?;

        // Add LIFETIME attribute (0x000D)
        Self::add_lifetime_attribute(&mut packet, lifetime_seconds)?;

        // Add SOFTWARE attribute
        Self::add_software_attribute(&mut packet, "NetGet TURN/1.0")?;

        // Update message length
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Build TURN refresh response packet
    fn build_refresh_response(transaction_id: &[u8], lifetime_seconds: u32) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: 0x0104 (Refresh Success Response)
        packet.extend_from_slice(&0x0104u16.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add LIFETIME attribute
        Self::add_lifetime_attribute(&mut packet, lifetime_seconds)?;

        // Update message length
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Build TURN create permission response packet
    fn build_permission_response(transaction_id: &[u8]) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: 0x0108 (CreatePermission Success Response)
        packet.extend_from_slice(&0x0108u16.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add SOFTWARE attribute
        Self::add_software_attribute(&mut packet, "NetGet TURN/1.0")?;

        // Update message length
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Build TURN error response packet
    fn build_error_response(
        transaction_id: &[u8],
        method: u16,
        error_code: u16,
        reason: &str,
    ) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Message Type: error response for given method
        // Class = 2 (error), method = provided
        let message_type =
            (method & 0x000F) | ((method & 0x0070) << 1) | ((method & 0x0F80) << 2) | 0x0110;
        packet.extend_from_slice(&message_type.to_be_bytes());

        // Message Length (will be updated later)
        let length_pos = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        // Magic Cookie
        packet.extend_from_slice(&0x2112A442u32.to_be_bytes());

        // Transaction ID
        packet.extend_from_slice(transaction_id);

        let attributes_start = packet.len();

        // Add ERROR-CODE attribute
        Self::add_error_code_attribute(&mut packet, error_code, reason)?;

        // Update message length
        let attributes_length = (packet.len() - attributes_start) as u16;
        packet[length_pos..length_pos + 2].copy_from_slice(&attributes_length.to_be_bytes());

        Ok(packet)
    }

    /// Add XOR-ed address attribute (XOR-RELAYED-ADDRESS, XOR-PEER-ADDRESS, etc.)
    fn add_xor_address_attribute(
        packet: &mut Vec<u8>,
        attr_type: u16,
        addr: std::net::SocketAddr,
        transaction_id: &[u8],
    ) -> Result<()> {
        packet.extend_from_slice(&attr_type.to_be_bytes());

        let attr_start = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes()); // Placeholder

        let value_start = packet.len();

        let magic_cookie = 0x2112A442u32;

        match addr {
            std::net::SocketAddr::V4(addr_v4) => {
                packet.push(0x00); // Reserved
                packet.push(0x01); // IPv4

                // XOR port
                let xor_port = addr_v4.port() ^ (magic_cookie >> 16) as u16;
                packet.extend_from_slice(&xor_port.to_be_bytes());

                // XOR address
                let ip_bytes = addr_v4.ip().octets();
                let magic_bytes = magic_cookie.to_be_bytes();
                for i in 0..4 {
                    packet.push(ip_bytes[i] ^ magic_bytes[i]);
                }
            }
            std::net::SocketAddr::V6(addr_v6) => {
                packet.push(0x00); // Reserved
                packet.push(0x02); // IPv6

                let xor_port = addr_v6.port() ^ (magic_cookie >> 16) as u16;
                packet.extend_from_slice(&xor_port.to_be_bytes());

                let ip_bytes = addr_v6.ip().octets();
                let magic_bytes = magic_cookie.to_be_bytes();

                for i in 0..4 {
                    packet.push(ip_bytes[i] ^ magic_bytes[i]);
                }
                for i in 0..12 {
                    packet.push(ip_bytes[i + 4] ^ transaction_id[i]);
                }
            }
        }

        let value_length = (packet.len() - value_start) as u16;
        packet[attr_start..attr_start + 2].copy_from_slice(&value_length.to_be_bytes());

        Self::add_padding(packet);
        Ok(())
    }

    /// Add LIFETIME attribute
    fn add_lifetime_attribute(packet: &mut Vec<u8>, lifetime_seconds: u32) -> Result<()> {
        // Attribute Type: 0x000D (LIFETIME)
        packet.extend_from_slice(&0x000Du16.to_be_bytes());

        // Attribute Length: 4 bytes
        packet.extend_from_slice(&4u16.to_be_bytes());

        // Lifetime value
        packet.extend_from_slice(&lifetime_seconds.to_be_bytes());

        Ok(())
    }

    /// Add SOFTWARE attribute
    fn add_software_attribute(packet: &mut Vec<u8>, software: &str) -> Result<()> {
        packet.extend_from_slice(&0x8022u16.to_be_bytes());

        let software_bytes = software.as_bytes();
        let length = software_bytes.len() as u16;

        packet.extend_from_slice(&length.to_be_bytes());
        packet.extend_from_slice(software_bytes);

        Self::add_padding(packet);
        Ok(())
    }

    /// Add ERROR-CODE attribute
    fn add_error_code_attribute(packet: &mut Vec<u8>, error_code: u16, reason: &str) -> Result<()> {
        packet.extend_from_slice(&0x0009u16.to_be_bytes());

        let attr_start = packet.len();
        packet.extend_from_slice(&0u16.to_be_bytes());

        let value_start = packet.len();

        packet.extend_from_slice(&0u16.to_be_bytes()); // Reserved
        let class = (error_code / 100) as u8;
        let number = (error_code % 100) as u8;
        packet.push(class);
        packet.push(number);
        packet.extend_from_slice(reason.as_bytes());

        let value_length = (packet.len() - value_start) as u16;
        packet[attr_start..attr_start + 2].copy_from_slice(&value_length.to_be_bytes());

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

fn send_turn_allocate_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_turn_allocate_response".to_string(),
        description: "Send TURN allocate response with relay address".to_string(),
        parameters: vec![
            Parameter {
                name: "relay_address".to_string(),
                type_hint: "string".to_string(),
                description: "Relay address allocated for client (e.g., \"203.0.113.100:55000\")"
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
                name: "lifetime_seconds".to_string(),
                type_hint: "number".to_string(),
                description: "Allocation lifetime in seconds. Default: 600".to_string(),
                required: false,
            },
            Parameter {
                name: "allocation_id".to_string(),
                type_hint: "string".to_string(),
                description: "Unique allocation identifier. Defaults to transaction_id".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_turn_allocate_response",
            "relay_address": "203.0.113.100:55000",
            "transaction_id": "0123456789abcdef01234567",
            "lifetime_seconds": 600,
            "allocation_id": "alloc-123"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> TURN allocate {relay_address}")
                .with_debug(
                    "TURN allocate_response: relay={relay_address}, lifetime={lifetime_seconds}s",
                ),
        ),
    }
}

fn send_turn_refresh_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_turn_refresh_response".to_string(),
        description: "Send TURN refresh response to extend allocation lifetime".to_string(),
        parameters: vec![
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID from request (hex string)".to_string(),
                required: true,
            },
            Parameter {
                name: "lifetime_seconds".to_string(),
                type_hint: "number".to_string(),
                description: "New lifetime in seconds. Default: 600".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_turn_refresh_response",
            "transaction_id": "0123456789abcdef01234567",
            "lifetime_seconds": 600
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> TURN refresh ({lifetime_seconds}s)")
                .with_debug("TURN refresh_response: lifetime={lifetime_seconds}s"),
        ),
    }
}

fn send_turn_create_permission_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_turn_create_permission_response".to_string(),
        description: "Send TURN create permission response".to_string(),
        parameters: vec![Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "Transaction ID from request (hex string)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_turn_create_permission_response",
            "transaction_id": "0123456789abcdef01234567"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> TURN permission created")
                .with_debug("TURN create_permission_response"),
        ),
    }
}

fn send_turn_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_turn_error_response".to_string(),
        description: "Send TURN error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "TURN error code (e.g., 400, 401, 403, 508)".to_string(),
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
                description: "Transaction ID from the request, hex-encoded (24 hex chars). Must match the request or the client will discard the response.".to_string(),
                required: true,
            },
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "Which request this error answers: \"allocate\" (default), \"refresh\" or \"create_permission\". Must match the request's method or the client ignores the error.".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_turn_error_response",
            "error_code": 508,
            "reason": "Insufficient Capacity",
            "transaction_id": "{{event.transaction_id}}",
            "method": "allocate"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> TURN error {error_code}: {reason}")
                .with_debug("TURN error_response: {error_code} {reason}"),
        ),
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Silently ignore the TURN request (no response)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("TURN request ignored")
                .with_debug("TURN ignore_request"),
        ),
    }
}

// Event types

pub static TURN_ALLOCATE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_allocate_request",
        "TURN allocate request received from client",
        json!({
            "type": "send_turn_allocate_response",
            "relay_address": "203.0.113.100:55000",
            "transaction_id": "{{event.transaction_id}}",
            "lifetime_seconds": 600
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "STUN/TURN transaction ID from the request, hex-encoded (24 hex chars = 12 bytes). MUST be copied into the response: clients discard any reply whose transaction ID differs from the request they sent.".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Client's IP:port as seen by the server".to_string(),
            required: true,
        },
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Server's listening IP:port".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Decoded TURN message type, e.g. \"AllocateRequest\"".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Size of the received datagram in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "existing_allocations".to_string(),
            type_hint: "array".to_string(),
            description: "Allocations currently held by this client: allocation_id, relay_address, lifetime_seconds, expires_in_seconds, permitted_peers".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_turn_allocate_response_action(),
        send_turn_error_response_action(),
        ignore_request_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("TURN allocate request")
            .with_debug("TURN allocate request")
            .with_trace("TURN allocate: {json_pretty(.)}"),
    )
});

pub static TURN_REFRESH_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_refresh_request",
        "TURN refresh request received from client",
        json!({
            "type": "send_turn_refresh_response",
            "transaction_id": "{{event.transaction_id}}",
            "lifetime_seconds": 600
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "STUN/TURN transaction ID from the request, hex-encoded (24 hex chars = 12 bytes). MUST be copied into the response: clients discard any reply whose transaction ID differs from the request they sent.".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Client's IP:port as seen by the server".to_string(),
            required: true,
        },
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Server's listening IP:port".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Decoded TURN message type, e.g. \"AllocateRequest\"".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Size of the received datagram in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "existing_allocations".to_string(),
            type_hint: "array".to_string(),
            description: "Allocations currently held by this client: allocation_id, relay_address, lifetime_seconds, expires_in_seconds, permitted_peers".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_turn_refresh_response_action(),
        send_turn_error_response_action(),
        ignore_request_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("TURN refresh request")
            .with_debug("TURN refresh request")
            .with_trace("TURN refresh: {json_pretty(.)}"),
    )
});

pub static TURN_CREATE_PERMISSION_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_create_permission_request",
        "TURN create permission request received from client",
        json!({
            "type": "send_turn_create_permission_response",
            "transaction_id": "{{event.transaction_id}}"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "STUN/TURN transaction ID from the request, hex-encoded (24 hex chars = 12 bytes). MUST be copied into the response: clients discard any reply whose transaction ID differs from the request they sent.".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Client's IP:port as seen by the server".to_string(),
            required: true,
        },
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Server's listening IP:port".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Decoded TURN message type, e.g. \"AllocateRequest\"".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Size of the received datagram in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "existing_allocations".to_string(),
            type_hint: "array".to_string(),
            description: "Allocations currently held by this client: allocation_id, relay_address, lifetime_seconds, expires_in_seconds, permitted_peers".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_turn_create_permission_response_action(),
        send_turn_error_response_action(),
        ignore_request_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("TURN create permission")
            .with_debug("TURN create permission")
            .with_trace("TURN create permission: {json_pretty(.)}"),
    )
});

fn get_turn_event_types() -> Vec<EventType> {
    vec![
        TURN_ALLOCATE_REQUEST_EVENT.clone(),
        TURN_REFRESH_REQUEST_EVENT.clone(),
        TURN_CREATE_PERMISSION_REQUEST_EVENT.clone(),
    ]
}
