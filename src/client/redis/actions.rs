//! Redis client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// Redis client protocol action handler
pub struct RedisClientProtocol;

impl RedisClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for RedisClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::redis::RedisClient;
            RedisClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_redis_command".to_string(),
                description: "Execute a Redis command".to_string(),
                parameters: vec![
                    Parameter {
                        name: "command".to_string(),
                        type_hint: "string".to_string(),
                        description: "Redis command (e.g., GET key, SET key value)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "execute_redis_command",
                    "command": "GET mykey"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the Redis server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_redis_command".to_string(),
                description: "Execute a Redis command in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "command".to_string(),
                        type_hint: "string".to_string(),
                        description: "Redis command".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "execute_redis_command",
                    "command": "SET result OK"
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "execute_redis_command" => {
                let command = action
                    .get("command")
                    .and_then(|v| v.as_str())
                    .context("Missing 'command' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "redis_command".to_string(),
                    data: json!({
                        "command": command,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown Redis client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Redis"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "redis_connected".to_string(),
                description: "Triggered when Redis client connects to server".to_string(),
                available_actions: vec!["execute_redis_command".to_string()],
            },
            EventType {
                id: "redis_response_received".to_string(),
                description: "Triggered when Redis client receives a response".to_string(),
                available_actions: vec!["execute_redis_command".to_string()],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>Redis"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["redis", "redis client", "connect to redis"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Direct TCP with simplified RESP parsing")
            .llm_control("Full control over Redis commands")
            .e2e_testing("Docker Redis container")
            .build()
    }

    fn description(&self) -> &'static str {
        "Redis client for key-value operations"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to Redis at localhost:6379 and get the value of 'user:123'"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}
