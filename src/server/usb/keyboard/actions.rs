//! USB HID Keyboard protocol actions implementation

#[cfg(feature = "usb-keyboard")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-keyboard")]
use crate::protocol::EventType;
#[cfg(feature = "usb-keyboard")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-keyboard")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-keyboard")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-keyboard")]
use serde_json::json;
#[cfg(feature = "usb-keyboard")]
use std::collections::HashMap;
#[cfg(feature = "usb-keyboard")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "usb-keyboard")]
use tokio::sync::Mutex;

// Event type definitions (static for efficient reuse)
#[cfg(feature = "usb-keyboard")]
pub static USB_KEYBOARD_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_keyboard_attached",
        "Host attached to USB keyboard device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
    ])
});

#[cfg(feature = "usb-keyboard")]
pub static USB_KEYBOARD_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_keyboard_detached",
        "Host detached from USB keyboard device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
    ])
});

#[cfg(feature = "usb-keyboard")]
pub static USB_KEYBOARD_LED_STATUS_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_keyboard_led_status",
        "Host changed keyboard LED status (Num Lock, Caps Lock, Scroll Lock)",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
        Parameter::new("num_lock", "boolean", "Num Lock LED state"),
        Parameter::new("caps_lock", "boolean", "Caps Lock LED state"),
        Parameter::new("scroll_lock", "boolean", "Scroll Lock LED state"),
    ])
});

/// USB HID Keyboard protocol action handler
#[cfg(feature = "usb-keyboard")]
pub struct UsbKeyboardProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    /// Map of USB/IP keyboard handlers for each connection
    handlers: Arc<Mutex<HashMap<ConnectionId, Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>>>>,
}

#[cfg(feature = "usb-keyboard")]
pub struct ConnectionData {
    // Placeholder for keyboard-specific connection data
    // Will be populated during USB/IP implementation
}

#[cfg(feature = "usb-keyboard")]
impl UsbKeyboardProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store the USB/IP keyboard handler for a connection
    pub async fn set_handler(
        &self,
        connection_id: ConnectionId,
        handler: Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>,
    ) {
        self.handlers.lock().await.insert(connection_id, handler);
    }

    /// Get the USB/IP keyboard handler for a connection
    async fn get_handler(
        &self,
        connection_id: ConnectionId,
    ) -> Option<Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>> {
        self.handlers.lock().await.get(&connection_id).cloned()
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-keyboard")]
impl Protocol for UsbKeyboardProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            type_text_action(),
            press_key_action(),
            press_key_combo_action(),
            release_all_keys_action(),
            wait_for_more_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "USB-Keyboard"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            USB_KEYBOARD_ATTACHED_EVENT.clone(),
            USB_KEYBOARD_DETACHED_EVENT.clone(),
            USB_KEYBOARD_LED_STATUS_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "USB>HID>Keyboard"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "keyboard", "hid", "input", "typing"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::new(
            crate::protocol::metadata::ProtocolState::Experimental,
            "Virtual USB HID keyboard device using USB/IP protocol",
            "LLM controls keyboard input (typing, key presses, combinations)",
            "E2E tests using Linux usbip client",
            crate::protocol::metadata::PrivilegeRequirement::None,
        )
        .with_notes("Requires client to have vhci-hcd kernel module and run 'usbip attach'")
    }

    fn description(&self) -> &'static str {
        "Virtual USB HID keyboard device"
    }

    fn example_prompt(&self) -> &'static str {
        "Create a USB keyboard device and type 'hello world' when attached"
    }

    fn group_name(&self) -> &'static str {
        "USB Devices"
    }
}

