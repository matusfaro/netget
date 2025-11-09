//! BLE Heart Rate Service (0x180D)

pub mod actions;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

pub struct BluetoothBleHeartRate;

impl BluetoothBleHeartRate {
    #[cfg(feature = "bluetooth-ble-heart-rate")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm_client: crate::llm::ollama_client::OllamaClient,
        app_state: Arc<crate::state::app_state::AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE Heart Rate Service: {}", device_name);
        let hr_instruction = format!("Configure as BLE Heart Rate Service (UUID: 0x180D) with Heart Rate Measurement characteristic (UUID: 0x2A37).", "");
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(device_name, llm_client, app_state, status_tx, server_id, hr_instruction).await
    }
}

#[cfg(not(feature = "bluetooth-ble-heart-rate"))]
impl BluetoothBleHeartRate {
    pub async fn spawn_with_llm_actions(_: String, _: crate::llm::ollama_client::OllamaClient, _: Arc<crate::state::app_state::AppState>, _: mpsc::UnboundedSender<String>, _: crate::state::ServerId) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE heart-rate not enabled - compile with --features bluetooth-ble-heart-rate")
    }
}

pub const HEART_RATE_SERVICE: u16 = 0x180D;
pub const HEART_RATE_MEASUREMENT: u16 = 0x2A37;

pub fn encode_heart_rate(bpm: u8) -> [u8; 2] {
    [0x00, bpm.clamp(30, 220)]  // Flags byte + BPM
}
