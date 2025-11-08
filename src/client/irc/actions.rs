//! IRC client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// IRC client connected event
pub static IRC_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "irc_connected",
        "IRC client successfully connected to server"
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote IRC server address".to_string(),
            required: true,
        },
        Parameter {
            name: "nickname".to_string(),
            type_hint: "string".to_string(),
            description: "Nickname registered with the server".to_string(),
            required: true,
        },
    ])
});

/// IRC message received event
pub static IRC_CLIENT_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "irc_message_received",
        "IRC message received from server or channel"
    )
    .with_parameters(vec![
        Parameter {
            name: "source".to_string(),
            type_hint: "string".to_string(),
            description: "Source of the message (nick!user@host or server)".to_string(),
            required: false,
        },
        Parameter {
            name: "command".to_string(),
            type_hint: "string".to_string(),
            description: "IRC command (e.g., PRIVMSG, JOIN, NOTICE, 001, 433)".to_string(),
            required: true,
        },
        Parameter {
            name: "target".to_string(),
            type_hint: "string".to_string(),
            description: "Target of the message (channel or user)".to_string(),
            required: false,
        },
        Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "The message text".to_string(),
            required: false,
        },
        Parameter {
            name: "raw_message".to_string(),
            type_hint: "string".to_string(),
            description: "The raw IRC message line".to_string(),
            required: true,
        },
    ])
});

/// IRC client protocol action handler
pub struct IrcClientProtocol;

impl IrcClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IrcClientProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "join_channel".to_string(),
                    description: "Join an IRC channel".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "channel".to_string(),
                            type_hint: "string".to_string(),
                            description: "Channel name (e.g., '#rust')".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "join_channel",
                        "channel": "#rust"
                    }),
                },
                ActionDefinition {
                    name: "part_channel".to_string(),
                    description: "Leave an IRC channel".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "channel".to_string(),
                            type_hint: "string".to_string(),
                            description: "Channel name to leave".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "message".to_string(),
                            type_hint: "string".to_string(),
                            description: "Optional part message".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "part_channel",
                        "channel": "#rust",
                        "message": "Goodbye!"
                    }),
                },
                ActionDefinition {
                    name: "change_nick".to_string(),
                    description: "Change the client's nickname".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "new_nick".to_string(),
                            type_hint: "string".to_string(),
                            description: "New nickname to use".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "change_nick",
                        "new_nick": "newname"
                    }),
                },
                ActionDefinition {
                    name: "disconnect".to_string(),
                    description: "Disconnect from the IRC server".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "quit_message".to_string(),
                            type_hint: "string".to_string(),
                            description: "Optional quit message".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "disconnect",
                        "quit_message": "Leaving"
                    }),
                },
            ]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "send_privmsg".to_string(),
                    description: "Send a PRIVMSG to a channel or user".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "target".to_string(),
                            type_hint: "string".to_string(),
                            description: "Target channel or user".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "message".to_string(),
                            type_hint: "string".to_string(),
                            description: "Message text to send".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "send_privmsg",
                        "target": "#rust",
                        "message": "Hello, channel!"
                    }),
                },
                ActionDefinition {
                    name: "send_notice".to_string(),
                    description: "Send a NOTICE to a channel or user".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "target".to_string(),
                            type_hint: "string".to_string(),
                            description: "Target channel or user".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "message".to_string(),
                            type_hint: "string".to_string(),
                            description: "Notice text to send".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "send_notice",
                        "target": "#rust",
                        "message": "Bot notification"
                    }),
                },
                ActionDefinition {
                    name: "send_raw".to_string(),
                    description: "Send a raw IRC command".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "command".to_string(),
                            type_hint: "string".to_string(),
                            description: "Raw IRC command (without CRLF)".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "send_raw",
                        "command": "MODE #rust +m"
                    }),
                },
                ActionDefinition {
                    name: "wait_for_more".to_string(),
                    description: "Wait for more messages before responding".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "wait_for_more"
                    }),
                },
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "IRC"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                EventType {
                    id: "irc_connected".to_string(),
                    description: "Triggered when IRC client connects and registers".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
                EventType {
                    id: "irc_message_received".to_string(),
                    description: "Triggered when IRC client receives any message".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
            ]
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>IRC"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["irc", "irc client", "chat", "connect to irc"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("irc crate for IRC protocol handling")
                .llm_control("Join/part channels, send messages, change nick")
                .e2e_testing("ngircd or inspircd as test server")
                .build()
        }
        fn description(&self) -> &'static str {
            "IRC client for connecting to IRC servers and chat channels"
        }
        fn example_prompt(&self) -> &'static str {
            "Connect to IRC at irc.libera.chat:6667 with nick testbot, join #test and say hello"
        }
        fn group_name(&self) -> &'static str {
            "Messaging"
        }
        fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
            vec![
                ParameterDefinition {
                    name: "nickname".to_string(),
                    type_hint: "string".to_string(),
                    description: "IRC nickname (default: netget_user)".to_string(),
                    required: false,
                    example: json!("mybot"),
                },
                ParameterDefinition {
                    name: "username".to_string(),
                    type_hint: "string".to_string(),
                    description: "IRC username (default: netget)".to_string(),
                    required: false,
                    example: json!("botuser"),
                },
                ParameterDefinition {
                    name: "realname".to_string(),
                    type_hint: "string".to_string(),
                    description: "IRC real name (default: NetGet IRC Client)".to_string(),
                    required: false,
                    example: json!("My IRC Bot"),
                },
            ]
        }
}

