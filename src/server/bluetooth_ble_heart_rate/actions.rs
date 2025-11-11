//! BLE Heart Rate Service protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static HEART_RATE_UPDATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("heart_rate_updated", "Heart rate BPM was updated").with_parameters(vec![
        Parameter {
            name: "bpm".to_string(),
            type_hint: "number".to_string(),
            description: "Beats per minute".to_string(),
            required: true,
        },
    ])
});

pub struct BluetoothBleHeartRateProtocol;

impl BluetoothBleHeartRateProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleHeartRateProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Device name (default: NetGet-HeartRate)".to_string(),
            required: false,
            example: json!("MyHeartRate"),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_bpm".to_string(),
                description: "Set heart rate in beats per minute".to_string(),
                parameters: vec![Parameter {
                    name: "bpm".to_string(),
                    type_hint: "number".to_string(),
                    description: "Beats per minute (30-220)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_bpm",
                    "bpm": 42
                }),
            },
            ActionDefinition {
                name: "simulate_activity".to_string(),
                description: "Simulate physical activity with varying heart rate".to_string(),
                parameters: vec![Parameter {
                    name: "activity".to_string(),
                    type_hint: "string".to_string(),
                    description: "Activity type (rest, walk, jog, run, sprint)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "simulate_activity",
                    "activity": "example_activity"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_HEART_RATE"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![HEART_RATE_UPDATED_EVENT.clone()]
    }
    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "heart", "rate", "bluetooth_ble_heart_rate"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Heart Rate Service (0x180D)")
            .llm_control("Actions: set_bpm, simulate_activity")
            .e2e_testing("Requires BLE device")
            .notes("Standard GATT Heart Rate Service")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE Heart Rate Service - monitor heart rate (BPM)"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a heart rate monitor. Start at 60 BPM, increase to 120 during exercise."
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleHeartRateProtocol {
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
            let device_name = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("device_name"))
                .as_deref()
                .unwrap_or("NetGet-HeartRate")
                .to_string();
            crate::server::bluetooth_ble_heart_rate::BluetoothBleHeartRate::spawn_with_llm_actions(
                device_name,
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
            "set_bpm" | "simulate_activity" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!(
                "Unknown heart rate action: {}",
                action_type
            )),
        }
    }
}
