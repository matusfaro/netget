//! BLE Health Thermometer Service (0x1809)
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleThermometer;
impl BluetoothBleThermometer {
    #[cfg(feature = "bluetooth-ble-thermometer")]
    pub async fn spawn_with_llm_actions(_: String, llm: crate::llm::ollama_client::OllamaClient, state: Arc<crate::state::app_state::AppState>, tx: mpsc::UnboundedSender<String>, id: crate::state::ServerId, inst: String) -> Result<std::net::SocketAddr> {
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions("NetGet-Thermometer".to_string(), llm, state, tx, id, format!("{}. Configure as BLE Health Thermometer (0x1809).", inst)).await
    }
}
#[cfg(not(feature = "bluetooth-ble-thermometer"))]
impl BluetoothBleThermometer {
    pub async fn spawn_with_llm_actions(_: String, _: crate::llm::ollama_client::OllamaClient, _: Arc<crate::state::app_state::AppState>, _: mpsc::UnboundedSender<String>, _: crate::state::ServerId, _: String) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE thermometer not enabled")
    }
}
