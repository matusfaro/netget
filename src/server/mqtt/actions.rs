//! MQTT protocol action definitions and implementations

use crate::llm::actions::protocol_trait::{ActionResult, Protocol};
use crate::llm::actions::{ActionDefinition, Server};
use crate::state::app_state::AppState;
use anyhow::Result;

/// MQTT protocol action handler
pub struct MqttProtocol;

impl MqttProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for MqttProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![] // No async actions for placeholder
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![] // No sync actions for placeholder
    }
    fn protocol_name(&self) -> &'static str {
        "MQTT"
    }
    fn get_event_types(&self) -> Vec<crate::protocol::EventType> {
        vec![] // No events for placeholder
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>MQTT"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["mqtt", "mosquitto", "iot messaging"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("rumqttd or manual")
            .llm_control("Pub/sub message routing")
            .e2e_testing("rumqttc")
            .notes("IoT messaging")
            .build()
    }
    fn description(&self) -> &'static str {
        "MQTT broker for IoT messaging"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an MQTT broker on port 1883 for IoT device messaging"
    }
    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for MqttProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::mqtt::MqttServer;
            MqttServer::spawn_with_llm_actions(
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
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Err(anyhow::anyhow!(
            "MQTT action execution not yet implemented: {}",
            action_type
        ))
    }
}
