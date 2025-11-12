//! AMQP server protocol actions

use crate::llm::actions::protocol_trait::{ActionResult, Protocol};
use crate::llm::actions::{ActionDefinition, Server};
use crate::protocol::SpawnContext;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// AMQP server protocol
pub struct AmqpProtocol;

impl AmqpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for AmqpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![] // Placeholder - AMQP actions not yet implemented
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![] // Placeholder
    }

    fn protocol_name(&self) -> &'static str {
        "AMQP"
    }

    fn get_event_types(&self) -> Vec<crate::protocol::EventType> {
        vec![] // Placeholder
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>AMQP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["amqp", "rabbitmq", "broker", "messaging", "queue"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Custom AMQP 0.9.1 wire protocol")
            .llm_control("Queue/exchange declarations, message routing")
            .e2e_testing("lapin AMQP client")
            .notes("Simplified RabbitMQ-compatible broker")
            .build()
    }

    fn description(&self) -> &'static str {
        "AMQP 0.9.1 broker for message queuing"
    }

    fn example_prompt(&self) -> &'static str {
        "Start AMQP broker on port 5672 and create queues for task distribution"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for AmqpProtocol {
    fn spawn(
        &self,
        ctx: SpawnContext,
    ) -> Pin<Box<dyn Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::amqp::AmqpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            )
            .await
        })
    }

    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Err(anyhow::anyhow!(
            "AMQP action execution not yet implemented: {}",
            action_type
        ))
    }
}
