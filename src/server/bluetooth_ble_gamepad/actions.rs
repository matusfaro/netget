//! BLE Gamepad Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};

pub struct BluetoothBleGamepadProtocol;
impl BluetoothBleGamepadProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleGamepadProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![]
    }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_GAMEPAD"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![]
    }
    fn stack_name(&self) -> &'static str {
        "DATALINK>BLE_GAMEPAD"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "gamepad"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Gamepad")
            .llm_control("Gamepad actions")
            .e2e_testing("Requires BLE device")
            .notes("BLE Gamepad")
            .build()
    }
    fn description(&self) -> &'static str {
        "BLE Gamepad"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a BLE gamepad device"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleGamepadProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction.clone())
                .unwrap_or_default();
            crate::server::bluetooth_ble_gamepad::BluetoothBleGamepad::spawn_with_llm_actions(
                "NetGet-Gamepad".to_string(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                instruction,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;
        Ok(ActionResult::Custom {
            name: action_type.to_string(),
            data: action,
        })
    }
}
