//! Bluetooth Low Energy (BLE) GATT server protocol actions
//!
//! Cross-platform BLE peripheral using ble-peripheral-rust

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Bluetooth server started event
pub static BLUETOOTH_BLE_STARTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bluetooth_ble_started",
        "Bluetooth Low Energy GATT server started and ready for configuration",
    )
    .with_parameters(vec![
        Parameter {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Name of the BLE device for advertising".to_string(),
            required: true,
        },
        Parameter {
            name: "instruction".to_string(),
            type_hint: "string".to_string(),
            description: "User instruction for server behavior".to_string(),
            required: true,
        },
    ])
});

/// Bluetooth adapter state changed event
pub static BLUETOOTH_STATE_CHANGED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bluetooth_state_changed",
        "Bluetooth adapter state changed (powered on/off, advertising started/stopped, etc.)",
    )
    .with_parameters(vec![Parameter {
        name: "state".to_string(),
        type_hint: "string".to_string(),
        description: "Current state description".to_string(),
        required: true,
    }])
});

/// Read request event
pub static BLUETOOTH_READ_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bluetooth_read_request",
        "Client is reading from a GATT characteristic - respond with data",
    )
    .with_parameters(vec![
        Parameter {
            name: "characteristic_uuid".to_string(),
            type_hint: "string".to_string(),
            description: "UUID of the characteristic being read".to_string(),
            required: true,
        },
        Parameter {
            name: "offset".to_string(),
            type_hint: "number".to_string(),
            description: "Byte offset for long reads (usually 0)".to_string(),
            required: true,
        },
    ])
});

/// Write request event
pub static BLUETOOTH_WRITE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bluetooth_write_request",
        "Client wrote data to a GATT characteristic",
    )
    .with_parameters(vec![
        Parameter {
            name: "characteristic_uuid".to_string(),
            type_hint: "string".to_string(),
            description: "UUID of the characteristic written to".to_string(),
            required: true,
        },
        Parameter {
            name: "value".to_string(),
            type_hint: "string".to_string(),
            description: "Hex-encoded data written by client".to_string(),
            required: true,
        },
        Parameter {
            name: "offset".to_string(),
            type_hint: "number".to_string(),
            description: "Byte offset for long writes (usually 0)".to_string(),
            required: true,
        },
        Parameter {
            name: "with_response".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether client expects a response".to_string(),
            required: true,
        },
    ])
});

/// Subscribe/unsubscribe to notifications event
pub static BLUETOOTH_SUBSCRIBE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bluetooth_subscribe",
        "Client subscribed or unsubscribed from characteristic notifications",
    )
    .with_parameters(vec![
        Parameter {
            name: "characteristic_uuid".to_string(),
            type_hint: "string".to_string(),
            description: "UUID of the characteristic".to_string(),
            required: true,
        },
        Parameter {
            name: "subscribed".to_string(),
            type_hint: "boolean".to_string(),
            description: "true if subscribed, false if unsubscribed".to_string(),
            required: true,
        },
    ])
});

/// Bluetooth server protocol handler
pub struct BluetoothBleProtocol;