// Implement Client trait (client-specific functionality)
impl Client for IrcClientProtocol {
        fn connect(
            &self,
            ctx: crate::protocol::ConnectContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::client::irc::IrcClient;
                IrcClient::connect_with_llm_actions(
                    ctx.remote_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.client_id,
                    ctx.startup_params,
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
                "join_channel" => {
                    let channel = action
                        .get("channel")
                        .and_then(|v| v.as_str())
                        .context("Missing 'channel' field")?;
    
                    Ok(ClientActionResult::Custom {
                        name: "join_channel".to_string(),
                        data: json!({ "channel": channel }),
                    })
                }
                "part_channel" => {
                    let channel = action
                        .get("channel")
                        .and_then(|v| v.as_str())
                        .context("Missing 'channel' field")?;
                    let message = action.get("message").and_then(|v| v.as_str());
    
                    Ok(ClientActionResult::Custom {
                        name: "part_channel".to_string(),
                        data: json!({ "channel": channel, "message": message }),
                    })
                }
                "change_nick" => {
                    let new_nick = action
                        .get("new_nick")
                        .and_then(|v| v.as_str())
                        .context("Missing 'new_nick' field")?;
    
                    Ok(ClientActionResult::Custom {
                        name: "change_nick".to_string(),
                        data: json!({ "new_nick": new_nick }),
                    })
                }
                "send_privmsg" => {
                    let target = action
                        .get("target")
                        .and_then(|v| v.as_str())
                        .context("Missing 'target' field")?;
                    let message = action
                        .get("message")
                        .and_then(|v| v.as_str())
                        .context("Missing 'message' field")?;
    
                    Ok(ClientActionResult::Custom {
                        name: "send_privmsg".to_string(),
                        data: json!({ "target": target, "message": message }),
                    })
                }
                "send_notice" => {
                    let target = action
                        .get("target")
                        .and_then(|v| v.as_str())
                        .context("Missing 'target' field")?;
                    let message = action
                        .get("message")
                        .and_then(|v| v.as_str())
                        .context("Missing 'message' field")?;
    
                    Ok(ClientActionResult::Custom {
                        name: "send_notice".to_string(),
                        data: json!({ "target": target, "message": message }),
                    })
                }
                "send_raw" => {
                    let command = action
                        .get("command")
                        .and_then(|v| v.as_str())
                        .context("Missing 'command' field")?;
    
                    Ok(ClientActionResult::Custom {
                        name: "send_raw".to_string(),
                        data: json!({ "command": command }),
                    })
                }
                "disconnect" => {
                    let quit_message = action.get("quit_message").and_then(|v| v.as_str());
                    Ok(ClientActionResult::Custom {
                        name: "disconnect".to_string(),
                        data: json!({ "quit_message": quit_message }),
                    })
                }
                "wait_for_more" => Ok(ClientActionResult::WaitForMore),
                _ => Err(anyhow::anyhow!("Unknown IRC client action: {}", action_type)),
            }
        }
}

