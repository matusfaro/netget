//! BLE Cycling Speed and Cadence Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static CYCLING_MEASUREMENT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("cycling_measurement", "Cycling speed/cadence updated", json!({"type": "placeholder", "event_id": "cycling_measurement"})).with_parameters(vec![
        Parameter {
            name: "speed_kmh".to_string(),
            type_hint: "number".to_string(),
            description: "Speed in km/h".to_string(),
            required: true,
        },
        Parameter {
            name: "cadence_rpm".to_string(),
            type_hint: "number".to_string(),
            description: "Cadence in RPM".to_string(),
            required: true,
        },
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("BLE cycling: {speed_kmh} km/h, {cadence_rpm} RPM")
            .with_debug("BLE cycling measurement: speed={speed_kmh} km/h, cadence={cadence_rpm} RPM")
            .with_trace("BLE cycling event: {json_pretty(.)}"),
    )
});

pub struct BluetoothBleCyclingProtocol;
impl BluetoothBleCyclingProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleCyclingProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Cycling device name for advertising (default: NetGet-Cycling)".to_string(),
                required: false,
                example: json!("MyBike"),
            },
        ]
    }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_speed".to_string(),
                description: "Set cycling speed".to_string(),
                parameters: vec![Parameter {
                    name: "kmh".to_string(),
                    type_hint: "number".to_string(),
                    description: "Speed in km/h".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_speed",
                    "kmh": 42
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BLE cycling speed: {kmh} km/h")
                        .with_debug("BLE cycling set_speed: kmh={kmh}"),
                ),
            },
            ActionDefinition {
                name: "set_cadence".to_string(),
                description: "Set pedaling cadence".to_string(),
                parameters: vec![Parameter {
                    name: "rpm".to_string(),
                    type_hint: "number".to_string(),
                    description: "Cadence in RPM".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_cadence",
                    "rpm": 42
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BLE cycling cadence: {rpm} RPM")
                        .with_debug("BLE cycling set_cadence: rpm={rpm}"),
                ),
            },
            ActionDefinition {
                name: "simulate_ride".to_string(),
                description: "Simulate cycling ride with varying speed/cadence".to_string(),
                parameters: vec![Parameter {
                    name: "profile".to_string(),
                    type_hint: "string".to_string(),
                    description: "Ride profile (flat, hill, interval)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "simulate_ride",
                    "profile": "example_profile"
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BLE cycling simulate: {profile}")
                        .with_debug("BLE cycling simulate_ride: profile={profile}"),
                ),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_CYCLING"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![CYCLING_MEASUREMENT_EVENT.clone()]
    }
    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_CYCLING"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "cycling", "bike", "fitness"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Cycling Speed and Cadence Service (0x1816)")
            .llm_control("Actions: set_speed, set_cadence, simulate_ride")
            .e2e_testing("Requires BLE device")
            .notes("Standard GATT Cycling Speed and Cadence Service")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE Cycling - speed and cadence monitoring"
    }
    fn example_prompt(&self) -> &'static str {
        "Simulate cycling at 25 km/h with 80 RPM cadence"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles BLE cycling sensor
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-cycling",
                "instruction": "Simulate cycling at 25 km/h with 80 RPM cadence",
                "startup_params": {
                    "device_name": "NetGet-Cycling"
                }
            }),
            // Script mode: Code-based cycling handling
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-cycling",
                "startup_params": {
                    "device_name": "NetGet-Cycling"
                },
                "event_handlers": [{
                    "event_pattern": "cycling_measurement",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<cycling_handler>"
                    }
                }]
            }),
            // Static mode: Fixed cycling measurement
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-cycling",
                "startup_params": {
                    "device_name": "NetGet-Cycling"
                },
                "event_handlers": [{
                    "event_pattern": "cycling_measurement",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "set_speed",
                            "kmh": 25
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleCyclingProtocol {
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
            crate::server::bluetooth_ble_cycling::BluetoothBleCycling::spawn_with_llm_actions(
                "NetGet-Cycling".to_string(),
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
        match action_type {
            "set_speed" | "set_cadence" | "simulate_ride" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!("Unknown cycling action: {}", action_type)),
        }
    }
}
