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
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID of the USB/IP session".to_string(),
            required: true,
        },
    ])
});

#[cfg(feature = "usb-mouse")]
pub static USB_MOUSE_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_mouse_detached",
        "Host detached from USB mouse device",
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID of the USB/IP session".to_string(),
            required: true,
        },
    ])
});

/// USB HID Mouse protocol action handler
#[cfg(feature = "usb-mouse")]
pub struct UsbMouseProtocol {
    /// Map of active connections (for async actions)
    #[allow(dead_code)]
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    /// Map of USB/IP mouse handlers for each connection
    handlers: Arc<Mutex<HashMap<ConnectionId, Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>>>>,
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
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store the USB/IP mouse handler for a connection
    pub async fn set_handler(
        &self,
        connection_id: ConnectionId,
        handler: Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>,
    ) {
        self.handlers.lock().await.insert(connection_id, handler);
    }

    /// Get the USB/IP mouse handler for a connection
    #[allow(dead_code)]
    async fn get_handler(
        &self,
        connection_id: ConnectionId,
    ) -> Option<Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>> {
        self.handlers.lock().await.get(&connection_id).cloned()
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
        crate::protocol::metadata::ProtocolMetadataV2::builder()
            .state(crate::protocol::metadata::DevelopmentState::Experimental)
            .implementation("Virtual USB HID mouse device using USB/IP protocol")
            .llm_control("LLM controls mouse movement, clicks, and scrolling")
            .e2e_testing("E2E tests using Linux usbip client")
            .privilege_requirement(crate::protocol::metadata::PrivilegeRequirement::None)
            .notes("Requires client to have vhci-hcd kernel module and run 'usbip attach'")
            .build()
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
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        let _connection_id = action["connection_id"]
            .as_str()
            .context("USB mouse actions require 'connection_id' field in action")?;

        // TODO: USB mouse handler integration once usbip crate supports mouse
        // For now, all actions are stubs that log warnings

        match action_type {
            "move_relative" => {
                let _x = action["x"].as_i64().context("move_relative requires 'x' field")? as i8;
                let _y = action["y"].as_i64().context("move_relative requires 'y' field")? as i8;

                // TODO: USB mouse support not yet implemented in usbip crate
                // Need to implement UsbHidMouseHandler and UsbHidMouseReport
                // See keyboard implementation for reference
                tracing::warn!(
                    "move_relative not yet implemented - usbip crate lacks mouse support"
                );
                Ok(ActionResult::NoAction)
            }
            "move_absolute" => {
                let _x = action["x"].as_i64().context("move_absolute requires 'x' field")?;
                let _y = action["y"].as_i64().context("move_absolute requires 'y' field")?;
                let _screen_width = action["screen_width"].as_i64().unwrap_or(1920);
                let _screen_height = action["screen_height"].as_i64().unwrap_or(1080);
                // TODO: Implement absolute positioning (requires tracking current position)
                tracing::warn!("move_absolute not yet implemented for USB mouse");
                Ok(ActionResult::NoAction)
            }
            "click" => {
                let _button = action["button"]
                    .as_str()
                    .context("click requires 'button' field")?;

                // TODO: USB mouse support not yet implemented in usbip crate
                tracing::warn!(
                    "click not yet implemented - usbip crate lacks mouse support"
                );
                Ok(ActionResult::NoAction)
            }
            "scroll" => {
                let _direction = action["direction"]
                    .as_str()
                    .context("scroll requires 'direction' field")?;
                let _amount = action["amount"].as_i64().unwrap_or(1) as i8;

                // TODO: USB mouse support not yet implemented in usbip crate
                tracing::warn!(
                    "scroll not yet implemented - usbip crate lacks mouse support"
                );
                Ok(ActionResult::NoAction)
            }
            "drag" => {
                let _start_x = action["start_x"].as_i64().context("drag requires 'start_x'")?;
                let _start_y = action["start_y"].as_i64().context("drag requires 'start_y'")?;
                let _end_x = action["end_x"].as_i64().context("drag requires 'end_x'")?;
                let _end_y = action["end_y"].as_i64().context("drag requires 'end_y'")?;
                let _duration_ms = action["duration_ms"].as_i64().unwrap_or(500);
                // TODO: Implement drag (requires position tracking + smooth movement)
                tracing::warn!("drag not yet implemented for USB mouse");
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
            Parameter {
                name: "x".to_string(),
                type_hint: "number".to_string(),
                description: "Horizontal movement in pixels (-127 to 127)".to_string(),
                required: true,
            },
            Parameter {
                name: "y".to_string(),
                type_hint: "number".to_string(),
                description: "Vertical movement in pixels (-127 to 127)".to_string(),
                required: true,
            },
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
            Parameter {
                name: "x".to_string(),
                type_hint: "number".to_string(),
                description: "Target X coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "y".to_string(),
                type_hint: "number".to_string(),
                description: "Target Y coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "screen_width".to_string(),
                type_hint: "number".to_string(),
                description: "Screen width in pixels (default: 1920)".to_string(),
                required: false,
            },
            Parameter {
                name: "screen_height".to_string(),
                type_hint: "number".to_string(),
                description: "Screen height in pixels (default: 1080)".to_string(),
                required: false,
            },
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
            Parameter {
                name: "button".to_string(),
                type_hint: "string".to_string(),
                description: "Button to click: 'left', 'right', or 'middle'".to_string(),
                required: true,
            },
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
            Parameter {
                name: "direction".to_string(),
                type_hint: "string".to_string(),
                description: "Scroll direction: 'up' or 'down'".to_string(),
                required: true,
            },
            Parameter {
                name: "amount".to_string(),
                type_hint: "number".to_string(),
                description: "Number of scroll steps (default: 1)".to_string(),
                required: false,
            },
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
            Parameter {
                name: "start_x".to_string(),
                type_hint: "number".to_string(),
                description: "Starting X coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "start_y".to_string(),
                type_hint: "number".to_string(),
                description: "Starting Y coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "end_x".to_string(),
                type_hint: "number".to_string(),
                description: "Ending X coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "end_y".to_string(),
                type_hint: "number".to_string(),
                description: "Ending Y coordinate".to_string(),
                required: true,
            },
            Parameter {
                name: "duration_ms".to_string(),
                type_hint: "number".to_string(),
                description: "Duration of drag in milliseconds (default: 500)".to_string(),
                required: false,
            },
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
