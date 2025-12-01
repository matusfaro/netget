//! BLE Proximity Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

pub struct BluetoothBleProximityProtocol;
impl BluetoothBleProximityProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleProximityProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Proximity sensor device name for advertising (default: NetGet-Proximity)".to_string(),
                required: false,
                example: json!("NetGet-Proximity"),
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
        "BLUETOOTH_BLE_PROXIMITY"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![]
    }
    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_PROXIMITY"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "proximity"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Proximity")
            .llm_control("Proximity actions")
            .e2e_testing("Requires BLE device")
            .notes("BLE Proximity")
            .build()
    }
    fn description(&self) -> &'static str {
        "BLE Proximity"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a BLE proximity device"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles BLE proximity device
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-proximity",
                "instruction": "Act as a BLE proximity sensor that detects and reports nearby devices",
                "startup_params": {
                    "device_name": "NetGet-Proximity"
                }
            }),
            // Script mode: Code-based proximity handling
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-proximity",
                "startup_params": {
                    "device_name": "NetGet-Proximity"
                },
                "event_handlers": [{
                    "event_pattern": "ble_proximity_detected",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<proximity_handler>"
                    }
                }]
            }),
            // Static mode: Fixed proximity action
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-proximity",
                "startup_params": {
                    "device_name": "NetGet-Proximity"
                },
                "event_handlers": [{
                    "event_pattern": "ble_proximity_detected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "wait_for_more"
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleProximityProtocol {
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
            crate::server::bluetooth_ble_proximity::BluetoothBleProximity::spawn_with_llm_actions(
                "NetGet-Proximity".to_string(),
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
