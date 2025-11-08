//! Bluetooth Low Energy (BLE) Battery Service implementation
//!
//! Builds on bluetooth-ble to provide standard Battery Service (0x180F).
//! Reports battery level as a percentage (0-100%).

pub mod actions;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::server::bluetooth_ble::BluetoothBle;

/// BLE Battery Service server
pub struct BluetoothBleBattery;

impl BluetoothBleBattery {
    /// Spawn BLE battery service server
    #[cfg(feature = "bluetooth-ble-battery")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        initial_level: u8,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE Battery Service: {} (initial level: {}%)", device_name, initial_level);

        // Create the underlying BLE server with Battery Service configuration
        let battery_instruction = format!(
            "{}. Configure as a BLE Battery Service (UUID: 0x180F) with Battery Level characteristic (UUID: 0x2A19). Initial battery level: {}%.",
            instruction,
            initial_level
        );

        // Use the base bluetooth-ble server
        BluetoothBle::spawn_with_llm_actions(
            device_name,
            llm_client,
            app_state,
            status_tx,
            server_id,
            battery_instruction,
        ).await
    }
}

#[cfg(not(feature = "bluetooth-ble-battery"))]
impl BluetoothBleBattery {
    pub async fn spawn_with_llm_actions(
        _device_name: String,
        _initial_level: u8,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE battery support not enabled - compile with --features bluetooth-ble-battery")
    }
}

/// Battery Service UUIDs (Bluetooth SIG assigned numbers)
pub mod battery_uuids {
    /// Battery Service UUID
    pub const BATTERY_SERVICE: u16 = 0x180F;

    /// Battery Level characteristic UUID
    pub const BATTERY_LEVEL: u16 = 0x2A19;
}

/// Battery Level characteristic value (1 byte)
///
/// Value range: 0-100 (percentage)
///
/// Example: 75% battery
/// ```
/// 0x4B  (75 decimal)
/// ```
pub fn encode_battery_level(level: u8) -> [u8; 1] {
    let clamped = level.min(100);
    [clamped]
}
