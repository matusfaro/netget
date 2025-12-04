//! TFTP client protocol actions

use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::actions::{ActionDefinition, Parameter};
use crate::protocol::{ConnectContext, EventType};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

pub struct TftpClientProtocol;

impl TftpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Event type constants
pub static TFTP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_connected", "TFTP client connected to server", json!({"type": "placeholder", "event_id": "tftp_connected"}))
        .with_parameters(vec![
            Parameter {
                name: "server_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Server socket address".to_string(),
                required: true,
            },
            Parameter {
                name: "operation".to_string(),
                type_hint: "string".to_string(),
                description: "Operation type (read or write)".to_string(),
                required: true,
            },
            Parameter {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "File being transferred".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_data_received", "Received data block from server", json!({"type": "placeholder", "event_id": "tftp_data_received"}))
        .with_parameters(vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number received".to_string(),
                required: true,
            },
            Parameter {
                name: "data_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Block data as hex string".to_string(),
                required: true,
            },
            Parameter {
                name: "data_length".to_string(),
                type_hint: "number".to_string(),
                description: "Number of bytes in block".to_string(),
                required: true,
            },
            Parameter {
                name: "is_final".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if final block (< 512 bytes)".to_string(),
                required: true,
            },
            Parameter {
                name: "total_bytes".to_string(),
                type_hint: "number".to_string(),
                description: "Total bytes received so far".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_CLIENT_ACK_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_ack_received", "Server acknowledged data block", json!({"type": "placeholder", "event_id": "tftp_ack_received"}))
        .with_parameters(vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number acknowledged".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_CLIENT_TRANSFER_COMPLETE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_transfer_complete", "File transfer completed", json!({"type": "placeholder", "event_id": "tftp_transfer_complete"}))
        .with_parameters(vec![
            Parameter {
                name: "total_bytes".to_string(),
                type_hint: "number".to_string(),
                description: "Total bytes transferred".to_string(),
                required: true,
            },
            Parameter {
                name: "total_blocks".to_string(),
                type_hint: "number".to_string(),
                description: "Total blocks transferred".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_CLIENT_ERROR_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_error", "TFTP error received from server", json!({"type": "placeholder", "event_id": "tftp_error"}))
        .with_parameters(vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "TFTP error code".to_string(),
                required: true,
            },
            Parameter {
                name: "error_message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message from server".to_string(),
                required: true,
            },
        ])
});

// Implement Protocol trait (common functionality)
impl crate::llm::actions::protocol_trait::Protocol for TftpClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![tftp_read_file_action(), tftp_write_file_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_ack_action(),
            send_data_block_action(),
            disconnect_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "TFTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            TFTP_CLIENT_CONNECTED_EVENT.clone(),
            TFTP_CLIENT_DATA_RECEIVED_EVENT.clone(),
            TFTP_CLIENT_ACK_RECEIVED_EVENT.clone(),
            TFTP_CLIENT_TRANSFER_COMPLETE_EVENT.clone(),
            TFTP_CLIENT_ERROR_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>TFTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["tftp", "file", "transfer"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Custom TFTP packet parsing over UDP")
            .llm_control("File read/write operations")
            .e2e_testing("Mock-based tests")
            .notes("Simple file transfer client for network booting")
            .build()
    }

    fn description(&self) -> &'static str {
        "TFTP client for file transfers"
    }

    fn example_prompt(&self) -> &'static str {
        "connect to 192.168.1.1:69 via tftp. Read file pxelinux.0"
    }

    fn group_name(&self) -> &'static str {
        "Clients"
    }

    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "File to transfer (extracted from instruction if not provided)".to_string(),
                required: false,
                example: serde_json::Value::Null,
            },
            crate::llm::actions::ParameterDefinition {
                name: "mode".to_string(),
                type_hint: "string".to_string(),
                description: "Transfer mode: octet (binary) or netascii (text), default: octet".to_string(),
                required: false,
                example: serde_json::json!("octet"),
            },
        ]
    }
}

// Implement Client trait (client-specific functionality)
impl Client for TftpClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::tftp::TftpClient::connect_with_llm_actions(
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
            "send_ack" => {
                let block_number = action
                    .get("block_number")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'block_number' in send_ack")?
                    as u16;

                // Build ACK packet: opcode(2) + block_number(2)
                let mut packet = Vec::with_capacity(4);
                packet.extend_from_slice(&4u16.to_be_bytes()); // Opcode ACK
                packet.extend_from_slice(&block_number.to_be_bytes());

                Ok(ClientActionResult::SendData(packet))
            }
            "send_data_block" => {
                let block_number = action
                    .get("block_number")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'block_number' in send_data_block")?
                    as u16;

                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data_hex' in send_data_block")?;

                let data =
                    hex::decode(data_hex).context("Failed to decode hex in send_data_block")?;

                if data.len() > 512 {
                    return Err(anyhow::anyhow!(
                        "TFTP data block cannot exceed 512 bytes, got {}",
                        data.len()
                    ));
                }

                // Build DATA packet: opcode(2) + block_number(2) + data
                let mut packet = Vec::with_capacity(4 + data.len());
                packet.extend_from_slice(&3u16.to_be_bytes()); // Opcode DATA
                packet.extend_from_slice(&block_number.to_be_bytes());
                packet.extend_from_slice(&data);

                Ok(ClientActionResult::SendData(packet))
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown TFTP client action: {}", action_type)),
        }
    }
}

// Action definitions

fn tftp_read_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "tftp_read_file".to_string(),
        description: "Request file from TFTP server (async)".to_string(),
        parameters: vec![
            Parameter {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "Name of file to read".to_string(),
                required: true,
            },
            Parameter::new(
                "mode",
                "string",
                "Transfer mode: octet or netascii (default: octet)",
            ),
        ],
        example: json!({
            "type": "tftp_read_file",
            "filename": "pxelinux.0",
            "mode": "octet"
        }),
        log_template: None,
    }
}

fn tftp_write_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "tftp_write_file".to_string(),
        description: "Send file to TFTP server (async)".to_string(),
        parameters: vec![
            Parameter {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "Name of file to write".to_string(),
                required: true,
            },
            Parameter {
                name: "data_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Complete file content as hex string".to_string(),
                required: true,
            },
            Parameter::new(
                "mode",
                "string",
                "Transfer mode: octet or netascii (default: octet)",
            ),
        ],
        example: json!({
            "type": "tftp_write_file",
            "filename": "config.txt",
            "data_hex": "48656c6c6f20574f524c4421",
            "mode": "octet"
        }),
        log_template: None,
    }
}

fn send_ack_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ack".to_string(),
        description: "Acknowledge received data block (sync)".to_string(),
        parameters: vec![Parameter::new(
            "block_number",
            "number",
            "Block number to acknowledge",
        )],
        example: json!({
            "type": "send_ack",
            "block_number": 5
        }),
        log_template: None,
    }
}

fn send_data_block_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_data_block".to_string(),
        description: "Send data block to server (sync)".to_string(),
        parameters: vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number".to_string(),
                required: true,
            },
            Parameter {
                name: "data_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Block data as hex (max 512 bytes)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_data_block",
            "block_number": 1,
            "data_hex": "48656c6c6f"
        }),
        log_template: None,
    }
}

fn disconnect_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect".to_string(),
        description: "Disconnect from TFTP server (sync)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "disconnect"
        }),
        log_template: None,
    }
}
