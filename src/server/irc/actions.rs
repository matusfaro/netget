//! IRC protocol actions implementation

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

/// IRC protocol action handler
///
/// Deliberately stateless. An `IrcClientState` map (nickname, username, realname, channels)
/// used to live here with insert/update/lookup helpers, but nothing ever called them - no
/// nickname was ever recorded and no channel was ever joined. It was also the wrong place for
/// it: protocols do not keep protocol state in Rust, the model tracks nicknames and channel
/// membership through its instruction and server memory.
pub struct IrcProtocol;

impl IrcProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IrcProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        // No `send_first`: it was declared here but `spawn` read it into `_send_first` and threw
        // it away, so callers were told the server could open the conversation when it never
        // did. An IRC server does not speak first anyway - the client sends NICK/USER. Not
        // declaring it makes `server_startup` warn that the flag is unsupported instead of
        // silently dropping it. Same reasoning as `src/server/ftp/actions.rs:215`.
        Vec::new()
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // IRC could have async actions like broadcast_message in the future
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_irc_message_action(),
            send_irc_welcome_action(),
            send_irc_pong_action(),
            send_irc_join_action(),
            send_irc_part_action(),
            send_irc_privmsg_action(),
            send_irc_notice_action(),
            send_irc_numeric_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "IRC"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_irc_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>IRC"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["irc", "chat"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual line-based IRC parsing, plain TCP only")
            .llm_control("Every inbound line; the model composes every reply")
            .e2e_testing("Raw TCP client issuing NICK/USER/JOIN/PRIVMSG")
            .notes(
                "Single-client view: no channel membership, no nick registry, no broadcast \
                 between connections, no TLS. The model tracks all of that itself.",
            )
            .build()
    }
    fn description(&self) -> &'static str {
        "IRC chat server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an IRC server"
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
                "port": 6667,
                "base_stack": "irc",
                "instruction": "IRC chat server, send welcome on NICK/USER, echo messages back"
            }),
            // Script-based example
            json!({
                "type": "open_server",
                "port": 6667,
                "base_stack": "irc",
                "event_handlers": [{
                    "event_pattern": "irc_message_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# Handle IRC messages\nmsg = event.get('message', '').strip()\nif msg.startswith('NICK'):\n    nick = msg.split()[1] if len(msg.split()) > 1 else 'guest'\n    respond([{'type': 'send_irc_welcome', 'nickname': nick, 'server': 'irc.netget.local', 'message': 'Welcome to NetGet IRC'}])\nelif msg.startswith('PING'):\n    token = msg.split(':')[1] if ':' in msg else ''\n    respond([{'type': 'send_irc_pong', 'token': token}])\nelif msg.startswith('JOIN'):\n    channel = msg.split()[1] if len(msg.split()) > 1 else '#general'\n    respond([{'type': 'send_irc_join', 'nickname': 'guest', 'channel': channel}])\nelse:\n    respond([{'type': 'wait_for_more'}])"
                    }
                }]
            }),
            // Static handler example
            json!({
                "type": "open_server",
                "port": 6667,
                "base_stack": "irc",
                "event_handlers": [{
                    "event_pattern": "irc_message_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_irc_welcome",
                            "nickname": "guest",
                            "server": "irc.netget.local",
                            "message": "Welcome to NetGet IRC"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for IrcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::irc::IrcServer;

            IrcServer::spawn_with_llm_actions(
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
            "send_irc_message" => self.execute_send_irc_message(action),
            "send_irc_welcome" => self.execute_send_irc_welcome(action),
            "send_irc_pong" => self.execute_send_irc_pong(action),
            "send_irc_join" => self.execute_send_irc_join(action),
            "send_irc_part" => self.execute_send_irc_part(action),
            "send_irc_privmsg" => self.execute_send_irc_privmsg(action),
            "send_irc_notice" => self.execute_send_irc_notice(action),
            "send_irc_numeric" => self.execute_send_irc_numeric(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown IRC action: {}", action_type)),
        }
    }
}

impl IrcProtocol {
    fn execute_send_irc_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // Ensure IRC messages end with \r\n
        let formatted = if message.ends_with("\r\n") {
            message.to_string()
        } else if message.ends_with('\n') {
            format!("{}\r", message.trim_end_matches('\n'))
        } else {
            format!("{}\r\n", message)
        };

        debug!("IRC sending message: {}", formatted.trim());
        Ok(ActionResult::Output(formatted.as_bytes().to_vec()))
    }

    fn execute_send_irc_welcome(&self, action: serde_json::Value) -> Result<ActionResult> {
        let nickname = action
            .get("nickname")
            .and_then(|v| v.as_str())
            .context("Missing 'nickname' parameter")?;

        let server = action
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("irc.server");

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Welcome to the IRC Network");

        // IRC numeric 001 (RPL_WELCOME)
        let response = format!(":{} 001 {} :{}\r\n", server, nickname, message);

        debug!("IRC sending welcome: {}", response.trim());
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_pong(&self, action: serde_json::Value) -> Result<ActionResult> {
        let token = action
            .get("token")
            .and_then(|v| v.as_str())
            .context("Missing 'token' parameter")?;

        let response = format!("PONG :{}\r\n", token);

        debug!("IRC sending PONG: {}", token);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_join(&self, action: serde_json::Value) -> Result<ActionResult> {
        let nickname = action
            .get("nickname")
            .and_then(|v| v.as_str())
            .context("Missing 'nickname' parameter")?;

        let channel = action
            .get("channel")
            .and_then(|v| v.as_str())
            .context("Missing 'channel' parameter")?;

        let user = action
            .get("user")
            .and_then(|v| v.as_str())
            .unwrap_or("user");

        let host = action
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        // IRC JOIN message
        let response = format!(":{nickname}!{user}@{host} JOIN {channel}\r\n");

        debug!("IRC sending JOIN: {} to {}", nickname, channel);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_part(&self, action: serde_json::Value) -> Result<ActionResult> {
        let nickname = action
            .get("nickname")
            .and_then(|v| v.as_str())
            .context("Missing 'nickname' parameter")?;

        let channel = action
            .get("channel")
            .and_then(|v| v.as_str())
            .context("Missing 'channel' parameter")?;

        let user = action
            .get("user")
            .and_then(|v| v.as_str())
            .unwrap_or("user");

        let host = action
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let reason = action.get("reason").and_then(|v| v.as_str());

        // IRC PART message
        let response = if let Some(reason) = reason {
            format!(":{nickname}!{user}@{host} PART {channel} :{reason}\r\n")
        } else {
            format!(":{nickname}!{user}@{host} PART {channel}\r\n")
        };

        debug!("IRC sending PART: {} from {}", nickname, channel);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_privmsg(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source = action
            .get("source")
            .and_then(|v| v.as_str())
            .context("Missing 'source' parameter")?;

        let target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // IRC PRIVMSG
        let response = format!(":{} PRIVMSG {} :{}\r\n", source, target, message);

        debug!("IRC sending PRIVMSG from {} to {}", source, target);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_notice(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source = action
            .get("source")
            .and_then(|v| v.as_str())
            .context("Missing 'source' parameter")?;

        let target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // IRC NOTICE
        let response = format!(":{} NOTICE {} :{}\r\n", source, target, message);

        debug!("IRC sending NOTICE from {} to {}", source, target);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_irc_numeric(&self, action: serde_json::Value) -> Result<ActionResult> {
        let server = action
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("irc.server");

        let code = action
            .get("code")
            .and_then(|v| v.as_u64())
            .context("Missing 'code' parameter")?;

        let target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // IRC numeric response
        let response = format!(":{} {:03} {} :{}\r\n", server, code, target, message);

        debug!("IRC sending numeric {}: {}", code, message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }
}

// Action definitions

fn send_irc_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_message".to_string(),
        description: "Send a raw IRC message (for custom responses)".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "IRC message to send (will auto-add \\r\\n if not present)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_irc_message",
            "message": ":server NOTICE * :Looking up your hostname"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC: {preview(message,60)}")
                .with_debug("IRC send_irc_message: {message}"),
        ),
    }
}

fn send_irc_welcome_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_welcome".to_string(),
        description: "Send IRC welcome message (numeric 001 - RPL_WELCOME)".to_string(),
        parameters: vec![
            Parameter {
                name: "nickname".to_string(),
                type_hint: "string".to_string(),
                description: "Client nickname".to_string(),
                required: true,
            },
            Parameter {
                name: "server".to_string(),
                type_hint: "string".to_string(),
                description: "Server name (default: irc.server)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Welcome message (default: 'Welcome to the IRC Network')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_irc_welcome",
            "nickname": "alice",
            "server": "irc.example.com",
            "message": "Welcome to the IRC Network, alice!"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC 001 {nickname}: {message}")
                .with_debug("IRC send_irc_welcome: nickname={nickname}, server={server}"),
        ),
    }
}

fn send_irc_pong_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_pong".to_string(),
        description: "Send IRC PONG response to PING".to_string(),
        parameters: vec![Parameter {
            name: "token".to_string(),
            type_hint: "string".to_string(),
            description: "Token from PING command".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_irc_pong",
            "token": "1234567890"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC PONG :{token}")
                .with_debug("IRC send_irc_pong: token={token}"),
        ),
    }
}

fn send_irc_join_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_join".to_string(),
        description: "Send IRC JOIN confirmation".to_string(),
        parameters: vec![
            Parameter {
                name: "nickname".to_string(),
                type_hint: "string".to_string(),
                description: "Client nickname".to_string(),
                required: true,
            },
            Parameter {
                name: "channel".to_string(),
                type_hint: "string".to_string(),
                description: "Channel name (e.g., #general)".to_string(),
                required: true,
            },
            Parameter {
                name: "user".to_string(),
                type_hint: "string".to_string(),
                description: "Username (default: user)".to_string(),
                required: false,
            },
            Parameter {
                name: "host".to_string(),
                type_hint: "string".to_string(),
                description: "Hostname (default: localhost)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_irc_join",
            "nickname": "alice",
            "channel": "#general"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC {nickname} JOIN {channel}")
                .with_debug("IRC send_irc_join: nickname={nickname}, channel={channel}"),
        ),
    }
}

fn send_irc_part_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_part".to_string(),
        description: "Send IRC PART confirmation (leaving channel)".to_string(),
        parameters: vec![
            Parameter {
                name: "nickname".to_string(),
                type_hint: "string".to_string(),
                description: "Client nickname".to_string(),
                required: true,
            },
            Parameter {
                name: "channel".to_string(),
                type_hint: "string".to_string(),
                description: "Channel name (e.g., #general)".to_string(),
                required: true,
            },
            Parameter {
                name: "user".to_string(),
                type_hint: "string".to_string(),
                description: "Username (default: user)".to_string(),
                required: false,
            },
            Parameter {
                name: "host".to_string(),
                type_hint: "string".to_string(),
                description: "Hostname (default: localhost)".to_string(),
                required: false,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Part reason (optional)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_irc_part",
            "nickname": "alice",
            "channel": "#general",
            "reason": "Goodbye!"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC {nickname} PART {channel}")
                .with_debug(
                    "IRC send_irc_part: nickname={nickname}, channel={channel}, reason={reason}",
                ),
        ),
    }
}

fn send_irc_privmsg_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_privmsg".to_string(),
        description: "Send IRC PRIVMSG (chat message)".to_string(),
        parameters: vec![
            Parameter {
                name: "source".to_string(),
                type_hint: "string".to_string(),
                description: "Source (nickname or server)".to_string(),
                required: true,
            },
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target (nickname or channel)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Message text".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_irc_privmsg",
            "source": "bot",
            "target": "alice",
            "message": "Hello, alice!"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC {source} -> {target}: {preview(message,40)}")
                .with_debug("IRC send_irc_privmsg: source={source}, target={target}"),
        ),
    }
}

