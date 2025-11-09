//! BLE HID Mouse protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Mouse connection event
pub static MOUSE_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "mouse_client_connected",
        "A device connected to the BLE mouse",
    )
    .with_parameters(vec![
        Parameter {
            name: "client_id".to_string(),
            type_hint: "number".to_string(),
            description: "Unique client connection ID".to_string(),
            required: true,
        },
    ])
});

/// Mouse disconnection event
pub static MOUSE_CLIENT_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "mouse_client_disconnected",
        "A device disconnected from the BLE mouse",
    )
    .with_parameters(vec![
        Parameter {
            name: "client_id".to_string(),
            type_hint: "number".to_string(),
            description: "Unique client connection ID".to_string(),
            required: true,
        },
    ])
});

/// BLE HID Mouse protocol handler
pub struct BluetoothBleMouseProtocol;

impl BluetoothBleMouseProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleMouseProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Mouse device name (default: NetGet-Mouse)".to_string(),
                required: false,
                example: json!("MyMouse"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            move_cursor_action(),
            click_action(),
            scroll_action(),
            drag_action(),
            send_to_client_action(),
            list_clients_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_MOUSE"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            MOUSE_CLIENT_CONNECTED_EVENT.clone(),
            MOUSE_CLIENT_DISCONNECTED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "mouse", "hid", "bluetooth_ble_mouse"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE HID over GATT mouse (builds on bluetooth-ble)")
            .llm_control("High-level mouse actions: move, click, scroll, drag, targeted messages")
            .e2e_testing("Requires BLE-capable device to pair and receive mouse events")
            .notes("HID over GATT mouse profile. Supports connection tracking and per-client messaging.")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE HID mouse - act as a Bluetooth mouse that devices can pair with"
    }

    fn example_prompt(&self) -> &'static str {
        "Act as a Bluetooth mouse. When a device connects, move the cursor in a circle and then click."
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBleMouseProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            let device_name = ctx.startup_params.as_ref().and_then(|p| p.get_optional_string("device_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("NetGet-Mouse")
                .to_string();

            // Get instruction from server instance
            let instruction = ctx.state.get_server(ctx.server_id).await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_mouse::BluetoothBleMouse::spawn_with_llm_actions(
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

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "move_cursor" | "click" | "scroll" | "drag" | "send_to_client" | "list_clients" => {
                Ok(ActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
            _ => Err(anyhow::anyhow!("Unknown mouse action: {}", action_type)),
        }
    }
}

fn move_cursor_action() -> ActionDefinition {
    ActionDefinition {
        name: "move_cursor".to_string(),
        description: "Move the mouse cursor by relative amounts".to_string(),
        parameters: vec![
            Parameter {
                name: "dx".to_string(),
                type_hint: "number".to_string(),
                description: "Horizontal movement (-127 to 127)".to_string(),
                required: true,
            },
            Parameter {
                name: "dy".to_string(),
                type_hint: "number".to_string(),
                description: "Vertical movement (-127 to 127)".to_string(),
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
            "type": "move_cursor",
            "dx": 42,
            "dy": 42
        }),
    }
}

fn click_action() -> ActionDefinition {
    ActionDefinition {
        name: "click".to_string(),
        description: "Click a mouse button".to_string(),
        parameters: vec![
            Parameter {
                name: "button".to_string(),
                type_hint: "string".to_string(),
                description: "Button to click: 'left', 'right', 'middle'".to_string(),
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
            "type": "click",
            "button": "example_button"
        }),
    }
}

fn scroll_action() -> ActionDefinition {
    ActionDefinition {
        name: "scroll".to_string(),
        description: "Scroll the mouse wheel".to_string(),
        parameters: vec![
            Parameter {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Scroll amount (-127 to 127, positive=up, negative=down)".to_string(),
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
            "type": "scroll",
            "amount": 42
        }),
    }
}

fn drag_action() -> ActionDefinition {
    ActionDefinition {
        name: "drag".to_string(),
        description: "Drag with a mouse button held down".to_string(),
        parameters: vec![
            Parameter {
                name: "button".to_string(),
                type_hint: "string".to_string(),
                description: "Button to hold: 'left', 'right', 'middle'".to_string(),
                required: true,
            },
            Parameter {
                name: "dx".to_string(),
                type_hint: "number".to_string(),
                description: "Horizontal movement while dragging".to_string(),
                required: true,
            },
            Parameter {
                name: "dy".to_string(),
                type_hint: "number".to_string(),
                description: "Vertical movement while dragging".to_string(),
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
            "type": "drag",
            "button": "example_button",
            "dx": 42,
            "dy": 42
        }),
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
                description: "Hex-encoded HID report (4 bytes: buttons, X, Y, wheel)".to_string(),
                required: true,
            },
        ],
    example: json!({
            "type": "send_to_client",
            "client_id": 42,
            "report": "example_report"
        }),
    }
}

fn list_clients_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_clients".to_string(),
        description: "List all connected mouse clients".to_string(),
        parameters: vec![],
    example: json!({
            "type": "list_clients"
        }),
    }
}
