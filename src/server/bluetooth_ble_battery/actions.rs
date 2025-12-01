//! BLE Battery Service protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Battery level changed event
pub static BATTERY_LEVEL_CHANGED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("battery_level_changed", "Battery level was updated", json!({"type": "placeholder", "event_id": "battery_level_changed"})).with_parameters(vec![
        Parameter {
            name: "level".to_string(),
            type_hint: "string".to_string(),
            description: "Battery level percentage (0-100)".to_string(),
            required: true,
        },
    ])
});

/// BLE Battery Service protocol handler
pub struct BluetoothBleBatteryProtocol;

impl BluetoothBleBatteryProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleBatteryProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Battery device name (default: NetGet-Battery)".to_string(),
                required: false,
                example: json!("MyBattery"),
            },
            ParameterDefinition {
                name: "initial_level".to_string(),
                type_hint: "number".to_string(),
                description: "Initial battery level (0-100, default: 100)".to_string(),
                required: false,
                example: json!(80),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            set_battery_level_action(),
            simulate_drain_action(),
            simulate_charge_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BATTERY"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![BATTERY_LEVEL_CHANGED_EVENT.clone()]
    }

    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BATTERY"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "battery", "bluetooth_ble_battery"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Battery Service (builds on bluetooth-ble)")
            .llm_control("Battery actions: set_battery_level, simulate_drain, simulate_charge")
            .e2e_testing("Requires BLE-capable device to read battery level")
            .notes(
                "Standard GATT Battery Service (0x180F) with single Battery Level characteristic.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE Battery Service - report battery level (0-100%)"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as a Bluetooth battery. Start at 100%, drain by 10% every 5 seconds."
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM controls battery level simulation
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-battery",
                "instruction": "Act as a Bluetooth battery. Start at 100%, drain by 10% every 5 seconds.",
                "startup_params": {
                    "device_name": "NetGet-Battery",
                    "initial_level": 100
                }
            }),
            // Script mode: Code-based battery simulation
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-battery",
                "startup_params": {
                    "device_name": "NetGet-Battery",
                    "initial_level": 100
                },
                "event_handlers": [{
                    "event_pattern": "battery_level_changed",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<battery_handler>"
                    }
                }]
            }),
            // Static mode: Fixed battery level
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-battery",
                "startup_params": {
                    "device_name": "NetGet-Battery",
                    "initial_level": 75
                },
                "event_handlers": [{
                    "event_pattern": "battery_level_changed",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "set_battery_level",
                            "level": 75
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleBatteryProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            let device_name = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("device_name"))
                .unwrap_or_else(|| "NetGet-Battery".to_string());

            let initial_level = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_u64("initial_level"))
                .unwrap_or(100) as u8;

            crate::server::bluetooth_ble_battery::BluetoothBleBattery::spawn_with_llm_actions(
                device_name,
                initial_level,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "set_battery_level" | "simulate_drain" | "simulate_charge" => {
                Ok(ActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
            _ => Err(anyhow::anyhow!("Unknown battery action: {}", action_type)),
        }
    }
}

fn set_battery_level_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_battery_level".to_string(),
        description: "Set battery level percentage".to_string(),
        parameters: vec![Parameter {
            name: "level".to_string(),
            type_hint: "number".to_string(),
            description: "Battery level (0-100)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "set_battery_level",
            "level": 75
        }),
    }
}

fn simulate_drain_action() -> ActionDefinition {
    ActionDefinition {
        name: "simulate_drain".to_string(),
        description: "Gradually decrease battery level".to_string(),
        parameters: vec![
            Parameter {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Amount to drain (percentage points)".to_string(),
                required: true,
            },
            Parameter {
                name: "interval_ms".to_string(),
                type_hint: "number".to_string(),
                description: "Interval between updates (milliseconds)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "simulate_drain",
            "amount": 10,
            "interval_ms": 5000
        }),
    }
}

fn simulate_charge_action() -> ActionDefinition {
    ActionDefinition {
        name: "simulate_charge".to_string(),
        description: "Gradually increase battery level".to_string(),
        parameters: vec![
            Parameter {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Amount to charge (percentage points)".to_string(),
                required: true,
            },
            Parameter {
                name: "interval_ms".to_string(),
                type_hint: "number".to_string(),
                description: "Interval between updates (milliseconds)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "simulate_charge",
            "amount": 20,
            "interval_ms": 2000
        }),
    }
}
