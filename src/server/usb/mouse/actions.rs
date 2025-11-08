//! USB HID Mouse protocol actions implementation

#[cfg(feature = "usb-mouse")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-mouse")]
use crate::protocol::EventType;
#[cfg(feature = "usb-mouse")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-mouse")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-mouse")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-mouse")]
use serde_json::json;
#[cfg(feature = "usb-mouse")]
use std::collections::HashMap;
#[cfg(feature = "usb-mouse")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "usb-mouse")]
use tokio::sync::Mutex;

// Event type definitions (static for efficient reuse)
#[cfg(feature = "usb-mouse")]
pub static USB_MOUSE_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_mouse_attached",
        "Host attached to USB mouse device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
    ])
});

#[cfg(feature = "usb-mouse")]
pub static USB_MOUSE_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_mouse_detached",
        "Host detached from USB mouse device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
    ])
});

/// USB HID Mouse protocol action handler
#[cfg(feature = "usb-mouse")]
pub struct UsbMouseProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

#[cfg(feature = "usb-mouse")]
pub struct ConnectionData {
    // Placeholder for mouse-specific connection data
}

#[cfg(feature = "usb-mouse")]
impl UsbMouseProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-mouse")]
impl Protocol for UsbMouseProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            move_relative_action(),
            move_absolute_action(),
            click_action(),
            scroll_action(),
            drag_action(),
            wait_for_more_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "USB-Mouse"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            USB_MOUSE_ATTACHED_EVENT.clone(),
            USB_MOUSE_DETACHED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "USB>HID>Mouse"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "mouse", "hid", "pointer", "cursor"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::new(
            crate::protocol::metadata::ProtocolState::Experimental,
            "Virtual USB HID mouse device using USB/IP protocol",
            "LLM controls mouse movement, clicks, and scrolling",
            "E2E tests using Linux usbip client",
            crate::protocol::metadata::PrivilegeRequirement::None,
        )
        .with_notes("Requires client to have vhci-hcd kernel module and run 'usbip attach'")
    }

    fn description(&self) -> &'static str {
        "Virtual USB HID mouse device"
    }

    fn example_prompt(&self) -> &'static str {
        "Create a USB mouse and move it in a circle when attached"
    }

    fn group_name(&self) -> &'static str {
        "USB Devices"
    }
}

// Implement Server trait
#[cfg(feature = "usb-mouse")]
impl Server for UsbMouseProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            crate::server::usb::mouse::UsbMouseServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        _connection_id: Option<ConnectionId>,
        _app_state: &AppState,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "move_relative" => {
                let x = action["x"].as_i64().context("move_relative requires 'x' field")? as i8;
                let y = action["y"].as_i64().context("move_relative requires 'y' field")? as i8;
                // TODO: Implement relative mouse movement via USB/IP
                Ok(ActionResult::NoAction)
            }
            "move_absolute" => {
                let x = action["x"].as_i64().context("move_absolute requires 'x' field")?;
                let y = action["y"].as_i64().context("move_absolute requires 'y' field")?;
                let screen_width = action["screen_width"].as_i64().unwrap_or(1920);
                let screen_height = action["screen_height"].as_i64().unwrap_or(1080);
                // TODO: Implement absolute mouse movement via USB/IP
                // Note: Need to convert absolute coords to relative movements
                Ok(ActionResult::NoAction)
            }
            "click" => {
                let button = action["button"]
                    .as_str()
                    .context("click requires 'button' field")?;
                // TODO: Implement mouse click via USB/IP
                Ok(ActionResult::NoAction)
            }
            "scroll" => {
                let direction = action["direction"]
                    .as_str()
                    .context("scroll requires 'direction' field")?;
                let amount = action["amount"].as_i64().unwrap_or(1) as i8;
                // TODO: Implement mouse scroll via USB/IP
                Ok(ActionResult::NoAction)
            }
            "drag" => {
                let start_x = action["start_x"].as_i64().context("drag requires 'start_x'")?;
                let start_y = action["start_y"].as_i64().context("drag requires 'start_y'")?;
                let end_x = action["end_x"].as_i64().context("drag requires 'end_x'")?;
                let end_y = action["end_y"].as_i64().context("drag requires 'end_y'")?;
                let duration_ms = action["duration_ms"].as_i64().unwrap_or(500);
                // TODO: Implement mouse drag via USB/IP
                Ok(ActionResult::NoAction)
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}

// Action definitions

#[cfg(feature = "usb-mouse")]
fn move_relative_action() -> ActionDefinition {
    ActionDefinition {
        name: "move_relative".to_string(),
        description: "Move mouse cursor by relative offset".to_string(),
        parameters: vec![
            Parameter::new("x", "number", "Horizontal movement in pixels (-127 to 127)"),
            Parameter::new("y", "number", "Vertical movement in pixels (-127 to 127)"),
        ],
        example: json!({
            "type": "move_relative",
            "x": 10,
            "y": -5
        }),
    }
}

#[cfg(feature = "usb-mouse")]
fn move_absolute_action() -> ActionDefinition {
    ActionDefinition {
        name: "move_absolute".to_string(),
        description: "Move mouse cursor to absolute screen position".to_string(),
        parameters: vec![
            Parameter::new("x", "number", "Target X coordinate"),
            Parameter::new("y", "number", "Target Y coordinate"),
            Parameter::new("screen_width", "number", "Screen width in pixels (default: 1920)").optional(),
            Parameter::new("screen_height", "number", "Screen height in pixels (default: 1080)").optional(),
        ],
        example: json!({
            "type": "move_absolute",
            "x": 960,
            "y": 540,
            "screen_width": 1920,
            "screen_height": 1080
        }),
    }
}

#[cfg(feature = "usb-mouse")]
fn click_action() -> ActionDefinition {
    ActionDefinition {
        name: "click".to_string(),
        description: "Click a mouse button".to_string(),
        parameters: vec![
            Parameter::new("button", "string", "Button to click: 'left', 'right', or 'middle'"),
        ],
        example: json!({
            "type": "click",
            "button": "left"
        }),
    }
}

#[cfg(feature = "usb-mouse")]
fn scroll_action() -> ActionDefinition {
    ActionDefinition {
        name: "scroll".to_string(),
        description: "Scroll the mouse wheel".to_string(),
        parameters: vec![
            Parameter::new("direction", "string", "Scroll direction: 'up' or 'down'"),
            Parameter::new("amount", "number", "Number of scroll steps (default: 1)").optional(),
        ],
        example: json!({
            "type": "scroll",
            "direction": "up",
            "amount": 3
        }),
    }
}

#[cfg(feature = "usb-mouse")]
fn drag_action() -> ActionDefinition {
    ActionDefinition {
        name: "drag".to_string(),
        description: "Drag from one position to another with left button held".to_string(),
        parameters: vec![
            Parameter::new("start_x", "number", "Starting X coordinate"),
            Parameter::new("start_y", "number", "Starting Y coordinate"),
            Parameter::new("end_x", "number", "Ending X coordinate"),
            Parameter::new("end_y", "number", "Ending Y coordinate"),
            Parameter::new("duration_ms", "number", "Duration of drag in milliseconds (default: 500)").optional(),
        ],
        example: json!({
            "type": "drag",
            "start_x": 100,
            "start_y": 100,
            "end_x": 200,
            "end_y": 200,
            "duration_ms": 500
        }),
    }
}

#[cfg(feature = "usb-mouse")]
fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more input from the host (do nothing for now)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}
