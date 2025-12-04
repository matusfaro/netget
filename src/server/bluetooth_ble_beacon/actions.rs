//! BLE Beacon protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Beacon started event
pub static BEACON_STARTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("beacon_started", "BLE beacon advertising started", json!({"type": "placeholder", "event_id": "beacon_started"})).with_parameters(vec![
        Parameter {
            name: "beacon_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of beacon (ibeacon, eddystone-uid, eddystone-url, eddystone-tlm)"
                .to_string(),
            required: true,
        },
    ])
});

/// Beacon stopped event
pub static BEACON_STOPPED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("beacon_stopped", "BLE beacon advertising stopped", json!({"type": "placeholder", "event_id": "beacon_stopped"})).with_parameters(vec![])
});

/// BLE Beacon protocol handler
pub struct BluetoothBleBeaconProtocol;

impl BluetoothBleBeaconProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleBeaconProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            advertise_ibeacon_action(),
            advertise_eddystone_uid_action(),
            advertise_eddystone_url_action(),
            advertise_eddystone_tlm_action(),
            stop_beacon_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BEACON"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![BEACON_STARTED_EVENT.clone(), BEACON_STOPPED_EVENT.clone()]
    }

    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BEACON"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "bluetooth",
            "beacon",
            "ibeacon",
            "eddystone",
            "bluetooth_ble_beacon",
        ]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE beacon advertising (builds on bluetooth-ble)")
            .llm_control("Beacon actions: iBeacon, Eddystone-UID, Eddystone-URL, Eddystone-TLM")
            .e2e_testing("Requires BLE-capable device to scan for beacons")
            .notes("Advertisement-only protocol. Supports iBeacon (Apple) and Eddystone (Google) formats.")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE beacon - broadcast proximity/location data (iBeacon, Eddystone)"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as an iBeacon with UUID 12345678-1234-5678-1234-567812345678, major 1, minor 100"
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles beacon broadcasting
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "instruction": "Act as an iBeacon with UUID 12345678-1234-5678-1234-567812345678, major 1, minor 100. Broadcast proximity data."
            }),
            // Script mode: Code-based beacon control
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "event_handlers": [{
                    "event_pattern": "beacon_started",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<beacon_handler>"
                    }
                }]
            }),
            // Static mode: Fixed beacon configuration
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "event_handlers": [{
                    "event_pattern": "beacon_started",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "advertise_ibeacon",
                            "uuid": "12345678-1234-5678-1234-567812345678",
                            "major": 1,
                            "minor": 100,
                            "tx_power": -59
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleBeaconProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            // Get instruction from server instance
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_beacon::BluetoothBleBeacon::spawn_with_llm_actions(
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
            "advertise_ibeacon"
            | "advertise_eddystone_uid"
            | "advertise_eddystone_url"
            | "advertise_eddystone_tlm"
            | "stop_beacon" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!("Unknown beacon action: {}", action_type)),
        }
    }
}

fn advertise_ibeacon_action() -> ActionDefinition {
    ActionDefinition {
        name: "advertise_ibeacon".to_string(),
        description: "Start advertising as an iBeacon (Apple standard)".to_string(),
        parameters: vec![
            Parameter {
                name: "uuid".to_string(),
                type_hint: "string".to_string(),
                description: "128-bit UUID (e.g., '12345678-1234-5678-1234-567812345678')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "major".to_string(),
                type_hint: "number".to_string(),
                description: "16-bit major identifier (0-65535, e.g., store ID)".to_string(),
                required: true,
            },
            Parameter {
                name: "minor".to_string(),
                type_hint: "number".to_string(),
                description: "16-bit minor identifier (0-65535, e.g., department ID)".to_string(),
                required: true,
            },
            Parameter {
                name: "tx_power".to_string(),
                type_hint: "number".to_string(),
                description: "Calibrated transmission power in dBm (default: -59)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "advertise_ibeacon",
            "uuid": "example_uuid",
            "major": 42,
            "minor": 42
        }),
        log_template: None,
    }
}

fn advertise_eddystone_uid_action() -> ActionDefinition {
    ActionDefinition {
        name: "advertise_eddystone_uid".to_string(),
        description: "Start advertising as Eddystone-UID (unique beacon ID)".to_string(),
        parameters: vec![
            Parameter {
                name: "namespace".to_string(),
                type_hint: "string".to_string(),
                description: "10-byte namespace ID (hex, e.g., '0123456789abcdef0123')".to_string(),
                required: true,
            },
            Parameter {
                name: "instance".to_string(),
                type_hint: "string".to_string(),
                description: "6-byte instance ID (hex, e.g., '0123456789ab')".to_string(),
                required: true,
            },
            Parameter {
                name: "tx_power".to_string(),
                type_hint: "number".to_string(),
                description: "Calibrated transmission power in dBm (default: -20)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "advertise_eddystone_uid",
            "namespace": "example_namespace",
            "instance": "example_instance"
        }),
        log_template: None,
    }
}

fn advertise_eddystone_url_action() -> ActionDefinition {
    ActionDefinition {
        name: "advertise_eddystone_url".to_string(),
        description: "Start advertising as Eddystone-URL (broadcast a URL)".to_string(),
        parameters: vec![
            Parameter {
                name: "url".to_string(),
                type_hint: "string".to_string(),
                description: "URL to broadcast (max ~17 chars after compression)".to_string(),
                required: true,
            },
            Parameter {
                name: "tx_power".to_string(),
                type_hint: "number".to_string(),
                description: "Calibrated transmission power in dBm (default: -20)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "advertise_eddystone_url",
            "url": "example_url"
        }),
        log_template: None,
    }
}

fn advertise_eddystone_tlm_action() -> ActionDefinition {
    ActionDefinition {
        name: "advertise_eddystone_tlm".to_string(),
        description: "Start advertising as Eddystone-TLM (telemetry data)".to_string(),
        parameters: vec![
            Parameter {
                name: "battery_voltage".to_string(),
                type_hint: "number".to_string(),
                description: "Battery voltage in mV (0-65535)".to_string(),
                required: false,
            },
            Parameter {
                name: "temperature".to_string(),
                type_hint: "number".to_string(),
                description: "Temperature in Celsius".to_string(),
                required: false,
            },
            Parameter {
                name: "adv_count".to_string(),
                type_hint: "number".to_string(),
                description: "Advertisement count since boot".to_string(),
                required: false,
            },
            Parameter {
                name: "uptime".to_string(),
                type_hint: "number".to_string(),
                description: "Uptime in seconds since boot".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "advertise_eddystone_tlm"
        }),
        log_template: None,
    }
}

fn stop_beacon_action() -> ActionDefinition {
    ActionDefinition {
        name: "stop_beacon".to_string(),
        description: "Stop beacon advertising".to_string(),
        parameters: vec![],
        example: json!({
            "type": "stop_beacon"
        }),
        log_template: None,
    }
}
