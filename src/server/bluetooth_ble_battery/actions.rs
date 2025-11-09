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
    EventType::new(
        "battery_level_changed",
        "Battery level was updated",
    )
    .with_parameters(vec![
        Parameter::new("level", "Battery level percentage (0-100)"),
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
        vec![
            BATTERY_LEVEL_CHANGED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "battery", "bluetooth_ble_battery"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Battery Service (builds on bluetooth-ble)")
            .llm_control("Battery actions: set_battery_level, simulate_drain, simulate_charge")
            .e2e_testing("Requires BLE-capable device to read battery level")
            .notes("Standard GATT Battery Service (0x180F) with single Battery Level characteristic.")
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
}

impl Server for BluetoothBleBatteryProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            let device_name = ctx.params
                .get("device_name")
                .and_then(|v| v.as_str())
                .unwrap_or("NetGet-Battery")
                .to_string();

            let initial_level = ctx.params
                .get("initial_level")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as u8;

            crate::server::bluetooth_ble_battery::BluetoothBleBattery::spawn_with_llm_actions(
                device_name,
                initial_level,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.server_id,
                ctx.instruction,
            )
            .await
        })
    }

    fn execute_action(
        &self,
        _connection_id: Option<crate::server::connection::ConnectionId>,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
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
        parameters: vec![
            ParameterDefinition {
                name: "level".to_string(),
                type_hint: "number".to_string(),
                description: "Battery level (0-100)".to_string(),
                required: true,
                example: json!(75),
            },
        ],
    }
}

fn simulate_drain_action() -> ActionDefinition {
    ActionDefinition {
        name: "simulate_drain".to_string(),
        description: "Gradually decrease battery level".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Amount to drain (percentage points)".to_string(),
                required: true,
                example: json!(10),
            },
            ParameterDefinition {
                name: "interval_ms".to_string(),
                type_hint: "number".to_string(),
                description: "Interval between updates (milliseconds)".to_string(),
                required: false,
                example: json!(5000),
            },
        ],
    }
}

fn simulate_charge_action() -> ActionDefinition {
    ActionDefinition {
        name: "simulate_charge".to_string(),
        description: "Gradually increase battery level".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Amount to charge (percentage points)".to_string(),
                required: true,
                example: json!(20),
            },
            ParameterDefinition {
                name: "interval_ms".to_string(),
                type_hint: "number".to_string(),
                description: "Interval between updates (milliseconds)".to_string(),
                required: false,
                example: json!(2000),
            },
        ],
    }
}
