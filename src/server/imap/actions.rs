//! IMAP protocol actions implementation
//!
//! This module implements the action system for IMAP (Internet Message Access Protocol).
//! The LLM controls all IMAP responses through these actions, including:
//! - Greeting and capability advertisement
//! - Authentication (LOGIN)
//! - Mailbox operations (SELECT, LIST, CREATE, DELETE, RENAME, STATUS, EXAMINE)
//! - Message operations (FETCH, STORE, SEARCH, COPY, EXPUNGE)
//! - UID-based operations (UID FETCH, UID STORE, UID SEARCH, UID COPY)
//! - APPEND for adding messages

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

/// IMAP protocol action handler
pub struct ImapProtocol;

impl ImapProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_imap_greeting(&self, action: serde_json::Value) -> Result<ActionResult> {
        let hostname = action
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let capabilities = action
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "IMAP4rev1".to_string());

        debug!("IMAP sending greeting: hostname={}, capabilities={}", hostname, capabilities);

        let greeting = format!("* OK [CAPABILITY {}] {} IMAP4rev1 Service Ready\r\n", capabilities, hostname);
        Ok(ActionResult::Output(greeting.into_bytes()))
    }

    fn execute_send_imap_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let tag = action
            .get("tag")
            .and_then(|v| v.as_str())
            .context("Missing 'tag' field in send_imap_response")?;

        let status = action
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("OK");

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let code = action
            .get("code")
            .and_then(|v| v.as_str());

        debug!("IMAP sending response: tag={}, status={}, message={}", tag, status, message);

        let response = if let Some(code) = code {
            format!("{} {} [{}] {}\r\n", tag, status, code, message)
        } else if !message.is_empty() {
            format!("{} {} {}\r\n", tag, status, message)
        } else {
            format!("{} {}\r\n", tag, status)
        };

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_untagged(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response_type = action
            .get("response_type")
            .and_then(|v| v.as_str())
            .context("Missing 'response_type' field in send_imap_untagged")?;

        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        debug!("IMAP sending untagged response: type={}, data={}", response_type, data);

        let response = if data.is_empty() {
            format!("* {}\r\n", response_type)
        } else {
            format!("* {} {}\r\n", response_type, data)
        };

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_capability(&self, action: serde_json::Value) -> Result<ActionResult> {
        let capabilities = action
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "IMAP4rev1".to_string());

        debug!("IMAP sending capability: {}", capabilities);

        let response = format!("* CAPABILITY {}\r\n", capabilities);
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_list(&self, action: serde_json::Value) -> Result<ActionResult> {
        let mailboxes = action
            .get("mailboxes")
            .and_then(|v| v.as_array())
            .context("Missing 'mailboxes' array in send_imap_list")?;

        debug!("IMAP sending LIST response: {} mailboxes", mailboxes.len());

        let mut response = Vec::new();

        for mailbox in mailboxes {
            let name = mailbox
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("INBOX");

            let delimiter = mailbox
                .get("delimiter")
                .and_then(|v| v.as_str())
                .unwrap_or("/");

            let flags = mailbox
                .get("flags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();

            let line = if flags.is_empty() {
                format!("* LIST () \"{}\" \"{}\"\r\n", delimiter, name)
            } else {
                format!("* LIST ({}) \"{}\" \"{}\"\r\n", flags, delimiter, name)
            };
            response.extend_from_slice(line.as_bytes());
        }

        Ok(ActionResult::Output(response))
    }

    fn execute_send_imap_status(&self, action: serde_json::Value) -> Result<ActionResult> {
        let mailbox = action
            .get("mailbox")
            .and_then(|v| v.as_str())
            .context("Missing 'mailbox' field in send_imap_status")?;

        let status_items = action
            .get("items")
            .and_then(|v| v.as_object())
            .context("Missing 'items' object in send_imap_status")?;

        debug!("IMAP sending STATUS response for mailbox: {}", mailbox);

        let mut items_str = Vec::new();
        for (key, value) in status_items {
            items_str.push(format!("{} {}", key.to_uppercase(), value));
        }

        let response = format!("* STATUS \"{}\" ({})\r\n", mailbox, items_str.join(" "));
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_fetch(&self, action: serde_json::Value) -> Result<ActionResult> {
        let sequence = action
            .get("sequence")
            .and_then(|v| v.as_u64())
            .context("Missing 'sequence' field in send_imap_fetch")?;

        let data = action
            .get("data")
            .and_then(|v| v.as_object())
            .context("Missing 'data' object in send_imap_fetch")?;

        debug!("IMAP sending FETCH response for message: {}", sequence);

        let mut items = Vec::new();

        // Build FETCH response items
        for (key, value) in data {
            match key.to_uppercase().as_str() {
                "FLAGS" => {
                    if let Some(flags_arr) = value.as_array() {
                        let flags: Vec<&str> = flags_arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect();
                        items.push(format!("FLAGS ({})", flags.join(" ")));
                    }
                }
                "UID" => {
                    if let Some(uid) = value.as_u64() {
                        items.push(format!("UID {}", uid));
                    }
                }
                "RFC822.SIZE" => {
                    if let Some(size) = value.as_u64() {
                        items.push(format!("RFC822.SIZE {}", size));
                    }
                }
                "BODY[]" | "RFC822" => {
                    if let Some(body) = value.as_str() {
                        items.push(format!("{} {{{}}}\r\n{}", key.to_uppercase(), body.len(), body));
                    }
                }
                "ENVELOPE" => {
                    if let Some(env_str) = value.as_str() {
                        items.push(format!("ENVELOPE {}", env_str));
                    }
                }
                "BODYSTRUCTURE" => {
                    if let Some(bs_str) = value.as_str() {
                        items.push(format!("BODYSTRUCTURE {}", bs_str));
                    }
                }
                "INTERNALDATE" => {
                    if let Some(date) = value.as_str() {
                        items.push(format!("INTERNALDATE \"{}\"", date));
                    }
                }
                _ => {
                    // Handle any other custom items
                    if let Some(val_str) = value.as_str() {
                        items.push(format!("{} {}", key.to_uppercase(), val_str));
                    }
                }
            }
        }

        let response = format!("* {} FETCH ({})\r\n", sequence, items.join(" "));
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_search(&self, action: serde_json::Value) -> Result<ActionResult> {
        let empty_vec = vec![];
        let results = action
            .get("results")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty_vec);

        debug!("IMAP sending SEARCH response: {} results", results.len());

        let ids: Vec<String> = results
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n.to_string()))
            .collect();

        let response = if ids.is_empty() {
            "* SEARCH\r\n".to_string()
        } else {
            format!("* SEARCH {}\r\n", ids.join(" "))
        };

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_exists(&self, action: serde_json::Value) -> Result<ActionResult> {
        let count = action
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        debug!("IMAP sending EXISTS response: {} messages", count);

        let response = format!("* {} EXISTS\r\n", count);
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_recent(&self, action: serde_json::Value) -> Result<ActionResult> {
        let count = action
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        debug!("IMAP sending RECENT response: {} messages", count);

        let response = format!("* {} RECENT\r\n", count);
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_flags(&self, action: serde_json::Value) -> Result<ActionResult> {
        let flags = action
            .get("flags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<&str>>()
                    .join(" ")
            })
            .unwrap_or_default();

        debug!("IMAP sending FLAGS response: {}", flags);

        let response = format!("* FLAGS ({})\r\n", flags);
        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_imap_expunge(&self, action: serde_json::Value) -> Result<ActionResult> {
        let sequence = action
            .get("sequence")
            .and_then(|v| v.as_u64())
            .context("Missing 'sequence' field in send_imap_expunge")?;

        debug!("IMAP sending EXPUNGE response for message: {}", sequence);

        let response = format!("* {} EXPUNGE\r\n", sequence);
        Ok(ActionResult::Output(response.into_bytes()))
    }
}

impl Server for ImapProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::imap::ImapServer;
            ImapServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // IMAP doesn't need async actions for now (all commands are synchronous request/response)
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_imap_greeting_action(),
            send_imap_response_action(),
            send_imap_untagged_action(),
            send_imap_capability_action(),
            send_imap_list_action(),
            send_imap_status_action(),
            send_imap_fetch_action(),
            send_imap_search_action(),
            send_imap_exists_action(),
            send_imap_recent_action(),
            send_imap_flags_action(),
            send_imap_expunge_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_imap_greeting" => self.execute_send_imap_greeting(action),
            "send_imap_response" => self.execute_send_imap_response(action),
            "send_imap_untagged" => self.execute_send_imap_untagged(action),
            "send_imap_capability" => self.execute_send_imap_capability(action),
            "send_imap_list" => self.execute_send_imap_list(action),
            "send_imap_status" => self.execute_send_imap_status(action),
            "send_imap_fetch" => self.execute_send_imap_fetch(action),
            "send_imap_search" => self.execute_send_imap_search(action),
            "send_imap_exists" => self.execute_send_imap_exists(action),
            "send_imap_recent" => self.execute_send_imap_recent(action),
            "send_imap_flags" => self.execute_send_imap_flags(action),
            "send_imap_expunge" => self.execute_send_imap_expunge(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown IMAP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "IMAP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_imap_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>IMAP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["imap"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Alpha
        )
    }
}

// ============================================================================
// Action Definitions
// ============================================================================

fn send_imap_greeting_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_greeting".to_string(),
        description: "Send IMAP server greeting with capabilities".to_string(),
        parameters: vec![
            Parameter {
                name: "hostname".to_string(),
                type_hint: "string".to_string(),
                description: "Server hostname (default: localhost)".to_string(),
                required: false,
            },
            Parameter {
                name: "capabilities".to_string(),
                type_hint: "array".to_string(),
                description: "Server capabilities (default: [IMAP4rev1])".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_imap_greeting",
            "hostname": "mail.example.com",
            "capabilities": ["IMAP4rev1", "IDLE", "NAMESPACE"]
        }),
    }
}