fn send_irc_notice_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_notice".to_string(),
        description: "Send IRC NOTICE (notification message)".to_string(),
        parameters: vec![
            Parameter {
                name: "source".to_string(),
                type_hint: "string".to_string(),
                description: "Source (nickname or server)".to_string(),
                required: true,
            },
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target (nickname or channel)".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Notice text".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_irc_notice",
            "source": "server",
            "target": "alice",
            "message": "Server maintenance in 5 minutes"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC NOTICE {source} -> {target}")
                .with_debug(
                    "IRC send_irc_notice: source={source}, target={target}, message={message}",
                ),
        ),
    }
}

fn send_irc_numeric_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_numeric".to_string(),
        description: "Send IRC numeric response (e.g., 332 for topic, 353 for names)".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "Numeric code (e.g., 332, 353, 366)".to_string(),
                required: true,
            },
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target nickname".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Message text".to_string(),
                required: true,
            },
            Parameter {
                name: "server".to_string(),
                type_hint: "string".to_string(),
                description: "Server name (default: irc.server)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_irc_numeric",
            "code": 332,
            "target": "alice",
            "message": "#general Welcome to our channel!"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> IRC {code} {target}: {preview(message,40)}")
                .with_debug("IRC send_irc_numeric: code={code}, target={target}"),
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
                .with_info("IRC waiting for more data")
                .with_debug("IRC wait_for_more"),
        ),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the IRC connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("IRC connection closed")
                .with_debug("IRC close_connection"),
        ),
    }
}

