//! BLE HID Keyboard protocol actions

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

/// Keyboard connection event
pub static KEYBOARD_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "keyboard_client_connected",
        "A device connected to the BLE keyboard",
        json!({
            "type": "type_text",
            "text": "Hello, World!"
        })
    )
    .with_parameters(vec![Parameter {
        name: "client_id".to_string(),
        type_hint: "number".to_string(),
        description: "Unique client connection ID".to_string(),
        required: true,
    }])
    .with_log_template(
        LogTemplate::new()
            .with_info("BLE keyboard client connected: {client_id}")
            .with_debug("BLE keyboard client {client_id} connected")
            .with_trace("BLE keyboard connected: {json_pretty(.)}"),
    )
});

/// Keyboard disconnection event
pub static KEYBOARD_CLIENT_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "keyboard_client_disconnected",
        "A device disconnected from the BLE keyboard",
        json!({
            "type": "list_clients"
        })
    )
    .with_parameters(vec![Parameter {
        name: "client_id".to_string(),
        type_hint: "number".to_string(),
        description: "Unique client connection ID".to_string(),
        required: true,
    }])
    .with_log_template(
        LogTemplate::new()
            .with_info("BLE keyboard client disconnected: {client_id}")
            .with_debug("BLE keyboard client {client_id} disconnected")
            .with_trace("BLE keyboard disconnected: {json_pretty(.)}"),
    )
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
        vec![ParameterDefinition {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Keyboard device name (default: NetGet-Keyboard)".to_string(),
            required: false,
            example: json!("MyKeyboard"),
        }]
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
        "BLUETOOTH_BLE_KEYBOARD"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "keyboard", "hid", "bluetooth_ble_keyboard"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

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

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles BLE keyboard interaction
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-keyboard",
                "instruction": "Act as a Bluetooth keyboard. When a device connects, type 'Hello from NetGet!' and wait for further instructions.",
                "startup_params": {
                    "device_name": "NetGet-Keyboard"
                }
            }),
            // Script mode: Code-based keyboard handling
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-keyboard",
                "startup_params": {
                    "device_name": "NetGet-Keyboard"
                },
                "event_handlers": [{
                    "event_pattern": "keyboard_client_connected",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<keyboard_handler>"
                    }
                }]
            }),
            // Static mode: Fixed keyboard actions on connect
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-keyboard",
                "startup_params": {
                    "device_name": "NetGet-Keyboard"
                },
                "event_handlers": [{
                    "event_pattern": "keyboard_client_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "type_text",
                            "text": "Hello from NetGet!"
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleKeyboardProtocol {
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
                .as_deref()
                .unwrap_or("NetGet-Keyboard")
                .to_string();

            // Get instruction from server instance
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_keyboard::BluetoothBleKeyboard::spawn_with_llm_actions(
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
        description: "Type a string of text on all connected devices or a specific client"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "text".to_string(),
                type_hint: "string".to_string(),
                description: "Text to type".to_string(),
                required: true,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client ID only".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "type_text",
            "text": "Hello, World!"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE keyboard type: \"{text}\"")
                .with_debug("BLE keyboard type_text: text=\"{text}\", client_id={client_id}"),
        ),
    }
}

fn press_key_action() -> ActionDefinition {
    ActionDefinition {
        name: "press_key".to_string(),
        description: "Press a single key (e.g., enter, escape, tab)".to_string(),
        parameters: vec![
            Parameter {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Key name to press".to_string(),
                required: true,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client only".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "press_key",
            "key": "enter"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE keyboard press: {key}")
                .with_debug("BLE keyboard press_key: key={key}, client_id={client_id}"),
        ),
    }
}

fn key_combo_action() -> ActionDefinition {
    ActionDefinition {
        name: "key_combo".to_string(),
        description: "Press a key combination (e.g., Ctrl+C, Alt+Tab)".to_string(),
        parameters: vec![
            Parameter {
                name: "modifiers".to_string(),
                type_hint: "array".to_string(),
                description: "Modifier keys: ctrl, shift, alt, gui (Windows key)".to_string(),
                required: false,
            },
            Parameter {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Main key to press with modifiers".to_string(),
                required: true,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Send to specific client only".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "key_combo",
            "modifiers": ["ctrl"],
            "key": "c"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE keyboard combo: {modifiers}+{key}")
                .with_debug("BLE keyboard key_combo: modifiers={modifiers}, key={key}"),
        ),
    }
}

fn send_to_client_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_client".to_string(),
        description: "Send raw HID report to a specific client".to_string(),
        parameters: vec![
            Parameter {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Client ID to send to".to_string(),
                required: true,
            },
            Parameter {
                name: "report".to_string(),
                type_hint: "string".to_string(),
                description: "Hex-encoded HID report (8 bytes)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_to_client",
            "client_id": 1,
            "report": "0000040000000000"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE keyboard HID to client {client_id}")
                .with_debug("BLE keyboard send_to_client: client_id={client_id}, report={report}"),
        ),
    }
}

fn list_clients_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_clients".to_string(),
        description: "List all connected keyboard clients".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_clients"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE keyboard list clients")
                .with_debug("BLE keyboard list_clients"),
        ),
    }
}
