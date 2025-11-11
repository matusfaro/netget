//! BLE Weight Scale Service (0x181D)
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleWeightScale;
impl BluetoothBleWeightScale {
    #[cfg(feature = "bluetooth-ble-weight-scale")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm: crate::llm::ollama_client::OllamaClient,
        state: Arc<crate::state::app_state::AppState>,
        tx: mpsc::UnboundedSender<String>,
        id: crate::state::ServerId,
        inst: String,
    ) -> Result<std::net::SocketAddr> {
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(
            device_name,
            llm,
            state,
            tx,
            id,
            format!("{}. Configure as BLE Weight Scale Service (0x181D).", inst),
        )
        .await
    }
}
#[cfg(not(feature = "bluetooth-ble-weight-scale"))]
impl BluetoothBleWeightScale {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE weight-scale not enabled")
    }
}

pub const WEIGHT_SCALE_SERVICE: u16 = 0x181D;
pub const WEIGHT_MEASUREMENT: u16 = 0x2A9D;
