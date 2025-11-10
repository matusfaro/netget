//! BLE Weight Scale Service
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static WEIGHT_MEASUREMENT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("weight_measurement", "Weight measurement updated").with_parameters(vec![
        Parameter { name: "weight_kg".to_string(), type_hint: "number".to_string(), description: "Weight in kilograms".to_string(), required: true },
        Parameter { name: "bmi".to_string(), type_hint: "number".to_string(), description: "Body Mass Index".to_string(), required: true },
    ])
});

pub struct BluetoothBleWeightScaleProtocol;
impl BluetoothBleWeightScaleProtocol { pub fn new() -> Self { Self } }

impl Protocol for BluetoothBleWeightScaleProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> { vec![] }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_weight".to_string(),
                description: "Set weight measurement".to_string(),
                parameters: vec![
                    Parameter { name: "kg".to_string(), type_hint: "number".to_string(), description: "Weight in kilograms".to_string(), required: true },
                ],
                example: json!({
                    "type": "set_weight",
                    "kg": 70.5
                }),
            },
            ActionDefinition {
                name: "set_bmi".to_string(),
                description: "Set Body Mass Index".to_string(),
                parameters: vec![
                    Parameter { name: "bmi".to_string(), type_hint: "number".to_string(), description: "BMI value".to_string(), required: true },
                ],
                example: json!({
                    "type": "set_bmi",
                    "bmi": 22.5
                }),
            },
            ActionDefinition {
                name: "multi_user".to_string(),
                description: "Support multiple user profiles".to_string(),
                parameters: vec![
                    Parameter { name: "user_id".to_string(), type_hint: "number".to_string(), description: "User ID (1-9)".to_string(), required: true },
                    Parameter { name: "weight_kg".to_string(), type_hint: "number".to_string(), description: "Weight in kg".to_string(), required: true },
                ],
                example: json!({
                    "type": "multi_user",
                    "user_id": 1,
                    "weight_kg": 70.5
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> { vec![] }
    fn protocol_name(&self) -> &'static str { "BLUETOOTH_BLE_WEIGHT_SCALE" }
    fn get_event_types(&self) -> Vec<EventType> { vec![WEIGHT_MEASUREMENT_EVENT.clone()] }
    fn stack_name(&self) -> &'static str { "DATALINK" }
    fn keywords(&self) -> Vec<&'static str> { vec!["bluetooth", "weight", "scale", "health"] }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
        ProtocolMetadataV2::builder().state(DevelopmentState::Experimental)
            .implementation("BLE Weight Scale Service (0x181D)")
            .llm_control("Actions: set_weight, set_bmi, multi_user")
            .e2e_testing("Requires BLE device")
            .notes("Standard GATT Weight Scale Service")
            .build()
    }

    fn description(&self) -> &'static str { "BLE Weight Scale - weight and BMI monitoring" }
    fn example_prompt(&self) -> &'static str { "Act as a weight scale showing 70.5 kg" }
    fn group_name(&self) -> &'static str { "Network" }
}

impl Server for BluetoothBleWeightScaleProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            let instruction = ctx.state.get_server(ctx.server_id).await.map(|s| s.instruction).unwrap_or_default();
            crate::server::bluetooth_ble_weight_scale::BluetoothBleWeightScale::spawn_with_llm_actions(
                "NetGet-Scale".to_string(), ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id, instruction
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        match action_type {
            "set_weight" | "set_bmi" | "multi_user" => Ok(ActionResult::Custom { name: action_type.to_string(), data: action }),
            _ => Err(anyhow::anyhow!("Unknown weight scale action: {}", action_type)),
        }
    }
}