// Implement Server trait
#[cfg(feature = "usb-keyboard")]
impl Server for UsbKeyboardProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            crate::server::usb::keyboard::UsbKeyboardServer::spawn_with_llm_actions(
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
        connection_id: Option<ConnectionId>,
        _app_state: &AppState,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        let connection_id = connection_id.context("USB keyboard actions require connection_id")?;

        // Get handler (need to use blocking approach since execute_action is sync)
        let handler = {
            let handlers = self.handlers.blocking_lock();
            handlers
                .get(&connection_id)
                .cloned()
                .context("No USB keyboard handler found for connection")?
        };

        match action_type {
            "type_text" => {
                let text = action["text"]
                    .as_str()
                    .context("type_text requires 'text' field")?;
                let typing_speed_ms = action["typing_speed_ms"].as_u64().unwrap_or(50);

                // Queue keyboard events for each character
                let mut handler_guard = handler.lock().unwrap();
                if let Some(hid) = handler_guard
                    .as_any()
                    .downcast_mut::<usbip::hid::UsbHidKeyboardHandler>()
                {
                    for ch in text.chars() {
                        if ch.is_ascii() {
                            let report = usbip::hid::UsbHidKeyboardReport::from_ascii(ch as u8);
                            hid.pending_key_events.push_back(report);
                            // Sleep between characters for natural typing
                            if typing_speed_ms > 0 {
                                std::thread::sleep(std::time::Duration::from_millis(typing_speed_ms));
                            }
                        }
                    }
                    tracing::info!(
                        "Queued {} keyboard events for connection {}",
                        text.len(),
                        connection_id
                    );
                    Ok(ActionResult::NoAction)
                } else {
                    Err(anyhow::anyhow!("Handler is not a USB HID keyboard handler"))
                }
            }
            "press_key" => {
                let key = action["key"]
                    .as_str()
                    .context("press_key requires 'key' field")?;
                let _modifiers: Vec<String> = action["modifiers"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                // Convert key to ASCII and send
                let mut handler_guard = handler.lock().unwrap();
                if let Some(hid) = handler_guard
                    .as_any()
                    .downcast_mut::<usbip::hid::UsbHidKeyboardHandler>()
                {
                    if key.len() == 1 && key.is_ascii() {
                        let report = usbip::hid::UsbHidKeyboardReport::from_ascii(key.as_bytes()[0]);
                        hid.pending_key_events.push_back(report);
                        tracing::info!("Queued key press '{}' for connection {}", key, connection_id);
                        Ok(ActionResult::NoAction)
                    } else {
                        Err(anyhow::anyhow!("Unsupported key: {}", key))
                    }
                } else {
                    Err(anyhow::anyhow!("Handler is not a USB HID keyboard handler"))
                }
            }
            "press_key_combo" => {
                let _keys = action["keys"]
                    .as_array()
                    .context("press_key_combo requires 'keys' array")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                // TODO: Implement key combination via USB/IP
                // This requires building a custom HID report with multiple keys pressed
                tracing::warn!("press_key_combo not yet implemented for USB keyboard");
                Ok(ActionResult::NoAction)
            }
            "release_all_keys" => {
                // Send empty report (all keys released)
                let mut handler_guard = handler.lock().unwrap();
                if let Some(hid) = handler_guard
                    .as_any()
                    .downcast_mut::<usbip::hid::UsbHidKeyboardHandler>()
                {
                    let empty_report = usbip::hid::UsbHidKeyboardReport::new();
                    hid.pending_key_events.push_back(empty_report);
                    tracing::info!("Released all keys for connection {}", connection_id);
                    Ok(ActionResult::NoAction)
                } else {
                    Err(anyhow::anyhow!("Handler is not a USB HID keyboard handler"))
                }
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}

// Action definitions

#[cfg(feature = "usb-keyboard")]
fn type_text_action() -> ActionDefinition {
    ActionDefinition {
        name: "type_text".to_string(),
        description: "Type text on the USB keyboard as if a user typed it".to_string(),
        parameters: vec![
            Parameter::new("text", "string", "Text to type"),
            Parameter::new(
                "typing_speed_ms",
                "number",
                "Delay between keypresses in milliseconds (default: 50ms)",
            )
            .optional(),
        ],
        example: json!({
            "type": "type_text",
            "text": "Hello, World!",
            "typing_speed_ms": 50
        }),
    }
}

#[cfg(feature = "usb-keyboard")]
fn press_key_action() -> ActionDefinition {
    ActionDefinition {
        name: "press_key".to_string(),
        description: "Press a single key with optional modifier keys (Ctrl, Shift, Alt, GUI)"
            .to_string(),
        parameters: vec![
            Parameter::new("key", "string", "Key to press (e.g., 'a', 'enter', 'f1')"),
            Parameter::new(
                "modifiers",
                "array",
                "Modifier keys: 'ctrl', 'shift', 'alt', 'gui' (Windows/Command key)",
            )
            .optional(),
        ],
        example: json!({
            "type": "press_key",
            "key": "c",
            "modifiers": ["ctrl"]
        }),
    }
}

#[cfg(feature = "usb-keyboard")]
fn press_key_combo_action() -> ActionDefinition {
    ActionDefinition {
        name: "press_key_combo".to_string(),
        description: "Press multiple keys simultaneously (e.g., Ctrl+Alt+Delete)".to_string(),
        parameters: vec![Parameter::new(
            "keys",
            "array",
            "Keys to press together: 'ctrl', 'alt', 'delete', etc.",
        )],
        example: json!({
            "type": "press_key_combo",
            "keys": ["ctrl", "alt", "delete"]
        }),
    }
}

#[cfg(feature = "usb-keyboard")]
fn release_all_keys_action() -> ActionDefinition {
    ActionDefinition {
        name: "release_all_keys".to_string(),
        description: "Release all currently pressed keys (useful if stuck)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "release_all_keys"
        }),
    }
}

#[cfg(feature = "usb-keyboard")]
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
