//! FTP protocol actions implementation
//!
//! Implements RFC 959 FTP command responses with LLM control.

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
use tracing::debug;

/// FTP protocol action handler
pub struct FtpProtocol;

impl FtpProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_ftp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action.get("code").and_then(|v| v.as_u64()).unwrap_or(500);

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Error");

        let response = format!("{} {}\r\n", code, message);

        debug!("FTP sending response: {} {}", code, message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_ftp_multiline(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action.get("code").and_then(|v| v.as_u64()).unwrap_or(200);

        let lines = action
            .get("lines")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        if lines.is_empty() {
            let message = action
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("OK");
            let response = format!("{} {}\r\n", code, message);
            return Ok(ActionResult::Output(response.as_bytes().to_vec()));
        }

        // Build multiline response
        let mut response = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i == lines.len() - 1 {
                // Last line uses space separator
                response.push_str(&format!("{} {}\r\n", code, line));
            } else {
                // Intermediate lines use dash separator
                response.push_str(&format!("{}-{}\r\n", code, line));
            }
        }

        debug!("FTP sending multiline response: code {}", code);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_ftp_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Ensure data ends with CRLF
        let formatted = if data.ends_with("\r\n") {
            data.to_string()
        } else if data.ends_with('\n') {
            format!("{}\r", data.trim_end_matches('\n'))
        } else {
            format!("{}\r\n", data)
        };

        debug!("FTP sending data: {} bytes", formatted.len());
        Ok(ActionResult::Output(formatted.as_bytes().to_vec()))
    }

    fn execute_send_ftp_list(&self, action: serde_json::Value) -> Result<ActionResult> {
        let entries = action
            .get("entries")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        let entry_count = entries.len();
        let mut response = String::new();
        for entry in entries {
            response.push_str(entry);
            response.push_str("\r\n");
        }

        debug!("FTP sending LIST data: {} entries", entry_count);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }
}

impl Default for FtpProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for FtpProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        use crate::llm::actions::ParameterDefinition;
        vec![ParameterDefinition {
            name: "passive_port_range".to_string(),
            type_hint: "string".to_string(),
            description: "Port range for passive mode data connections (default: 20000-20099)"
                .to_string(),
            required: false,
            example: json!("20000-20099"),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // FTP doesn't need async actions for now
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_ftp_response_action(),
            send_ftp_multiline_action(),
            send_ftp_data_action(),
            send_ftp_list_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "FTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_ftp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>FTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ftp", "file transfer", "ftp server"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual line-based parsing with tokio")
            .llm_control("All FTP commands + responses")
            .e2e_testing("curl, lftp, or raw TCP client")
            .notes("Basic FTP functionality, control channel only (no data channel)")
            .build()
    }

    fn description(&self) -> &'static str {
        "FTP file transfer server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an FTP server on port 21 that allows anonymous login and lists files"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles all FTP responses intelligently
            json!({
                "type": "open_server",
                "port": 21,
                "base_stack": "ftp",
                "send_first": true,
                "instruction": "FTP server that allows anonymous login and responds to FTP commands"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 21,
                "base_stack": "ftp",
                "send_first": true,
                "event_handlers": [{
                    "event_pattern": "ftp_command",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<ftp_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 21,
                "base_stack": "ftp",
                "send_first": true,
                "event_handlers": [{
                    "event_pattern": "ftp_command",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_ftp_response",
                            "code": 500,
                            "message": "Command not recognized"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for FtpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ftp::FtpServer;
            #[allow(deprecated)]
            let listen_addr = ctx.socket_addr().unwrap_or(ctx.legacy_listen_addr());
            FtpServer::spawn_with_llm_actions(
                listen_addr,
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
            "send_ftp_response" => self.execute_send_ftp_response(action),
            "send_ftp_multiline" => self.execute_send_ftp_multiline(action),
            "send_ftp_data" => self.execute_send_ftp_data(action),
            "send_ftp_list" => self.execute_send_ftp_list(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown FTP action: {}", action_type)),
        }
    }
}

// Action definitions

fn send_ftp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ftp_response".to_string(),
        description: "Send an FTP response with code and message".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "FTP response code (e.g., 220, 230, 250, 550)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Response message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_ftp_response",
            "code": 220,
            "message": "FTP Server Ready"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> FTP {code} {message}")
                .with_debug("FTP send_ftp_response: {code} {message}"),
        ),
    }
}

fn send_ftp_multiline_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ftp_multiline".to_string(),
        description: "Send a multiline FTP response (for FEAT, HELP, etc.)".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "FTP response code".to_string(),
                required: true,
            },
            Parameter {
                name: "lines".to_string(),
                type_hint: "array".to_string(),
                description: "Array of response lines".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_ftp_multiline",
            "code": 211,
            "lines": ["Features:", "UTF8", "End"]
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> FTP {code} multiline")
                .with_debug("FTP send_ftp_multiline: code={code}, lines={lines_len}"),
        ),
    }
}

