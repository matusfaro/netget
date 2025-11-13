//! Bluetooth Low Energy (BLE) Remote Control implementation
//!
//! Builds on bluetooth-ble to provide HID Consumer Control functionality.
//! Acts as a media remote control for TVs, media players, etc.

pub mod actions;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::llm::ollama_client::OllamaClient;
use crate::server::bluetooth_ble::BluetoothBle;
use crate::state::app_state::AppState;

/// BLE Remote Control server
pub struct BluetoothBleRemote;

impl BluetoothBleRemote {
    /// Spawn BLE remote control server
    #[cfg(feature = "bluetooth-ble-remote")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE Remote Control server: {}", device_name);

        // Create the underlying BLE server with HID remote configuration
        let remote_instruction = format!(
            "Configure as a BLE HID remote control with HID Service (UUID: 0x1812). {} {}",
            instruction,
            "Add HID Report Map, HID Report Input, HID Information, and HID Control Point characteristics for consumer control."
        );

        // Use the base bluetooth-ble server
        BluetoothBle::spawn_with_llm_actions(
            device_name,
            llm_client,
            app_state,
            status_tx,
            server_id,
            remote_instruction,
        )
        .await
    }
}

#[cfg(not(feature = "bluetooth-ble-remote"))]
impl BluetoothBleRemote {
    pub async fn spawn_with_llm_actions(
        _device_name: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!(
            "BLE remote support not enabled - compile with --features bluetooth-ble-remote"
        )
    }
}

/// HID Consumer Control usage codes
pub mod consumer_control {
    // Media control
    pub const PLAY_PAUSE: u16 = 0xCD;
    pub const NEXT_TRACK: u16 = 0xB5;
    pub const PREVIOUS_TRACK: u16 = 0xB6;
    pub const STOP: u16 = 0xB7;
    pub const FAST_FORWARD: u16 = 0xB3;
    pub const REWIND: u16 = 0xB4;

    // Volume control
    pub const VOLUME_UP: u16 = 0xE9;
    pub const VOLUME_DOWN: u16 = 0xEA;
    pub const MUTE: u16 = 0xE2;

    // Other controls
    pub const POWER: u16 = 0x30;
    pub const MENU: u16 = 0x40;
    pub const HOME: u16 = 0x223;
}

/// HID Report Descriptor for Consumer Control remote
pub const HID_REMOTE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x10, //   Report Count (16)
    // Media controls (16 bits)
    0x09, 0xCD, //   Usage (Play/Pause)
    0x09, 0xB5, //   Usage (Next Track)
    0x09, 0xB6, //   Usage (Previous Track)
    0x09, 0xB7, //   Usage (Stop)
    0x09, 0xB3, //   Usage (Fast Forward)
    0x09, 0xB4, //   Usage (Rewind)
    0x09, 0xE9, //   Usage (Volume Up)
    0x09, 0xEA, //   Usage (Volume Down)
    0x09, 0xE2, //   Usage (Mute)
    0x09, 0x30, //   Usage (Power)
    0x09, 0x40, //   Usage (Menu)
    0x09, 0x23, 0x02, //   Usage (Home)
    0x09, 0x00, //   Usage (Unassigned) - padding
    0x09, 0x00, //   Usage (Unassigned) - padding
    0x09, 0x00, //   Usage (Unassigned) - padding
    0x09, 0x00, //   Usage (Unassigned) - padding
    0x81, 0x02, //   Input (Data, Variable, Absolute)
    0xC0, // End Collection
];

/// Build a remote control report (2 bytes)
///
/// Format:
/// - Bytes 0-1: Button bits (16 buttons)
///
/// Example: Play/Pause pressed
/// ```text
/// 01 00  (bit 0 set)
/// ```
pub fn build_remote_report(button: &str) -> [u8; 2] {
    let mut report = [0u8; 2];

    let bit_position = match button {
        "play_pause" => 0,
        "next_track" => 1,
        "previous_track" => 2,
        "stop" => 3,
        "fast_forward" => 4,
        "rewind" => 5,
        "volume_up" => 6,
        "volume_down" => 7,
        "mute" => 8,
        "power" => 9,
        "menu" => 10,
        "home" => 11,
        _ => return report, // No button pressed
    };

    if bit_position < 8 {
        report[0] = 1 << bit_position;
    } else {
        report[1] = 1 << (bit_position - 8);
    }

    report
}
