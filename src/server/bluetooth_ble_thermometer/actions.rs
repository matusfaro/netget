//! BLE Thermometer Service
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static TEMPERATURE_UPDATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("temperature_updated", "Temperature was updated").with_parameters(vec![Parameter {
        name: "celsius".to_string(),
        type_hint: "number".to_string(),
        description: "Temperature in Celsius".to_string(),
        required: true,
    }])
});

pub struct BluetoothBleThermometerProtocol;
impl BluetoothBleThermometerProtocol { pub fn new() -> Self { Self } }

impl Protocol for BluetoothBleThermometerProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> { vec![] }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![ActionDefinition {
            name: "set_temperature".to_string(),
            description: "Set temperature".to_string(),
            parameters: vec![
                Parameter { name: "celsius".to_string(), type_hint: "number".to_string(), description: "Temperature in Celsius".to_string(), required: true}
            ],
            example: json!({"type": "set_temperature", "celsius": 37.0}),
        }]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> { vec![] }
    fn protocol_name(&self) -> &'static str { "BLUETOOTH_BLE_THERMOMETER" }
    fn get_event_types(&self) -> Vec<EventType> { vec![TEMPERATURE_UPDATED_EVENT.clone()] }
    fn stack_name(&self) -> &'static str { "DATALINK" }
    fn keywords(&self) -> Vec<&'static str> { vec!["bluetooth", "ble", "thermometer", "temperature"] }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
        ProtocolMetadataV2::builder().state(DevelopmentState::Experimental).implementation("BLE Health Thermometer (0x1809)").llm_control("Actions: set_temperature").e2e_testing("Requires BLE device").notes("Standard GATT Thermometer Service").build()
    }
    fn description(&self) -> &'static str { "BLE Health Thermometer - temperature monitoring" }
    fn example_prompt(&self) -> &'static str { "Act as a thermometer at 37°C body temperature" }
    fn group_name(&self) -> &'static str { "Network" }
}

impl Server for BluetoothBleThermometerProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            // Get instruction from server instance
            let instruction = ctx.state.get_server(ctx.server_id).await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_thermometer::BluetoothBleThermometer::spawn_with_llm_actions("NetGet-Thermometer".to_string(), ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id, instruction).await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        match action_type {
            "set_temperature" => Ok(ActionResult::Custom { name: action_type.to_string(), data: action }),
            _ => Err(anyhow::anyhow!("Unknown thermometer action: {}", action_type)),
        }
    }
}