fn send_imap_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_response".to_string(),
        description: "Send tagged IMAP response (OK/NO/BAD)".to_string(),
        parameters: vec![
            Parameter {
                name: "tag".to_string(),
                type_hint: "string".to_string(),
                description: "Command tag from client request".to_string(),
                required: true,
            },
            Parameter {
                name: "status".to_string(),
                type_hint: "string".to_string(),
                description: "Response status: OK, NO, or BAD".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Response message".to_string(),
                required: false,
            },
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description: "Optional response code in brackets (e.g., READ-WRITE, READ-ONLY)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_imap_response",
            "tag": "A001",
            "status": "OK",
            "code": "READ-WRITE",
            "message": "SELECT completed"
        }),
    }
}

fn send_imap_untagged_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_untagged".to_string(),
        description: "Send untagged IMAP response (informational data)".to_string(),
        parameters: vec![
            Parameter {
                name: "response_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of untagged response (e.g., OK, BYE, NO, BAD, CAPABILITY)".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Response data".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_imap_untagged",
            "response_type": "OK",
            "data": "[PERMANENTFLAGS (\\Deleted \\Seen \\*)] Limited"
        }),
    }
}

fn send_imap_capability_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_capability".to_string(),
        description: "Send IMAP CAPABILITY response".to_string(),
        parameters: vec![
            Parameter {
                name: "capabilities".to_string(),
                type_hint: "array".to_string(),
                description: "Array of capability strings".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_imap_capability",
            "capabilities": ["IMAP4rev1", "IDLE", "NAMESPACE", "UIDPLUS"]
        }),
    }
}

