//! USB HID Mouse protocol actions implementation

#[cfg(feature = "usb-mouse")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-mouse")]
use crate::protocol::log_template::LogTemplate;
#[cfg(feature = "usb-mouse")]
use crate::protocol::EventType;
#[cfg(feature = "usb-mouse")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-mouse")]
use crate::server::usb::descriptors::{mouse_buttons, MouseReport};
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
        "A USB/IP host attached to this virtual mouse and will now accept input reports. Move, \
         click, scroll or drag to drive the host's pointer, or wait_for_more to stay idle.",
        json!({"type": "move_relative", "x": 50, "y": -20}),
    )
    .with_parameters(vec![Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection ID of the USB/IP session. Actions may quote it back, but it is \
                      optional: with one host attached it is inferred."
            .to_string(),
        required: true,
    }])
    .with_actions(vec![
        move_relative_action(),
        move_absolute_action(),
        click_action(),
        scroll_action(),
        drag_action(),
        wait_for_more_action(),
    ])
    .with_alternative_example(json!({"type": "click", "button": "left"}))
});

#[cfg(feature = "usb-mouse")]
pub static USB_MOUSE_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_mouse_detached",
        "The host detached from this virtual mouse. Purely informational - the USB/IP session \
         is gone, so an input report has nowhere to go.",
        json!({"type": "show_message", "message": "USB mouse host detached"}),
    )
    .with_parameters(vec![Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection ID of the USB/IP session. Actions may quote it back, but it is \
                      optional: with one host attached it is inferred."
            .to_string(),
        required: true,
    }])
    .with_no_actions()
});

/// USB HID Mouse protocol action handler
#[cfg(feature = "usb-mouse")]
pub struct UsbMouseProtocol {
    /// Map of active connections (for async actions)
    #[allow(dead_code)]
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    /// Map of USB/IP mouse handlers for each connection.
    ///
    /// A `std::sync::Mutex`, not a tokio one: `execute_action` is synchronous and runs on a
    /// runtime worker, so reaching an async lock from it would need
    /// `Handle::current().block_on`, which panics there. No guard is held across an `.await`.
    handlers: Arc<std::sync::Mutex<HashMap<ConnectionId, SharedHandler>>>,
}

/// The handler `usbip` holds for one attached host, as this module shares it.
#[cfg(feature = "usb-mouse")]
type SharedHandler = Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>;

#[cfg(feature = "usb-mouse")]
pub struct ConnectionData {
    // Placeholder for mouse-specific connection data
}