impl BluetoothBleProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for BluetoothBleProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Bluetooth device name for advertising (default: NetGet-BLE)"
                    .to_string(),
                required: false,
                example: json!("MyDevice"),
            },
            ParameterDefinition {
                name: "auto_advertise".to_string(),
                type_hint: "boolean".to_string(),
                description: "Start advertising immediately after server starts (default: true)"
                    .to_string(),
                required: false,
                example: json!(true),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            add_service_action(),
            start_advertising_action(),
            stop_advertising_action(),
            send_notification_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![respond_to_read_action(), respond_to_write_action()]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            BLUETOOTH_BLE_STARTED_EVENT.clone(),
            BLUETOOTH_STATE_CHANGED_EVENT.clone(),
            BLUETOOTH_READ_REQUEST_EVENT.clone(),
            BLUETOOTH_WRITE_REQUEST_EVENT.clone(),
            BLUETOOTH_SUBSCRIBE_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "DATALINK>BLE"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "gatt", "peripheral", "bluetooth_ble"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("ble-peripheral-rust (cross-platform: Windows/WinRT, macOS/CoreBluetooth, Linux/BlueZ)")
            .llm_control("Full GATT server control: services, characteristics, read/write/notify")
            .e2e_testing("Real BLE hardware or simulator required")
            .notes("Cross-platform BLE peripheral. LLM controls GATT services, advertising, and responses.")
            .build()
    }

    fn description(&self) -> &'static str {
        "Bluetooth Low Energy (BLE) GATT server - act as a Bluetooth peripheral device"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as a BLE heart rate monitor. Create Heart Rate Service (0x180D) with Measurement characteristic (0x2A37). Start at 72 BPM, increase by 1 every 2 seconds, send notifications."
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for BluetoothBleProtocol {
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
                .map(|s| s.to_string())
                .unwrap_or_else(|| "NetGet-BLE".to_string());

            let instruction = "Act as a Bluetooth Low Energy GATT server".to_string();

            crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(
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
        // Actions are executed directly in the server event loop
        // This is just for validation
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "add_service" | "start_advertising" | "stop_advertising" | "send_notification"
            | "respond_to_read" | "respond_to_write" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!(
                "Unknown Bluetooth action type: {}",
                action_type
            )),
        }
    }
}

// Action definitions

fn add_service_action() -> ActionDefinition {
    ActionDefinition {
        name: "add_service".to_string(),
        description: "Add a GATT service with characteristics to the BLE server".to_string(),
        parameters: vec![
            Parameter {
                name: "uuid".to_string(),
                type_hint: "string".to_string(),
                description: "Service UUID (standard 16-bit like '180D' or full 128-bit UUID)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "primary".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether this is a primary service (default: true)".to_string(),
                required: false,
            },
            Parameter {
                name: "characteristics".to_string(),
                type_hint: "array".to_string(),
                description: "Array of characteristic definitions".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "add_service",
            "uuid": "180D",
            "primary": true,
            "characteristics": [{
                "uuid": "2A37",
                "properties": ["read", "notify"],
                "permissions": ["readable"],
                "initial_value": "0048"
            }]
        }),
    }
}

fn start_advertising_action() -> ActionDefinition {
    ActionDefinition {
        name: "start_advertising".to_string(),
        description: "Start BLE advertising to make the device discoverable".to_string(),
        parameters: vec![Parameter {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description:
                "Device name to advertise (optional, uses server default if not specified)"
                    .to_string(),
            required: false,
        }],
        example: json!({
            "type": "start_advertising"
        }),
    }
}

fn stop_advertising_action() -> ActionDefinition {
    ActionDefinition {
        name: "stop_advertising".to_string(),
        description: "Stop BLE advertising".to_string(),
        parameters: vec![],
        example: json!({
            "type": "stop_advertising"
        }),
    }
}

fn send_notification_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_notification".to_string(),
        description: "Send a notification to subscribed clients for a characteristic".to_string(),
        parameters: vec![
            Parameter {
                name: "characteristic_uuid".to_string(),
                type_hint: "string".to_string(),
                description: "UUID of the characteristic to update".to_string(),
                required: true,
            },
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "Hex-encoded value to send (e.g., '0048' for 72 in decimal)"
                    .to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_notification",
            "characteristic_uuid": "example_characteristic_uuid",
            "value": "example_value"
        }),
    }
}

fn respond_to_read_action() -> ActionDefinition {
    ActionDefinition {
        name: "respond_to_read".to_string(),
        description: "Respond to a client's read request with data (use in response to bluetooth_read_request event)".to_string(),
        parameters: vec![
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "Hex-encoded value to return to client".to_string(),
                required: true,
            },
        ],
    example: json!({
            "type": "respond_to_read",
            "value": "example_value"
        }),
    }
}

fn respond_to_write_action() -> ActionDefinition {
    ActionDefinition {
        name: "respond_to_write".to_string(),
        description: "Acknowledge a client's write request (use in response to bluetooth_write_request event)".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "string".to_string(),
                description: "Response status: 'success' or 'error' (default: success)".to_string(),
                required: false,
            },
        ],
    example: json!({
            "type": "respond_to_write"
        }),
    }
}
