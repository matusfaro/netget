//! BLE Running Speed and Cadence Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub static RUNNING_MEASUREMENT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("running_measurement", "Running speed/cadence updated").with_parameters(vec![
        Parameter {
            name: "pace_min_km".to_string(),
            type_hint: "number".to_string(),
            description: "Pace in min/km".to_string(),
            required: true,
        },
        Parameter {
            name: "cadence_spm".to_string(),
            type_hint: "number".to_string(),
            description: "Cadence in steps/min".to_string(),
            required: true,
        },
    ])
});

pub struct BluetoothBleRunningProtocol;
impl BluetoothBleRunningProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleRunningProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![]
    }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_pace".to_string(),
                description: "Set running pace".to_string(),
                parameters: vec![Parameter {
                    name: "min_per_km".to_string(),
                    type_hint: "number".to_string(),
                    description: "Pace in min/km".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_pace",
                    "min_per_km": 42
                }),
            },
            ActionDefinition {
                name: "set_cadence".to_string(),
                description: "Set running cadence".to_string(),
                parameters: vec![Parameter {
                    name: "spm".to_string(),
                    type_hint: "number".to_string(),
                    description: "Steps per minute".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_cadence",
                    "spm": 42
                }),
            },
            ActionDefinition {
                name: "simulate_run".to_string(),
                description: "Simulate running activity".to_string(),
                parameters: vec![Parameter {
                    name: "profile".to_string(),
                    type_hint: "string".to_string(),
                    description: "Run profile (easy, tempo, interval, sprint)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "simulate_run",
                    "profile": "example_profile"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_RUNNING"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![RUNNING_MEASUREMENT_EVENT.clone()]
    }
    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_RUNNING"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "running", "jogging", "fitness"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Running Speed and Cadence Service (0x1814)")
            .llm_control("Actions: set_pace, set_cadence, simulate_run")
            .e2e_testing("Requires BLE device")
            .notes("Standard GATT Running Speed and Cadence Service")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE Running - pace and cadence monitoring"
    }
    fn example_prompt(&self) -> &'static str {
        "Simulate running at 5:30 min/km pace with 180 SPM"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleRunningProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            // Get instruction from server state
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction.clone())
                .unwrap_or_default();

            crate::server::bluetooth_ble_running::BluetoothBleRunning::spawn_with_llm_actions(
                "NetGet-Running".to_string(),
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
            "set_pace" | "set_cadence" | "simulate_run" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!("Unknown running action: {}", action_type)),
        }
    }
}
