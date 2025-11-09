//! BLE Cycling Speed and Cadence Service
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static CYCLING_MEASUREMENT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("cycling_measurement", "Cycling speed/cadence updated").with_parameters(vec![
        Parameter::new("speed_kmh", "Speed in km/h"),
        Parameter::new("cadence_rpm", "Cadence in RPM"),
    ])
});

pub struct BluetoothBleCyclingProtocol;
impl BluetoothBleCyclingProtocol { pub fn new() -> Self { Self } }

impl Protocol for BluetoothBleCyclingProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> { vec![] }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_speed".to_string(),
                description: "Set cycling speed".to_string(),
                parameters: vec![
                    Parameter { name: "kmh".to_string(), type_hint: "number".to_string(), description: "Speed in km/h".to_string(), required: true},
                ],
            example: json!({
            "type": "set_speed",
            "kmh": 42
        }),
    },
            ActionDefinition {
                name: "set_cadence".to_string(),
                description: "Set pedaling cadence".to_string(),
                parameters: vec![
                    Parameter { name: "rpm".to_string(), type_hint: "number".to_string(), description: "Cadence in RPM".to_string(), required: true},
                ],
            example: json!({
            "type": "set_cadence",
            "rpm": 42
        }),
    },
            ActionDefinition {
                name: "simulate_ride".to_string(),
                description: "Simulate cycling ride with varying speed/cadence".to_string(),
                parameters: vec![
                    Parameter { name: "profile".to_string(), type_hint: "string".to_string(), description: "Ride profile (flat, hill, interval)".to_string(), required: true},
                ],
            example: json!({
            "type": "simulate_ride",
            "profile": "example_profile"
        }),
    },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> { vec![] }
    fn protocol_name(&self) -> &'static str { "BLUETOOTH_BLE_CYCLING" }
    fn get_event_types(&self) -> Vec<EventType> { vec![CYCLING_MEASUREMENT_EVENT.clone()] }
    fn stack_name(&self) -> &'static str { "DATALINK" }
    fn keywords(&self) -> Vec<&'static str> { vec!["bluetooth", "ble", "cycling", "bike", "fitness"] }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
        ProtocolMetadataV2::builder().state(DevelopmentState::Experimental)
            .implementation("BLE Cycling Speed and Cadence Service (0x1816)")
            .llm_control("Actions: set_speed, set_cadence, simulate_ride")
            .e2e_testing("Requires BLE device")
            .notes("Standard GATT Cycling Speed and Cadence Service")
            .build()
    }

    fn description(&self) -> &'static str { "BLE Cycling - speed and cadence monitoring" }
    fn example_prompt(&self) -> &'static str { "Simulate cycling at 25 km/h with 80 RPM cadence" }
    fn group_name(&self) -> &'static str { "Network" }
}

impl Server for BluetoothBleCyclingProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::bluetooth_ble_cycling::BluetoothBleCycling::spawn_with_llm_actions(
                "NetGet-Cycling".to_string(), ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id, ctx.instruction
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        match action_type {
            "set_speed" | "set_cadence" | "simulate_ride" => Ok(ActionResult::Custom { name: action_type.to_string(), data: action }),
            _ => Err(anyhow::anyhow!("Unknown cycling action: {}", action_type)),
        }
    }
}