// ============================================================================
// IRC Event Type Constants
// ============================================================================

pub static IRC_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "irc_message_received",
        "One IRC protocol line received from a client (NICK, USER, JOIN, PRIVMSG, PING, QUIT, ...). \
         The server does not speak first, so registration begins with the client's NICK/USER.",
        json!({"type": "send_irc_welcome", "nickname": "alice", "server": "irc.example.com", "message": "Welcome to the IRC Network"}),
    )
    .with_parameters(vec![Parameter {
        name: "message".to_string(),
        type_hint: "string".to_string(),
        description: "The IRC message line received, with the trailing CRLF stripped \
                      (e.g. 'NICK alice', 'PRIVMSG #general :hello')"
            .to_string(),
        required: true,
    }])
    .with_actions(vec![
        send_irc_message_action(),
        send_irc_welcome_action(),
        send_irc_pong_action(),
        send_irc_join_action(),
        send_irc_part_action(),
        send_irc_privmsg_action(),
        send_irc_notice_action(),
        send_irc_numeric_action(),
        wait_for_more_action(),
        close_connection_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("IRC {client_ip}: {preview(message,80)}")
            .with_debug("IRC message from {client_ip}:{client_port}")
            .with_trace("IRC: {json_pretty(.)}"),
    )
});

pub fn get_irc_event_types() -> Vec<EventType> {
    vec![IRC_MESSAGE_RECEIVED_EVENT.clone()]
}