fn send_ftp_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ftp_data".to_string(),
        description: "Send raw data (for inline data transfer)".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send (will auto-add CRLF if not present)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_ftp_data",
            "data": "Hello from FTP"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> FTP data {output_bytes}B")
                .with_debug("FTP send_ftp_data: {output_bytes}B")
                .with_trace("FTP data: {preview(data,200)}"),
        ),
    }
}

fn send_ftp_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ftp_list".to_string(),
        description: "Send directory listing entries".to_string(),
        parameters: vec![Parameter {
            name: "entries".to_string(),
            type_hint: "array".to_string(),
            description: "Array of directory entries in Unix ls -l format".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_ftp_list",
            "entries": [
                "-rw-r--r-- 1 ftp ftp 1024 Jan 01 00:00 file.txt",
                "drwxr-xr-x 2 ftp ftp 4096 Jan 01 00:00 subdir"
            ]
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> FTP LIST {entries_len} entries")
                .with_debug("FTP send_ftp_list: {entries_len} entries"),
        ),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_debug("FTP waiting for more data"),
        ),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the FTP connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("FTP connection closed")
                .with_debug("FTP close_connection"),
        ),
    }
}

// ============================================================================
// FTP Action Constants
// ============================================================================

pub static SEND_FTP_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(send_ftp_response_action);
pub static SEND_FTP_MULTILINE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(send_ftp_multiline_action);
pub static SEND_FTP_DATA_ACTION: LazyLock<ActionDefinition> = LazyLock::new(send_ftp_data_action);
pub static SEND_FTP_LIST_ACTION: LazyLock<ActionDefinition> = LazyLock::new(send_ftp_list_action);
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(wait_for_more_action);
pub static CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(close_connection_action);

// ============================================================================
// FTP Event Type Constants
// ============================================================================

/// FTP command event - triggered when client sends an FTP command
pub static FTP_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ftp_command",
        "FTP command received from client",
        json!({"type": "send_ftp_response", "code": 220, "message": "FTP Server Ready"}),
    )
    .with_parameters(vec![Parameter {
        name: "command".to_string(),
        type_hint: "string".to_string(),
        description: "The FTP command received (e.g., 'USER anonymous', 'LIST', 'RETR file.txt')"
            .to_string(),
        required: true,
    }])
    .with_actions(vec![
        SEND_FTP_RESPONSE_ACTION.clone(),
        SEND_FTP_MULTILINE_ACTION.clone(),
        SEND_FTP_DATA_ACTION.clone(),
        SEND_FTP_LIST_ACTION.clone(),
        WAIT_FOR_MORE_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("FTP {client_ip}: {command}")
            .with_debug("FTP command from {client_ip}:{client_port}: {command}")
            .with_trace("FTP: {json_pretty(.)}"),
    )
});

/// Get FTP event types
pub fn get_ftp_event_types() -> Vec<EventType> {
    vec![FTP_COMMAND_EVENT.clone()]
}
