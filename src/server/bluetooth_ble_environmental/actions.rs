//! BLE Environmental Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};

pub struct BluetoothBleEnvironmentalProtocol;
impl BluetoothBleEnvironmentalProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleEnvironmentalProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Environmental sensor device name for advertising (default: NetGet-Environment)".to_string(),
                required: false,
                example: json!("NetGet-Environment"),
            },
        ]
    }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_ENVIRONMENTAL"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![]
    }
    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_ENVIRONMENTAL"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "environmental"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Environmental")
            .llm_control("Environmental actions")
            .e2e_testing("Requires BLE device")
            .notes("BLE Environmental")
            .build()
    }
    fn description(&self) -> &'static str {
        "BLE Environmental"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a BLE environmental device"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleEnvironmentalProtocol {
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
                .map(|s| s.instruction)
                .unwrap_or_default();
            crate::server::bluetooth_ble_environmental::BluetoothBleEnvironmental::spawn_with_llm_actions(
                "NetGet-Environmental".to_string(), ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id, instruction
            ).await
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
