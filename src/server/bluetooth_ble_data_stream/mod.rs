//! BLE Data Stream Service (custom GATT for real-time streaming)
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleDataStream;
impl BluetoothBleDataStream {
    #[cfg(feature = "bluetooth-ble-data-stream")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm: crate::llm::ollama_client::OllamaClient,
        state: Arc<crate::state::app_state::AppState>,
        tx: mpsc::UnboundedSender<String>,
        id: crate::state::ServerId,
        inst: String,
    ) -> Result<std::net::SocketAddr> {
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(device_name, llm, state, tx, id, format!("{}. Configure as BLE data streaming service with custom GATT characteristics for real-time sensor data.", inst)).await
    }
}
#[cfg(not(feature = "bluetooth-ble-data-stream"))]
impl BluetoothBleDataStream {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE data-stream not enabled")
    }
}