fn send_imap_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_list".to_string(),
        description: "Send IMAP LIST response with mailbox list".to_string(),
        parameters: vec![
            Parameter {
                name: "mailboxes".to_string(),
                type_hint: "array".to_string(),
                description: "Array of mailbox objects with name, delimiter, and flags".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_list",
            "mailboxes": [
                {
                    "name": "INBOX",
                    "delimiter": "/",
                    "flags": ["\\HasNoChildren"]
                },
                {
                    "name": "Sent",
                    "delimiter": "/",
                    "flags": []
                }
            ]
        }),
    }
}

fn send_imap_status_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_status".to_string(),
        description: "Send IMAP STATUS response with mailbox status information".to_string(),
        parameters: vec![
            Parameter {
                name: "mailbox".to_string(),
                type_hint: "string".to_string(),
                description: "Mailbox name".to_string(),
                required: true,
            },
            Parameter {
                name: "items".to_string(),
                type_hint: "object".to_string(),
                description: "Status items (MESSAGES, RECENT, UIDNEXT, UIDVALIDITY, UNSEEN)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_status",
            "mailbox": "INBOX",
            "items": {
                "MESSAGES": 5,
                "RECENT": 2,
                "UIDNEXT": 1006,
                "UIDVALIDITY": 1234567890,
                "UNSEEN": 3
            }
        }),
    }
}

fn send_imap_fetch_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_fetch".to_string(),
        description: "Send IMAP FETCH response with message data".to_string(),
        parameters: vec![
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "Message sequence number".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "object".to_string(),
                description: "Message data (FLAGS, UID, RFC822.SIZE, BODY[], ENVELOPE, etc.)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_fetch",
            "sequence": 1,
            "data": {
                "FLAGS": ["\\Seen"],
                "UID": 1001,
                "RFC822.SIZE": 2048,
                "BODY[]": "From: sender@example.com\r\nSubject: Test\r\n\r\nHello World"
            }
        }),
    }
}

