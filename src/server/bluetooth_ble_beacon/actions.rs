//! BLE beacon (iBeacon / Eddystone) - INCOMPLETE: the base stack cannot set the manufacturer/service advertising data a beacon requires
//!
//! Profile wrapper over the `bluetooth-ble` base stack.
//!
//! The base server (`BluetoothBle::spawn_with_llm_actions`) hardcodes `BluetoothBleProtocol`
//! when it calls `call_llm`, so the events the model actually sees and the actions it may
//! answer with are always the base's. Declaring profile-specific actions or events here would
//! document a vocabulary that no code path can ever emit or execute, so this protocol forwards
//! the base's set verbatim - the same shape `doh`/`dot` use to forward `DnsProtocol`'s actions.
//! The profile identity lives in the instruction preamble and in the GATT layout suggested by
//! the startup examples below.
//!
//! A beacon is defined entirely by its advertising payload: iBeacon needs Apple manufacturer-
//! specific data and Eddystone needs 0xFEAA service data. The base stack's start_advertising
//! only accepts a device name and a list of service UUIDs, and ble-peripheral-rust 0.2 exposes
//! no way to set advertising payload bytes at all. Nothing this protocol can emit is
//! recognisable to a beacon scanner, so it is marked Incomplete and hidden from the LLM rather
//! than advertised as working. The iBeacon and Eddystone frame builders in mod.rs are correct
//! but have nothing to feed.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::server::bluetooth_ble::actions::BluetoothBleProtocol;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::json;

/// BLE beacon (iBeacon / Eddystone) - INCOMPLETE: the base stack cannot set the manufacturer/service advertising data a beacon requires
pub struct BluetoothBleBeaconProtocol;

impl BluetoothBleBeaconProtocol {
    pub fn new() -> Self {
        Self
    }

    /// The base protocol this profile delegates its whole vocabulary to.
    fn base() -> BluetoothBleProtocol {
        BluetoothBleProtocol::new()
    }
}

impl Default for BluetoothBleBeaconProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Protocol for BluetoothBleBeaconProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Bluetooth device name to advertise (default: NetGet-Beacon)".to_string(),
            required: false,
            example: json!("NetGet-Beacon"),
        }]
    }

    /// Delegated: the base stack owns every action this server can execute.
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition> {
        Self::base().get_async_actions(state)
    }

    /// Delegated: see `get_async_actions`. Returning `vec![]` here while the base emits
    /// events would leave the model with no way to answer a read or a write.
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        Self::base().get_sync_actions()
    }

    /// Delegated: the base's event types are the only ones ever emitted for this server, and
    /// they carry their own `.with_actions(...)` lists, which is what `call_llm` offers the
    /// model. An event id declared here but not emitted by the base would silently match an
    /// `event_handlers` pattern that can never fire.
    fn get_event_types(&self) -> Vec<EventType> {
        Self::base().get_event_types()
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BEACON"
    }

    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_BEACON"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "beacon", "ibeacon", "eddystone"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .implementation(
                "bluetooth-ble base stack (ble-peripheral-rust) plus an instruction preamble for the beacon (advertising-only) role",
            )
            .llm_control(
                "None that is meaningful: the base GATT actions cannot produce a beacon advertisement, so the protocol is hidden from the LLM.",
            )
            .e2e_testing(
                "Requires a real Bluetooth LE adapter and a central such as nRF Connect; no automated coverage",
            )
            .notes(
                "A beacon is defined entirely by its advertising payload: iBeacon needs Apple manufacturer-specific data and Eddystone needs 0xFEAA service data. The base stack's start_advertising only accepts a device name and a list of service UUIDs, and ble-peripheral-rust 0.2 exposes no way to set advertising payload bytes at all. Nothing this protocol can emit is recognisable to a beacon scanner, so it is marked Incomplete and hidden from the LLM rather than advertised as working. The iBeacon and Eddystone frame builders in mod.rs are correct but have nothing to feed.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE beacon (iBeacon / Eddystone) - INCOMPLETE: the base stack cannot set the manufacturer/service advertising data a beacon requires"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as an iBeacon with UUID 12345678-1234-5678-1234-567812345678, major 1, minor 100 - and say plainly that this build cannot emit iBeacon advertising data"
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        // Every event id and action name below is one the base stack really emits and really
        // executes. UUIDs are written in full 128-bit form because the base parses them with
        // `Uuid::parse_str`, which rejects the 16-bit shorthand.
        StartupExamples::new(
            // LLM mode: the model builds the GATT layout and answers reads itself.
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "instruction": "Act as an iBeacon with UUID 12345678-1234-5678-1234-567812345678, major 1, minor 100 - and say plainly that this build cannot emit iBeacon advertising data",
                "startup_params": {
                    "device_name": "NetGet-Beacon"
                }
            }),
            // Script mode: a read is answered in-process, with no model call.
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "startup_params": {
                    "device_name": "NetGet-Beacon"
                },
                "event_handlers": [
                    {
                        "event_pattern": "bluetooth_read_request",
                        "handler": {
                            "type": "script",
                            "language": "python",
                            "code": "actions = [{'type': 'respond_to_read', 'value': '64'}]"
                        }
                    }
                ]
            }),
            // Static mode: fixed GATT layout and a fixed read response, with no model call.
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-beacon",
                "startup_params": {
                    "device_name": "NetGet-Beacon"
                },
                "event_handlers": [
                    {
                        "event_pattern": "bluetooth_ble_started",
                        "handler": {
                            "type": "static",
                            "actions": [
                                {
                                    "type": "add_service",
                                    "uuid": "12345678-1234-5678-1234-567812345678",
                                    "primary": true,
                                    "characteristics": [
                                        {
                                            "uuid": "00002a19-0000-1000-8000-00805f9b34fb",
                                            "properties": [
                                                "read"
                                            ],
                                            "permissions": [
                                                "readable"
                                            ],
                                            "initial_value": "64"
                                        }
                                    ]
                                },
                                {
                                    "type": "start_advertising",
                                    "device_name": "NetGet-Beacon",
                                    "service_uuids": [
                                        "12345678-1234-5678-1234-567812345678"
                                    ]
                                }
                            ]
                        }
                    },
                    {
                        "event_pattern": "bluetooth_read_request",
                        "handler": {
                            "type": "static",
                            "actions": [
                                {
                                    "type": "respond_to_read",
                                    "value": "64"
                                }
                            ]
                        }
                    }
                ]
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
            let device_name = ctx
                .startup_params
                .as_ref()
                .map(|p| p.get_optional_string("device_name"))
                .transpose()?
                .flatten()
                .unwrap_or_else(|| "NetGet-Beacon".to_string());

            // The user's own instruction must reach the base stack; the profile preamble is
            // added there, not substituted for it.
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_beacon::BluetoothBleBeacon::spawn_with_llm_actions(
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

    /// Delegated: the base's executor is what actually runs, so validation must accept exactly
    /// the base's action names and reject everything else rather than waving any action through.
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        Self::base().execute_action(action)
    }
}
