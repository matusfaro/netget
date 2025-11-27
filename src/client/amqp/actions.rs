//! AMQP client protocol actions

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// AMQP client connected event
pub static AMQP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("amqp_connected", "AMQP client connected to broker", json!({"type": "placeholder", "event_id": "amqp_connected"})).with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote broker address".to_string(),
            required: true,
        },
    ])
});

/// AMQP client channel opened event
pub static AMQP_CLIENT_CHANNEL_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("amqp_channel_opened", "AMQP channel opened", json!({"type": "placeholder", "event_id": "amqp_channel_opened"})).with_parameters(vec![Parameter {
        name: "channel_id".to_string(),
        type_hint: "number".to_string(),
        description: "Channel ID".to_string(),
        required: true,
    }])
});

/// AMQP client message received event
pub static AMQP_CLIENT_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("amqp_message_received", "Message received from queue", json!({"type": "placeholder", "event_id": "amqp_message_received"})).with_parameters(vec![
        Parameter {
            name: "queue_name".to_string(),
            type_hint: "string".to_string(),
            description: "Queue name".to_string(),
            required: true,
        },
        Parameter {
            name: "message_body".to_string(),
            type_hint: "string".to_string(),
            description: "Message content".to_string(),
            required: true,
        },
    ])
});

/// AMQP client protocol
pub struct AmqpClientProtocol;

impl AmqpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for AmqpClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "open_channel".to_string(),
                description: "Open a new AMQP channel".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "open_channel"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the AMQP broker".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "AMQP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            AMQP_CLIENT_CONNECTED_EVENT.clone(),
            AMQP_CLIENT_CHANNEL_OPENED_EVENT.clone(),
            AMQP_CLIENT_MESSAGE_RECEIVED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>AMQP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["amqp", "rabbitmq", "client"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("lapin AMQP client library")
            .llm_control("Queue/exchange operations, message publishing/consuming")
            .e2e_testing("NetGet AMQP server or RabbitMQ")
            .notes("AMQP 0.9.1 client for RabbitMQ compatibility")
            .build()
    }

    fn description(&self) -> &'static str {
        "AMQP 0.9.1 client for connecting to RabbitMQ"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to RabbitMQ at localhost:5672 and publish messages"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for AmqpClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::amqp::AmqpClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: Value) -> Result<ClientActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Missing action type")?;

        match action_type {
            "open_channel" => Ok(ClientActionResult::Custom {
                name: "open_channel".to_string(),
                data: json!({}),
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Ok(ClientActionResult::WaitForMore),
        }
    }
}
