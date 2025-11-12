use crate::llm::actions::{ActionDefinition, ActionParameter};
use crate::protocol::EventType;
use crate::server::ProtocolActions;
use crate::state::app_state::AppState;
use serde_json::json;
use std::sync::LazyLock;

/// Event: POP3 command received from client
pub static POP3_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "pop3_command",
        "POP3 command received from client (USER, PASS, STAT, LIST, RETR, DELE, QUIT, etc.)",
    )
    .with_parameters(vec![
        ("command", "The POP3 command (e.g., 'USER alice', 'STAT')"),
        ("connection_id", "Unique connection identifier"),
    ])
});

pub struct Pop3Protocol;

impl Pop3Protocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for Pop3Protocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "close_pop3_connection".to_string(),
                description: "Close a POP3 connection".to_string(),
                parameters: vec![ActionParameter {
                    name: "connection_id".to_string(),
                    description: "Connection ID to close".to_string(),
                    example: json!("conn-123"),
                }],
                example: json!({
                    "type": "close_pop3_connection",
                    "connection_id": "conn-123"
                }),
            },
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_pop3_ok".to_string(),
                description: "Send POP3 +OK response".to_string(),
                parameters: vec![ActionParameter {
                    name: "message".to_string(),
                    description: "Optional message after +OK".to_string(),
                    example: json!("1 octets"),
                }],
                example: json!({
                    "type": "send_pop3_ok",
                    "message": "1 octets"
                }),
            },
            ActionDefinition {
                name: "send_pop3_err".to_string(),
                description: "Send POP3 -ERR response".to_string(),
                parameters: vec![ActionParameter {
                    name: "message".to_string(),
                    description: "Error message".to_string(),
                    example: json!("Invalid credentials"),
                }],
                example: json!({
                    "type": "send_pop3_err",
                    "message": "Invalid credentials"
                }),
            },
            ActionDefinition {
                name: "send_pop3_greeting".to_string(),
                description: "Send POP3 greeting banner (sent automatically on connect)".to_string(),
                parameters: vec![ActionParameter {
                    name: "message".to_string(),
                    description: "Greeting message (e.g., server name)".to_string(),
                    example: json!("POP3 server ready"),
                }],
                example: json!({
                    "type": "send_pop3_greeting",
                    "message": "POP3 server ready"
                }),
            },
            ActionDefinition {
                name: "send_pop3_stat".to_string(),
                description: "Send POP3 STAT response with message count and total size".to_string(),
                parameters: vec![
                    ActionParameter {
                        name: "message_count".to_string(),
                        description: "Number of messages in mailbox".to_string(),
                        example: json!(3),
                    },
                    ActionParameter {
                        name: "total_size".to_string(),
                        description: "Total size of all messages in octets".to_string(),
                        example: json!(1024),
                    },
                ],
                example: json!({
                    "type": "send_pop3_stat",
                    "message_count": 3,
                    "total_size": 1024
                }),
            },
            ActionDefinition {
                name: "send_pop3_list".to_string(),
                description: "Send POP3 LIST response with message sizes".to_string(),
                parameters: vec![ActionParameter {
                    name: "messages".to_string(),
                    description: "Array of message objects with 'id' and 'size' fields, or null for single message".to_string(),
                    example: json!([{"id": 1, "size": 512}, {"id": 2, "size": 256}]),
                }],
                example: json!({
                    "type": "send_pop3_list",
                    "messages": [{"id": 1, "size": 512}, {"id": 2, "size": 256}]
                }),
            },
            ActionDefinition {
                name: "send_pop3_uidl".to_string(),
                description: "Send POP3 UIDL response with unique message identifiers".to_string(),
                parameters: vec![ActionParameter {
                    name: "messages".to_string(),
                    description: "Array of message objects with 'id' and 'uidl' fields".to_string(),
                    example: json!([{"id": 1, "uidl": "msg-abc123"}, {"id": 2, "uidl": "msg-def456"}]),
                }],
                example: json!({
                    "type": "send_pop3_uidl",
                    "messages": [{"id": 1, "uidl": "msg-abc123"}, {"id": 2, "uidl": "msg-def456"}]
                }),
            },
            ActionDefinition {
                name: "send_pop3_retr".to_string(),
                description: "Send POP3 RETR response with email message content".to_string(),
                parameters: vec![
                    ActionParameter {
                        name: "size".to_string(),
                        description: "Size of message in octets".to_string(),
                        example: json!(512),
                    },
                    ActionParameter {
                        name: "content".to_string(),
                        description: "Email message content (headers + body)".to_string(),
                        example: json!("From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nHello"),
                    },
                ],
                example: json!({
                    "type": "send_pop3_retr",
                    "size": 512,
                    "content": "From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nHello"
                }),
            },
            ActionDefinition {
                name: "send_pop3_top".to_string(),
                description: "Send POP3 TOP response with email headers and limited body lines".to_string(),
                parameters: vec![
                    ActionParameter {
                        name: "content".to_string(),
                        description: "Email headers and requested body lines".to_string(),
                        example: json!("From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nFirst line"),
                    },
                ],
                example: json!({
                    "type": "send_pop3_top",
                    "content": "From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nFirst line"
                }),
            },
            ActionDefinition {
                name: "send_pop3_message".to_string(),
                description: "Send custom POP3 response".to_string(),
                parameters: vec![ActionParameter {
                    name: "message".to_string(),
                    description: "Full POP3 response line (including +OK or -ERR)".to_string(),
                    example: json!("+OK Custom response"),
                }],
                example: json!({
                    "type": "send_pop3_message",
                    "message": "+OK Custom response"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Do not send any response, wait for more commands from client".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
            ActionDefinition {
                name: "close_connection".to_string(),
                description: "Close the POP3 connection".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "close_connection"
                }),
            },
        ]
    }

    fn get_event_types(&self) -> Vec<&'static LazyLock<EventType>> {
        vec![&POP3_COMMAND_EVENT]
    }

    fn protocol_name(&self) -> &'static str {
        "pop3"
    }

    fn stack_name(&self) -> &'static str {
        "Application"
    }
}
