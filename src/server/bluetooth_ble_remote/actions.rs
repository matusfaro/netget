//! BLE Remote Control protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
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
    )
    .with_parameters(vec![
        Parameter {
            name: "button".to_string(),
            type_hint: "string".to_string(),
            description: "Button name (play_pause, volume_up, etc.)".to_string(),
            required: true,
        },
    ])
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
        vec![
            ParameterDefinition {
                name: "device_name".to_string(),
                type_hint: "string".to_string(),
                description: "Remote device name (default: NetGet-Remote)".to_string(),
                required: false,
                example: json!("MyRemote"),
            },
        ]
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
        vec![
            REMOTE_BUTTON_PRESSED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "ble", "remote", "media", "bluetooth_ble_remote"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

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
}

impl Server for BluetoothBleRemoteProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            let device_name = ctx.startup_params.as_ref().and_then(|p| p.get_optional_string("device_name"))
                .as_deref()
                .unwrap_or("NetGet-Remote")
                .to_string();

            // Get instruction from server instance
            let instruction = ctx.state.get_server(ctx.server_id).await
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

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "play_pause" | "next_track" | "previous_track" | "volume_up" | "volume_down" |
            "mute" | "fast_forward" | "rewind" | "stop" => {
                Ok(ActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
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
    }
}