#[cfg(feature = "usb-mouse")]
impl Default for UsbMouseProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "usb-mouse")]
impl UsbMouseProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Store the USB/IP mouse handler for a connection.
    pub async fn set_handler(&self, connection_id: ConnectionId, handler: SharedHandler) {
        self.lock_handlers().insert(connection_id, handler);
    }

    /// Forget a connection's handler when its USB/IP session ends.
    pub async fn remove_handler(&self, connection_id: ConnectionId) {
        self.lock_handlers().remove(&connection_id);
    }

    fn lock_handlers(&self) -> std::sync::MutexGuard<'_, HashMap<ConnectionId, SharedHandler>> {
        self.handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Find the handler an action refers to.
    ///
    /// `connection_id` is **optional**: with exactly one host attached it is inferred, and with
    /// several, omitting it is an error that names the candidates. It used to be required *and*
    /// string-typed, so a model that emitted the id as a number - which they routinely do -
    /// got "USB mouse actions require 'connection_id'". Both forms are accepted.
    fn resolve_handler(&self, action: &serde_json::Value) -> Result<SharedHandler> {
        let handlers = self.lock_handlers();

        // All three forms a model can produce: the number, the number as a string, and the
        // `conn-N` form the events themselves carry. That last one matters — every event
        // reports `connection_id.to_string()`, which is `"conn-2"`, not `2`.
        let requested = action["connection_id"].as_u64().or_else(|| {
            action["connection_id"].as_str().and_then(|s| {
                let s = s.trim();
                s.strip_prefix("conn-").unwrap_or(s).parse::<u64>().ok()
            })
        });

        if let Some(id) = requested {
            let connection_id = ConnectionId::new(id as u32);
            return handlers.get(&connection_id).cloned().ok_or_else(|| {
                anyhow::anyhow!("No USB mouse attached on connection {}", connection_id)
            });
        }

        match handlers.len() {
            0 => Err(anyhow::anyhow!(
                "No USB/IP host is attached to this mouse, so an input report has nowhere to go"
            )),
            1 => Ok(handlers.values().next().cloned().expect("len checked")),
            _ => {
                let mut ids: Vec<u32> = handlers.keys().map(|c| c.as_u32()).collect();
                ids.sort_unstable();
                Err(anyhow::anyhow!(
                    "{} USB/IP hosts are attached ({:?}); the action must name one with \
                     'connection_id'",
                    ids.len(),
                    ids
                ))
            }
        }
    }

    /// Queue reports on the handler an action refers to.
    ///
    /// The host polls the interrupt IN endpoint every 10ms and takes one report per poll, so a
    /// sequence of reports paces itself on the wire. Nothing here sleeps.
    fn queue(&self, action: &serde_json::Value, reports: Vec<MouseReport>) -> Result<usize> {
        if reports.is_empty() {
            return Err(anyhow::anyhow!("action produced no mouse reports"));
        }
        let handler = self.resolve_handler(action)?;
        let mut guard = handler
            .lock()
            .map_err(|poisoned| poisoned.into_inner())
            .unwrap_or_else(|guard| guard);
        let mouse = guard
            .as_any()
            .downcast_mut::<super::handler::UsbHidMouseHandler>()
            .context("Handler is not a USB HID mouse handler")?;
        let count = reports.len();
        for report in reports {
            mouse.queue(report);
        }
        Ok(count)
    }
}

/// Split a movement into steps a single HID report can carry.
///
/// A boot-protocol report holds one signed byte per axis, so anything past +/-127 has to become
/// several reports. The model is not asked to know that: it says "move 400 right" and gets four
/// reports.
#[cfg(feature = "usb-mouse")]
fn split_movement(mut x: i64, mut y: i64) -> Vec<MouseReport> {
    let mut reports = Vec::new();
    while x != 0 || y != 0 {
        let dx = x.clamp(-127, 127);
        let dy = y.clamp(-127, 127);
        let mut report = MouseReport::new();
        report.x = dx as i8;
        report.y = dy as i8;
        reports.push(report);
        x -= dx;
        y -= dy;
    }
    reports
}

