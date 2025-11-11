//! BLE File Transfer Service
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleFileTransfer;
impl BluetoothBleFileTransfer {
    #[cfg(feature = "bluetooth-ble-file-transfer")]
    pub async fn spawn_with_llm_actions(
        _: String,
        llm: crate::llm::ollama_client::OllamaClient,
        state: Arc<crate::state::app_state::AppState>,
        tx: mpsc::UnboundedSender<String>,
        id: crate::state::ServerId,
        inst: String,
    ) -> Result<std::net::SocketAddr> {
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(
            "NetGet-FileTransfer".to_string(),
            llm,
            state,
            tx,
            id,
            format!("{}. Configure as BLE File Transfer.", inst),
        )
        .await
    }
}
#[cfg(not(feature = "bluetooth-ble-file-transfer"))]
impl BluetoothBleFileTransfer {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE file_transfer not enabled")
    }
}
