//! BLE Remote Control protocol actions

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

/// Remote control button pressed event
pub static REMOTE_BUTTON_PRESSED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "remote_button_pressed",
        "A remote control button was pressed",
        json!({
            "type": "play_pause"
        })
    )
    .with_parameters(vec![Parameter {
        name: "button".to_string(),
        type_hint: "string".to_string(),
        description: "Button name (play_pause, volume_up, etc.)".to_string(),
        required: true,
    }])
    .with_log_template(
        LogTemplate::new()
            .with_info("BLE remote button pressed: {button}")
            .with_debug("BLE remote button: {button}")
            .with_trace("BLE remote event: {json_pretty(.)}"),
    )
});

/// BLE Remote Control protocol handler
pub struct BluetoothBleRemoteProtocol;

impl BluetoothBleRemoteProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBleRemoteProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "device_name".to_string(),
            type_hint: "string".to_string(),
            description: "Remote device name (default: NetGet-Remote)".to_string(),
            required: false,
            example: json!("MyRemote"),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            play_pause_action(),
            next_track_action(),
            previous_track_action(),
            volume_up_action(),
            volume_down_action(),
            mute_action(),
            fast_forward_action(),
            rewind_action(),
            stop_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_REMOTE"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![REMOTE_BUTTON_PRESSED_EVENT.clone()]
    }

    fn stack_name(&self) -> &'static str {
        "BLUETOOTH_BLE_REMOTE"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "remote", "media", "bluetooth_ble_remote"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE HID Consumer Control remote (builds on bluetooth-ble)")
            .llm_control("Media control actions: play/pause, volume, track navigation")
            .e2e_testing("Requires BLE-capable device to pair and receive media commands")
            .notes("HID Consumer Control profile for media player/TV remote control.")
            .build()
    }

    fn description(&self) -> &'static str {
        "BLE remote control - media player/TV remote with play/pause, volume, etc."
    }

    fn example_prompt(&self) -> &'static str {
        "Act as a Bluetooth media remote. When connected, press play/pause then volume up."
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles remote control
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-remote",
                "instruction": "Act as a Bluetooth media remote. When connected, press play/pause then volume up.",
                "startup_params": {
                    "device_name": "NetGet-Remote"
                }
            }),
            // Script mode: Code-based remote handling
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-remote",
                "startup_params": {
                    "device_name": "NetGet-Remote"
                },
                "event_handlers": [{
                    "event_pattern": "remote_button_pressed",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<remote_handler>"
                    }
                }]
            }),
            // Static mode: Fixed remote actions
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "bluetooth-ble-remote",
                "startup_params": {
                    "device_name": "NetGet-Remote"
                },
                "event_handlers": [{
                    "event_pattern": "remote_button_pressed",
                    "handler": {
                        "type": "static",
                        "actions": [
                            {"type": "play_pause"},
                            {"type": "volume_up"}
                        ]
                    }
                }]
            }),
        )
    }
}

impl Server for BluetoothBleRemoteProtocol {
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
                .unwrap_or("NetGet-Remote")
                .to_string();

            // Get instruction from server instance
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction)
                .unwrap_or_default();

            crate::server::bluetooth_ble_remote::BluetoothBleRemote::spawn_with_llm_actions(
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
            "play_pause" | "next_track" | "previous_track" | "volume_up" | "volume_down"
            | "mute" | "fast_forward" | "rewind" | "stop" => Ok(ActionResult::Custom {
                name: action_type.to_string(),
                data: action,
            }),
            _ => Err(anyhow::anyhow!("Unknown remote action: {}", action_type)),
        }
    }
}

fn play_pause_action() -> ActionDefinition {
    ActionDefinition {
        name: "play_pause".to_string(),
        description: "Toggle play/pause".to_string(),
        parameters: vec![],
        example: json!({
            "type": "play_pause"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: play/pause")
                .with_debug("BLE remote play_pause"),
        ),
    }
}

fn next_track_action() -> ActionDefinition {
    ActionDefinition {
        name: "next_track".to_string(),
        description: "Skip to next track".to_string(),
        parameters: vec![],
        example: json!({
            "type": "next_track"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: next track")
                .with_debug("BLE remote next_track"),
        ),
    }
}

fn previous_track_action() -> ActionDefinition {
    ActionDefinition {
        name: "previous_track".to_string(),
        description: "Go to previous track".to_string(),
        parameters: vec![],
        example: json!({
            "type": "previous_track"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: previous track")
                .with_debug("BLE remote previous_track"),
        ),
    }
}

fn volume_up_action() -> ActionDefinition {
    ActionDefinition {
        name: "volume_up".to_string(),
        description: "Increase volume".to_string(),
        parameters: vec![],
        example: json!({
            "type": "volume_up"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: volume up")
                .with_debug("BLE remote volume_up"),
        ),
    }
}

fn volume_down_action() -> ActionDefinition {
    ActionDefinition {
        name: "volume_down".to_string(),
        description: "Decrease volume".to_string(),
        parameters: vec![],
        example: json!({
            "type": "volume_down"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: volume down")
                .with_debug("BLE remote volume_down"),
        ),
    }
}

fn mute_action() -> ActionDefinition {
    ActionDefinition {
        name: "mute".to_string(),
        description: "Toggle mute".to_string(),
        parameters: vec![],
        example: json!({
            "type": "mute"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: mute")
                .with_debug("BLE remote mute"),
        ),
    }
}

fn fast_forward_action() -> ActionDefinition {
    ActionDefinition {
        name: "fast_forward".to_string(),
        description: "Fast forward".to_string(),
        parameters: vec![],
        example: json!({
            "type": "fast_forward"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: fast forward")
                .with_debug("BLE remote fast_forward"),
        ),
    }
}

fn rewind_action() -> ActionDefinition {
    ActionDefinition {
        name: "rewind".to_string(),
        description: "Rewind".to_string(),
        parameters: vec![],
        example: json!({
            "type": "rewind"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: rewind")
                .with_debug("BLE remote rewind"),
        ),
    }
}

fn stop_action() -> ActionDefinition {
    ActionDefinition {
        name: "stop".to_string(),
        description: "Stop playback".to_string(),
        parameters: vec![],
        example: json!({
            "type": "stop"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BLE remote: stop")
                .with_debug("BLE remote stop"),
        ),
    }
}
