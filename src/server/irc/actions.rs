//! IRC protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use tracing::debug;

/// IRC client state for tracking nicknames and channels
#[derive(Clone, Debug)]
pub struct IrcClientState {
    pub nickname: Option<String>,
    pub username: Option<String>,
    pub realname: Option<String>,
    pub channels: Vec<String>,
}

impl IrcClientState {
    pub fn new() -> Self {
        Self {
            nickname: None,
            username: None,
            realname: None,
            channels: Vec::new(),
        }
    }
}

/// IRC protocol action handler
pub struct IrcProtocol {
    /// Map of active connections to their IRC state
    clients: Arc<Mutex<HashMap<ConnectionId, IrcClientState>>>,
}

impl IrcProtocol {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a connection to the protocol handler
    pub async fn add_connection(&self, connection_id: ConnectionId) {
        self.clients
            .lock()
            .await
            .insert(connection_id, IrcClientState::new());
    }

    /// Remove a connection from the protocol handler
    pub async fn remove_connection(&self, connection_id: &ConnectionId) {
        self.clients.lock().await.remove(connection_id);
    }

    /// Update client nickname
    pub async fn set_nickname(&self, connection_id: ConnectionId, nickname: String) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            client.nickname = Some(nickname);
        }
    }

    /// Update client username and realname
    pub async fn set_user_info(
        &self,
        connection_id: ConnectionId,
        username: String,
        realname: String,
    ) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            client.username = Some(username);
            client.realname = Some(realname);
        }
    }

    /// Add a channel to client's channel list
    pub async fn join_channel(&self, connection_id: ConnectionId, channel: String) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            if !client.channels.contains(&channel) {
                client.channels.push(channel);
            }
        }
    }

    /// Remove a channel from client's channel list
    pub async fn part_channel(&self, connection_id: ConnectionId, channel: &str) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            client.channels.retain(|c| c != channel);
        }
    }

    /// Get client state
    pub async fn get_client_state(&self, connection_id: &ConnectionId) -> Option<IrcClientState> {
        self.clients.lock().await.get(connection_id).cloned()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IrcProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
                crate::llm::actions::ParameterDefinition {
                    name: "send_first".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Whether the server should send the first message after connection (not typically needed for this protocol)".to_string(),
                    required: false,
                    example: serde_json::json!(false),
                },
            ]
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
            .implementation("Manual line-based IRC parsing")
            .llm_control("All IRC messages (NICK, JOIN, PRIVMSG)")
            .e2e_testing("Manual IRC client")
            .notes("No channel state tracking")
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
            let _send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
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
        log_template: None,
    }
}

// ============================================================================
// IRC Event Type Constants
// ============================================================================

pub static IRC_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("irc_message_received", "IRC message received from a client", json!({"type": "placeholder", "event_id": "irc_message_received"}))
        .with_parameters(vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "The IRC message line received".to_string(),
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