/// Map a button name to its HID report bit.
#[cfg(feature = "usb-mouse")]
fn button_bit(name: &str) -> Result<u8> {
    match name.to_ascii_lowercase().as_str() {
        "left" => Ok(mouse_buttons::LEFT),
        "right" => Ok(mouse_buttons::RIGHT),
        "middle" => Ok(mouse_buttons::MIDDLE),
        other => Err(anyhow::anyhow!(
            "unknown mouse button '{}'; expected left, right or middle",
            other
        )),
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
            .notes("Virtual USB HID mouse over USB/IP. A hand-written UsbInterfaceHandler (handler.rs) supplies the report descriptor and 4-byte reports, because usbip 0.9 still ships no UsbHidMouseHandler. It was written but never wired: handle_connection took the accepted socket as _stream, dropped it, ran no USB/IP session, and parked on sleep(u64::MAX), while every action logged 'not yet implemented' and returned NoAction. All of that is live now, and usb_mouse_detached has an emit site for the first time. move_relative splits movement larger than a report can carry; click and drag emit their own release, so neither sticks; scroll sends one detent per report. move_absolute has no way to know where the pointer is -- a boot-protocol mouse is relative-only -- so it slams into the top-left corner first and moves out from there, which is visible on screen. Exercised against an in-test USB/IP client that decodes the HID reports; never against a real Linux usbip host, so nothing here has been seen by a kernel HID driver.")
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

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        // Deterministic: nudge the pointer once the emulated mouse is attached,
        // no LLM call.
        let script = r#"import json, sys
data = json.load(sys.stdin)
event = data["event"]
if data["event_type_id"] == "usb_mouse_attached":
    actions = [{"type": "move_relative", "x": 10, "y": -5}]
else:
    actions = []
print(json.dumps({"actions": actions}))"#;

        StartupExamples::new(
            // LLM mode: LLM handles USB mouse device
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-mouse",
                "instruction": "Create a USB mouse and move it in a circle pattern when attached"
            }),
            // Script mode: Code-based mouse handling
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-mouse",
                "event_handlers": [{
                    "event_pattern": "usb_mouse_attached",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": script
                    }
                }]
            }),
            // Static mode: Fixed mouse action
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-mouse",
                "event_handlers": [{
                    "event_pattern": "usb_mouse_attached",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "move_relative",
                            "x": 100,
                            "y": 50
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait
#[cfg(feature = "usb-mouse")]
impl Server for UsbMouseProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            crate::server::usb::mouse::UsbMouseServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        // Every action below used to parse its parameters, log
        // "not yet implemented - usbip crate lacks mouse support", and return NoAction. The
        // model was handed a full pointer vocabulary that moved nothing: the server never even
        // ran a USB/IP session (see mod.rs). They drive handler.rs now.
        match action_type {
            "move_relative" => {
                let x = action["x"]
                    .as_i64()
                    .context("move_relative requires 'x' field")?;
                let y = action["y"]
                    .as_i64()
                    .context("move_relative requires 'y' field")?;
                if x == 0 && y == 0 {
                    return Err(anyhow::anyhow!(
                        "move_relative with x=0 and y=0 would move nothing"
                    ));
                }

                let reports = split_movement(x, y);
                let count = self.queue(&action, reports)?;
                tracing::info!("USB mouse move ({}, {}) as {} report(s)", x, y, count);
                Ok(ActionResult::NoAction)
            }

            "move_absolute" => {
                let x = action["x"]
                    .as_i64()
                    .context("move_absolute requires 'x' field")?;
                let y = action["y"]
                    .as_i64()
                    .context("move_absolute requires 'y' field")?;
                let screen_width = action["screen_width"].as_i64().unwrap_or(1920);
                let screen_height = action["screen_height"].as_i64().unwrap_or(1080);

                if !(0..=screen_width).contains(&x) || !(0..=screen_height).contains(&y) {
                    return Err(anyhow::anyhow!(
                        "move_absolute target ({}, {}) is outside the {}x{} screen",
                        x,
                        y,
                        screen_width,
                        screen_height
                    ));
                }

                // A boot-protocol mouse reports *relative* motion and nothing tells the device
                // where the pointer currently is. The only way to reach an absolute position is
                // the one every automation tool uses: slam into the top-left corner, which the
                // host clamps, and move out from a known origin. It is not silent about the
                // cost - this is more reports than a relative move, and it visibly moves the
                // pointer to the corner first.
                let mut reports = split_movement(-screen_width - 127, -screen_height - 127);
                reports.extend(split_movement(x, y));
                let count = self.queue(&action, reports)?;
                tracing::info!(
                    "USB mouse move_absolute to ({}, {}) on {}x{} as {} report(s)",
                    x,
                    y,
                    screen_width,
                    screen_height,
                    count
                );
                Ok(ActionResult::NoAction)
            }

            "click" => {
                let button = action["button"]
                    .as_str()
                    .context("click requires 'button' field")?;
                let bit = button_bit(button)?;

                // Press then release. Without the release report the host sees the button as
                // still held, which turns a click into a stuck drag.
                let mut press = MouseReport::new();
                press.buttons = bit;
                self.queue(&action, vec![press, MouseReport::new()])?;
                tracing::info!("USB mouse {} click", button);
                Ok(ActionResult::NoAction)
            }

            "scroll" => {
                let direction = action["direction"]
                    .as_str()
                    .context("scroll requires 'direction' field")?;
                let amount = action["amount"].as_i64().unwrap_or(1);
                if amount <= 0 {
                    return Err(anyhow::anyhow!("scroll 'amount' must be positive"));
                }

                let step: i8 = match direction.to_ascii_lowercase().as_str() {
                    "up" => 1,
                    "down" => -1,
                    other => {
                        return Err(anyhow::anyhow!(
                            "unknown scroll direction '{}'; expected up or down",
                            other
                        ))
                    }
                };

                // One detent per report: a wheel value of N is not N clicks to every host, and
                // repeated single detents is what a real wheel produces.
                let reports: Vec<MouseReport> = (0..amount.min(64))
                    .map(|_| {
                        let mut r = MouseReport::new();
                        r.wheel = step;
                        r
                    })
                    .collect();
                let count = self.queue(&action, reports)?;
                tracing::info!("USB mouse scroll {} x{}", direction, count);
                Ok(ActionResult::NoAction)
            }

            "drag" => {
                let start_x = action["start_x"]
                    .as_i64()
                    .context("drag requires 'start_x'")?;
                let start_y = action["start_y"]
                    .as_i64()
                    .context("drag requires 'start_y'")?;
                let end_x = action["end_x"].as_i64().context("drag requires 'end_x'")?;
                let end_y = action["end_y"].as_i64().context("drag requires 'end_y'")?;
                let duration_ms = action["duration_ms"].as_i64().unwrap_or(500).max(0);
                let button = action["button"].as_str().unwrap_or("left");
                let bit = button_bit(button)?;

                // The host polls every 10ms and takes one report per poll, so the number of
                // steps *is* the duration. Nothing sleeps: `execute_action` is synchronous and
                // runs on a runtime worker, where a blocking sleep would stall it.
                let steps = (duration_ms / 10).clamp(1, 64);
                let (dx, dy) = (end_x - start_x, end_y - start_y);

                let mut reports = Vec::new();
                let mut press = MouseReport::new();
                press.buttons = bit;
                reports.push(press);

                let mut moved_x = 0i64;
                let mut moved_y = 0i64;
                for step in 1..=steps {
                    let target_x = dx * step / steps;
                    let target_y = dy * step / steps;
                    for mut report in split_movement(target_x - moved_x, target_y - moved_y) {
                        // Keep the button held for every intermediate report, or the host sees
                        // the drag end at the first movement.
                        report.buttons = bit;
                        reports.push(report);
                    }
                    moved_x = target_x;
                    moved_y = target_y;
                }
                reports.push(MouseReport::new()); // release

                let count = self.queue(&action, reports)?;
                tracing::info!(
                    "USB mouse drag ({}, {}) -> ({}, {}) with {} held, {} report(s)",
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    button,
                    count
                );
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse move ({x}, {y})")
                .with_debug("USB-Mouse move_relative: x={x} y={y}"),
        ),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse move to ({x}, {y})")
                .with_debug(
                    "USB-Mouse move_absolute: x={x} y={y} screen={screen_width}x{screen_height}",
                ),
        ),
    }
}

#[cfg(feature = "usb-mouse")]
fn click_action() -> ActionDefinition {
    ActionDefinition {
        name: "click".to_string(),
        description: "Click a mouse button".to_string(),
        parameters: vec![Parameter {
            name: "button".to_string(),
            type_hint: "string".to_string(),
            description: "Button to click: 'left', 'right', or 'middle'".to_string(),
            required: true,
        }],
        example: json!({
            "type": "click",
            "button": "left"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse click {button}")
                .with_debug("USB-Mouse click: button={button}"),
        ),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse scroll {direction} ({amount})")
                .with_debug("USB-Mouse scroll: direction={direction} amount={amount}"),
        ),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse drag ({start_x},{start_y}) to ({end_x},{end_y})")
                .with_debug("USB-Mouse drag: from=({start_x},{start_y}) to=({end_x},{end_y}) duration={duration_ms}ms"),
        ),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB mouse wait for more")
                .with_debug("USB-Mouse wait_for_more"),
        ),
    }
}
