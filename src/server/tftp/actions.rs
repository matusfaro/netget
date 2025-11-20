//! TFTP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct TftpProtocol;

impl TftpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Event type constants
pub static TFTP_READ_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_read_request", "Client requests to read a file")
        .with_parameters(vec![
            Parameter {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "Name of file being requested".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "string".to_string(),
                description: "Transfer mode (netascii, octet, mail)".to_string(),
                required: true,
            },
            Parameter {
                name: "client_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Client socket address".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_WRITE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_write_request", "Client requests to write a file")
        .with_parameters(vec![
            Parameter {
                name: "filename".to_string(),
                type_hint: "string".to_string(),
                description: "Name of file to write".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "string".to_string(),
                description: "Transfer mode (netascii, octet, mail)".to_string(),
                required: true,
            },
            Parameter {
                name: "client_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Client socket address".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_DATA_BLOCK_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_data_block", "Received data block from client (write operation)")
        .with_parameters(vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number (1-65535)".to_string(),
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
                description: "Number of bytes in this block".to_string(),
                required: true,
            },
            Parameter {
                name: "is_final".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if this is the final block (< 512 bytes)".to_string(),
                required: true,
            },
        ])
});

pub static TFTP_ACK_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tftp_ack_received", "Client acknowledged data block (read operation)")
        .with_parameters(vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number acknowledged".to_string(),
                required: true,
            },
        ])
});

fn get_tftp_event_types() -> Vec<EventType> {
    vec![
        TFTP_READ_REQUEST_EVENT.clone(),
        TFTP_WRITE_REQUEST_EVENT.clone(),
        TFTP_DATA_BLOCK_EVENT.clone(),
        TFTP_ACK_RECEIVED_EVENT.clone(),
    ]
}

// Implement Protocol trait (common functionality)
impl Protocol for TftpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_tftp_data_action(),
            send_tftp_ack_action(),
            send_tftp_error_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "TFTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_tftp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>TFTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["tftp", "file", "transfer", "pxe", "boot"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(69))
            .implementation("Custom TFTP packet parsing and state machine")
            .llm_control("File read/write operations via memory (no disk storage)")
            .e2e_testing("Mock-based tests with RRQ/WRQ flows")
            .notes("LLM provides all file content from memory/instruction")
            .build()
    }

    fn description(&self) -> &'static str {
        "Trivial File Transfer Protocol server for network booting and simple file transfers"
    }

    fn example_prompt(&self) -> &'static str {
        "listen on port 69 via tftp. Serve pxelinux.0 boot file with hex data 4d5a9000..."
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for TftpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::tftp::TftpServer;
            TftpServer::spawn_with_llm_actions(
                ctx.listen_addr,
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
            "send_tftp_data" => self.execute_send_tftp_data(action),
            "send_tftp_ack" => self.execute_send_tftp_ack(action),
            "send_tftp_error" => self.execute_send_tftp_error(action),
            _ => Err(anyhow::anyhow!("Unknown TFTP action: {}", action_type)),
        }
    }
}

impl TftpProtocol {
    fn execute_send_tftp_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let block_number = action
            .get("block_number")
            .and_then(|v| v.as_u64())
            .context("Missing 'block_number' in send_tftp_data action")? as u16;

        let data_hex = action
            .get("data_hex")
            .and_then(|v| v.as_str())
            .context("Missing 'data_hex' in send_tftp_data action")?;

        let data = hex::decode(data_hex)
            .context("Failed to decode hex data in send_tftp_data action")?;

        if data.len() > 512 {
            return Err(anyhow::anyhow!(
                "TFTP data block cannot exceed 512 bytes, got {}",
                data.len()
            ));
        }

        // Build TFTP DATA packet
        // Opcode (2 bytes) = 3 (DATA)
        // Block number (2 bytes)
        // Data (0-512 bytes)
        let mut packet = Vec::with_capacity(4 + data.len());
        packet.extend_from_slice(&3u16.to_be_bytes()); // Opcode DATA
        packet.extend_from_slice(&block_number.to_be_bytes());
        packet.extend_from_slice(&data);

        Ok(ActionResult::Output(packet))
    }

    fn execute_send_tftp_ack(&self, action: serde_json::Value) -> Result<ActionResult> {
        let block_number = action
            .get("block_number")
            .and_then(|v| v.as_u64())
            .context("Missing 'block_number' in send_tftp_ack action")? as u16;

        // Build TFTP ACK packet
        // Opcode (2 bytes) = 4 (ACK)
        // Block number (2 bytes)
        let mut packet = Vec::with_capacity(4);
        packet.extend_from_slice(&4u16.to_be_bytes()); // Opcode ACK
        packet.extend_from_slice(&block_number.to_be_bytes());

        Ok(ActionResult::Output(packet))
    }

    fn execute_send_tftp_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;

        let error_message = action
            .get("error_message")
            .and_then(|v| v.as_str())
            .unwrap_or("Error");

        // Build TFTP ERROR packet
        // Opcode (2 bytes) = 5 (ERROR)
        // Error code (2 bytes)
        // Error message (string)
        // Null terminator (1 byte)
        let mut packet = Vec::with_capacity(4 + error_message.len() + 1);
        packet.extend_from_slice(&5u16.to_be_bytes()); // Opcode ERROR
        packet.extend_from_slice(&error_code.to_be_bytes());
        packet.extend_from_slice(error_message.as_bytes());
        packet.push(0); // Null terminator

        Ok(ActionResult::Output(packet))
    }
}

// Action definitions

fn send_tftp_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_tftp_data".to_string(),
        description: "Send a data block to the client".to_string(),
        parameters: vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number (1-65535)".to_string(),
                required: true,
            },
            Parameter {
                name: "data_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Data as hex string (max 512 bytes)".to_string(),
                required: true,
            },
            Parameter {
                name: "is_final".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if this is the final block (< 512 bytes, optional)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_tftp_data",
            "block_number": 1,
            "data_hex": "48656c6c6f20544654502100",
            "is_final": true
        }),
    }
}

fn send_tftp_ack_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_tftp_ack".to_string(),
        description: "Acknowledge received data block".to_string(),
        parameters: vec![
            Parameter {
                name: "block_number".to_string(),
                type_hint: "number".to_string(),
                description: "Block number to acknowledge".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_tftp_ack",
            "block_number": 5
        }),
    }
}

fn send_tftp_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_tftp_error".to_string(),
        description: "Send error and terminate transfer".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "Error code (0=not defined, 1=file not found, 2=access violation, 3=disk full, 4=illegal operation, 5=unknown TID, 6=file exists, 7=no such user)".to_string(),
                required: true,
            },
            Parameter {
                name: "error_message".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_tftp_error",
            "error_code": 1,
            "error_message": "File not found"
        }),
    }
}