fn send_imap_search_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_search".to_string(),
        description: "Send IMAP SEARCH response with matching message IDs".to_string(),
        parameters: vec![
            Parameter {
                name: "results".to_string(),
                type_hint: "array".to_string(),
                description: "Array of message sequence numbers matching search criteria".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_search",
            "results": [1, 3, 5]
        }),
    }
}

fn send_imap_exists_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_exists".to_string(),
        description: "Send IMAP EXISTS response indicating number of messages in mailbox".to_string(),
        parameters: vec![
            Parameter {
                name: "count".to_string(),
                type_hint: "number".to_string(),
                description: "Number of messages that exist".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_exists",
            "count": 5
        }),
    }
}

fn send_imap_recent_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_recent".to_string(),
        description: "Send IMAP RECENT response indicating number of recent messages".to_string(),
        parameters: vec![
            Parameter {
                name: "count".to_string(),
                type_hint: "number".to_string(),
                description: "Number of recent messages".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_recent",
            "count": 2
        }),
    }
}

fn send_imap_flags_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_flags".to_string(),
        description: "Send IMAP FLAGS response with available message flags".to_string(),
        parameters: vec![
            Parameter {
                name: "flags".to_string(),
                type_hint: "array".to_string(),
                description: "Array of available flags".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_flags",
            "flags": ["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"]
        }),
    }
}

fn send_imap_expunge_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_imap_expunge".to_string(),
        description: "Send IMAP EXPUNGE response indicating a message was permanently removed".to_string(),
        parameters: vec![
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "Sequence number of the expunged message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_imap_expunge",
            "sequence": 3
        }),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data from client before processing".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the IMAP connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}

// ============================================================================
// Event Types
// ============================================================================

pub static IMAP_CONNECTION_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "imap_connection",
        "Initial IMAP connection established - send greeting"
    )
    .with_parameters(vec![])
    .with_actions(vec![
        send_imap_greeting_action(),
    ])
});

pub static IMAP_AUTH_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "imap_auth",
        "IMAP LOGIN command received - authenticate user"
    )
    .with_parameters(vec![
        Parameter {
            name: "tag".to_string(),
            type_hint: "string".to_string(),
            description: "Command tag".to_string(),
            required: true,
        },
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Username for authentication".to_string(),
            required: true,
        },
        Parameter {
            name: "password".to_string(),
            type_hint: "string".to_string(),
            description: "Password for authentication".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_imap_response_action(),
        close_connection_action(),
    ])
});

pub static IMAP_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "imap_command",
        "IMAP command received from client"
    )
    .with_parameters(vec![
        Parameter {
            name: "tag".to_string(),
            type_hint: "string".to_string(),
            description: "Command tag".to_string(),
            required: true,
        },
        Parameter {
            name: "command".to_string(),
            type_hint: "string".to_string(),
            description: "IMAP command (CAPABILITY, SELECT, LIST, FETCH, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "args".to_string(),
            type_hint: "string".to_string(),
            description: "Command arguments".to_string(),
            required: false,
        },
        Parameter {
            name: "session_state".to_string(),
            type_hint: "string".to_string(),
            description: "Current session state (NotAuthenticated, Authenticated, Selected, Logout)".to_string(),
            required: true,
        },
        Parameter {
            name: "authenticated_user".to_string(),
            type_hint: "string".to_string(),
            description: "Authenticated username (if any)".to_string(),
            required: false,
        },
        Parameter {
            name: "selected_mailbox".to_string(),
            type_hint: "string".to_string(),
            description: "Currently selected mailbox (if any)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_imap_response_action(),
        send_imap_untagged_action(),
        send_imap_capability_action(),
        send_imap_list_action(),
        send_imap_status_action(),
        send_imap_fetch_action(),
        send_imap_search_action(),
        send_imap_exists_action(),
        send_imap_recent_action(),
        send_imap_flags_action(),
        send_imap_expunge_action(),
        wait_for_more_action(),
        close_connection_action(),
    ])
});

pub fn get_imap_event_types() -> Vec<EventType> {
    vec![
        IMAP_CONNECTION_EVENT.clone(),
        IMAP_AUTH_EVENT.clone(),
        IMAP_COMMAND_EVENT.clone(),
    ]
}
