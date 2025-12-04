use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// Event: POP3 command received from client
pub static POP3_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "pop3_command",
        "POP3 command received from client (USER, PASS, STAT, LIST, RETR, DELE, QUIT, etc.)",
        json!({"type": "send_pop3_ok", "message": "command processed"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "command".to_string(),
            type_hint: "string".to_string(),
            description: "The POP3 command (e.g., 'USER alice', 'STAT')".to_string(),
            required: true,
        },
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique connection identifier".to_string(),
            required: true,
        },
    ])
});

pub struct Pop3Protocol;

impl Pop3Protocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_pop3_greeting(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("POP3 server ready");

        let response = format!("+OK {}\r\n", message);

        debug!("POP3 sending greeting: {}", response.trim());
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_ok(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let response = if message.is_empty() {
            "+OK\r\n".to_string()
        } else {
            format!("+OK {}\r\n", message)
        };

        debug!("POP3 sending +OK: {}", message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_err(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        let response = format!("-ERR {}\r\n", message);

        debug!("POP3 sending -ERR: {}", message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_stat(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_count = action
            .get("message_count")
            .and_then(|v| v.as_u64())
            .context("Missing 'message_count' parameter")?;

        let total_size = action
            .get("total_size")
            .and_then(|v| v.as_u64())
            .context("Missing 'total_size' parameter")?;

        let response = format!("+OK {} {}\r\n", message_count, total_size);

        debug!("POP3 sending STAT: {} messages, {} bytes", message_count, total_size);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_list(&self, action: serde_json::Value) -> Result<ActionResult> {
        let messages = action
            .get("messages")
            .and_then(|v| v.as_array())
            .context("Missing 'messages' parameter")?;

        let mut response = format!("+OK {} messages\r\n", messages.len());
        for msg in messages {
            let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let size = msg.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            response.push_str(&format!("{} {}\r\n", id, size));
        }
        response.push_str(".\r\n");

        debug!("POP3 sending LIST with {} messages", messages.len());
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_uidl(&self, action: serde_json::Value) -> Result<ActionResult> {
        let messages = action
            .get("messages")
            .and_then(|v| v.as_array())
            .context("Missing 'messages' parameter")?;

        let mut response = format!("+OK {} messages\r\n", messages.len());
        for msg in messages {
            let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let uidl = msg.get("uidl").and_then(|v| v.as_str()).unwrap_or("");
            response.push_str(&format!("{} {}\r\n", id, uidl));
        }
        response.push_str(".\r\n");

        debug!("POP3 sending UIDL with {} messages", messages.len());
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_retr(&self, action: serde_json::Value) -> Result<ActionResult> {
        let size = action
            .get("size")
            .and_then(|v| v.as_u64())
            .context("Missing 'size' parameter")?;

        let content = action
            .get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' parameter")?;

        let mut response = format!("+OK {} octets\r\n", size);
        response.push_str(content);
        if !content.ends_with("\r\n") {
            response.push_str("\r\n");
        }
        response.push_str(".\r\n");

        debug!("POP3 sending RETR with {} bytes", size);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_top(&self, action: serde_json::Value) -> Result<ActionResult> {
        let content = action
            .get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' parameter")?;

        let mut response = "+OK\r\n".to_string();
        response.push_str(content);
        if !content.ends_with("\r\n") {
            response.push_str("\r\n");
        }
        response.push_str(".\r\n");

        debug!("POP3 sending TOP");
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_pop3_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // Ensure message ends with \r\n
        let formatted = if message.ends_with("\r\n") {
            message.to_string()
        } else if message.ends_with('\n') {
            format!("{}\r", message.trim_end_matches('\n'))
        } else {
            format!("{}\r\n", message)
        };

        debug!("POP3 sending custom message: {}", formatted.trim());
        Ok(ActionResult::Output(formatted.as_bytes().to_vec()))
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Pop3Protocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "enable_tls".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable POP3S (implicit TLS) mode (default: false)".to_string(),
                required: false,
                example: json!(true),
            },
            ParameterDefinition {
                name: "tls_common_name".to_string(),
                type_hint: "string".to_string(),
                description: "TLS certificate Common Name (CN) (default: 'netget-pop3-server')"
                    .to_string(),
                required: false,
                example: json!("mail.example.com"),
            },
            ParameterDefinition {
                name: "tls_san_dns_names".to_string(),
                type_hint: "array".to_string(),
                description:
                    "TLS certificate Subject Alternative Names (DNS names) (default: ['localhost', '*.local'])"
                        .to_string(),
                required: false,
                example: json!(["mail.example.com", "localhost", "*.example.com"]),
            },
            ParameterDefinition {
                name: "tls_validity_days".to_string(),
                type_hint: "integer".to_string(),
                description: "TLS certificate validity period in days (default: 365)".to_string(),
                required: false,
                example: json!(365),
            },
            ParameterDefinition {
                name: "tls_organization".to_string(),
                type_hint: "string".to_string(),
                description: "TLS certificate Organization (O) (default: 'NetGet')".to_string(),
                required: false,
                example: json!("Example Corp"),
            },
            ParameterDefinition {
                name: "tls_organizational_unit".to_string(),
                type_hint: "string".to_string(),
                description:
                    "TLS certificate Organizational Unit (OU) (default: 'POP3 Server')".to_string(),
                required: false,
                example: json!("IT Department"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![ActionDefinition {
            name: "close_pop3_connection".to_string(),
            description: "Close a POP3 connection".to_string(),
            parameters: vec![Parameter {
                name: "connection_id".to_string(),
                type_hint: "string".to_string(),
                description: "Connection ID to close".to_string(),
                required: true,
            }],
            example: json!({
                "type": "close_pop3_connection",
                "connection_id": "conn-123"
            }),
        }]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_pop3_ok".to_string(),
                description: "Send POP3 +OK response".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "string".to_string(),
                    description: "Optional message after +OK".to_string(),
                    required: false,
                }],
                example: json!({
                    "type": "send_pop3_ok",
                    "message": "1 octets"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_err".to_string(),
                description: "Send POP3 -ERR response".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "string".to_string(),
                    description: "Error message".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_err",
                    "message": "Invalid credentials"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_greeting".to_string(),
                description: "Send POP3 greeting banner (sent automatically on connect)".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "string".to_string(),
                    description: "Greeting message (e.g., server name)".to_string(),
                    required: false,
                }],
                example: json!({
                    "type": "send_pop3_greeting",
                    "message": "POP3 server ready"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_stat".to_string(),
                description: "Send POP3 STAT response with message count and total size".to_string(),
                parameters: vec![
                    Parameter {
                        name: "message_count".to_string(),
                        type_hint: "number".to_string(),
                        description: "Number of messages in mailbox".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "total_size".to_string(),
                        type_hint: "number".to_string(),
                        description: "Total size of all messages in octets".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_pop3_stat",
                    "message_count": 3,
                    "total_size": 1024
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_list".to_string(),
                description: "Send POP3 LIST response with message sizes".to_string(),
                parameters: vec![Parameter {
                    name: "messages".to_string(),
                    type_hint: "array".to_string(),
                    description:
                        "Array of message objects with 'id' and 'size' fields".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_list",
                    "messages": [{"id": 1, "size": 512}, {"id": 2, "size": 256}]
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_uidl".to_string(),
                description: "Send POP3 UIDL response with unique message identifiers".to_string(),
                parameters: vec![Parameter {
                    name: "messages".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of message objects with 'id' and 'uidl' fields".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_uidl",
                    "messages": [{"id": 1, "uidl": "msg-abc123"}, {"id": 2, "uidl": "msg-def456"}]
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_retr".to_string(),
                description: "Send POP3 RETR response with email message content".to_string(),
                parameters: vec![
                    Parameter {
                        name: "size".to_string(),
                        type_hint: "number".to_string(),
                        description: "Size of message in octets".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "content".to_string(),
                        type_hint: "string".to_string(),
                        description: "Email message content (headers + body)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_pop3_retr",
                    "size": 512,
                    "content": "From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nHello"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_top".to_string(),
                description: "Send POP3 TOP response with email headers and limited body lines"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "content".to_string(),
                    type_hint: "string".to_string(),
                    description: "Email headers and requested body lines".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_top",
                    "content": "From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nFirst line"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_pop3_message".to_string(),
                description: "Send custom POP3 response".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "string".to_string(),
                    description: "Full POP3 response line (including +OK or -ERR)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_message",
                    "message": "+OK Custom response"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Do not send any response, wait for more commands from client"
                    .to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "close_connection".to_string(),
                description: "Close the POP3 connection".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "close_connection"
                }),
            log_template: None,
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "POP3"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![EventType::new("pop3_command", "Triggered when POP3 command is received from client", json!({"type": "placeholder", "event_id": "pop3_command"}))]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>POP3"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["pop3", "pop3 server", "via pop3", "post office protocol"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation(
                "Manual TCP/TLS implementation with full LLM control over protocol responses",
            )
            .llm_control(
                "Full control over POP3 responses (+OK, -ERR, STAT, LIST, RETR, etc.)",
            )
            .e2e_testing("Manual TCP client with line-based protocol testing")
            .build()
    }

    fn description(&self) -> &'static str {
        "POP3 email retrieval server (RFC 1939)"
    }

    fn example_prompt(&self) -> &'static str {
        "Listen on port 110 via POP3. Accept all authentication and return 3 test messages."
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        StartupExamples::new(
            // LLM-driven example
            json!({
                "type": "open_server",
                "port": 110,
                "base_stack": "pop3",
                "instruction": "POP3 server with 3 test messages, accept any credentials"
            }),
            // Script-based example
            json!({
                "type": "open_server",
                "port": 110,
                "base_stack": "pop3",
                "event_handlers": [{
                    "event_pattern": "pop3_command",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# Handle POP3 commands\ncmd = event.get('command', '').upper()\nif cmd.startswith('USER'):\n    respond([{'type': 'send_pop3_ok', 'message': 'user accepted'}])\nelif cmd.startswith('PASS'):\n    respond([{'type': 'send_pop3_ok', 'message': 'logged in, 3 messages'}])\nelif cmd == 'STAT':\n    respond([{'type': 'send_pop3_stat', 'message_count': 3, 'total_size': 1024}])\nelif cmd == 'LIST':\n    respond([{'type': 'send_pop3_list', 'messages': [{'id': 1, 'size': 512}, {'id': 2, 'size': 256}, {'id': 3, 'size': 256}]}])\nelif cmd == 'QUIT':\n    respond([{'type': 'send_pop3_ok', 'message': 'bye'}])\nelse:\n    respond([{'type': 'send_pop3_ok'}])"
                    }
                }]
            }),
            // Static handler example
            json!({
                "type": "open_server",
                "port": 110,
                "base_stack": "pop3",
                "event_handlers": [{
                    "event_pattern": "pop3_command",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_pop3_ok",
                            "message": "OK"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for Pop3Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::pop3::Pop3Server;

            // TLS configuration - TODO: Implement when rustls API is stable
            // For now, only plain POP3 is supported
            let tls_config = None;

            Pop3Server::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                tls_config,
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
            "send_pop3_greeting" => self.execute_send_pop3_greeting(action),
            "send_pop3_ok" => self.execute_send_pop3_ok(action),
            "send_pop3_err" => self.execute_send_pop3_err(action),
            "send_pop3_stat" => self.execute_send_pop3_stat(action),
            "send_pop3_list" => self.execute_send_pop3_list(action),
            "send_pop3_uidl" => self.execute_send_pop3_uidl(action),
            "send_pop3_retr" => self.execute_send_pop3_retr(action),
            "send_pop3_top" => self.execute_send_pop3_top(action),
            "send_pop3_message" => self.execute_send_pop3_message(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown POP3 action: {}", action_type)),
        }
    }
}
