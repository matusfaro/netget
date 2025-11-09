//! BLE HID Keyboard protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Keyboard connection event
pub static KEYBOARD_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "keyboard_client_connected",
        "A device connected to the BLE keyboard",
    )
    .with_parameters(vec![
        Parameter::new("client_id", "Unique client connection ID"),
    ])
});

/// Keyboard disconnection event
pub static KEYBOARD_CLIENT_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "keyboard_client_disconnected",
        "A device disconnected from the BLE keyboard",
    )
    .with_parameters(vec![
        Parameter::new("client_id", "Unique client connection ID"),
    ])
});

/// BLE HID Keyboard protocol handler
pub struct BluetoothBleKeyboardProtocol;

impl BluetoothBleKeyboardProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleKeyboardProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Keyboard device name (default: NetGet-Keyboard)".to_string(),
                required: false,
                example: json!("MyKeyboard"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            type_text_action(),
            press_key_action(),
            key_combo_action(),
            send_to_client_action(),
            list_clients_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_KEYBOARD"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            KEYBOARD_CLIENT_CONNECTED_EVENT.clone(),
            KEYBOARD_CLIENT_DISCONNECTED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "keyboard", "hid", "bluetooth_ble_keyboard"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE HID over GATT keyboard (builds on bluetooth-ble)")
            .llm_control("High-level keyboard actions: type text, key combinations, targeted messages")
            .e2e_testing("Requires BLE-capable device to pair and receive keypresses")
            .notes("HID over GATT keyboard profile. Supports connection tracking and per-client messaging.")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE HID keyboard - act as a Bluetooth keyboard that devices can pair with"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as a Bluetooth keyboard. When a device connects, type 'Hello from NetGet!' and then wait for further instructions."
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleKeyboardProtocol {
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
                .unwrap_or("NetGet-Keyboard")
                .to_string();

            crate::server::bluetooth_ble_keyboard::BluetoothBleKeyboard::spawn_with_llm_actions(
                device_name,
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
            "type_text" | "press_key" | "key_combo" | "send_to_client" | "list_clients" => {
                Ok(ActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
            _ => Err(anyhow::anyhow!("Unknown keyboard action: {}", action_type)),
        }
    }
}

fn type_text_action() -> ActionDefinition {
    ActionDefinition {
        name: "type_text".to_string(),
        description: "Type a string of text on all connected devices or a specific client".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "text".to_string(),
                type_hint: "string".to_string(),
                description: "Text to type".to_string(),
                required: true,
                example: json!("Hello, World!"),
            },
            ParameterDefinition {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client ID only".to_string(),
                required: false,
                example: json!(1),
            },
        ],
    }
}

fn press_key_action() -> ActionDefinition {
    ActionDefinition {
        name: "press_key".to_string(),
        description: "Press a single key (e.g., 'enter', 'escape', 'tab')".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Key name to press".to_string(),
                required: true,
                example: json!("enter"),
            },
            ParameterDefinition {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client only".to_string(),
                required: false,
                example: json!(1),
            },
        ],
    }
}

fn key_combo_action() -> ActionDefinition {
    ActionDefinition {
        name: "key_combo".to_string(),
        description: "Press a key combination (e.g., Ctrl+C, Alt+Tab)".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "modifiers".to_string(),
                type_hint: "array".to_string(),
                description: "Modifier keys: 'ctrl', 'shift', 'alt', 'gui' (Windows key)".to_string(),
                required: false,
                example: json!(["ctrl"]),
            },
            ParameterDefinition {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Main key to press with modifiers".to_string(),
                required: true,
                example: json!("c"),
            },
            ParameterDefinition {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client only".to_string(),
                required: false,
                example: json!(1),
            },
        ],
    }
}

fn send_to_client_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_client".to_string(),
        description: "Send raw HID report to a specific client".to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Client ID to send to".to_string(),
                required: true,
                example: json!(1),
            },
            ParameterDefinition {
                name: "report".to_string(),
                type_hint: "string".to_string(),
                description: "Hex-encoded HID report (8 bytes)".to_string(),
                required: true,
                example: json!("0000040000000000"),
            },
        ],
    }
}

fn list_clients_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_clients".to_string(),
        description: "List all connected keyboard clients".to_string(),
        parameters: vec![],
    }
}
