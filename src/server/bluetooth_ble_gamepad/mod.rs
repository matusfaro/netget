//! BLE Gamepad Service
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleGamepad;
impl BluetoothBleGamepad {
    #[cfg(feature = "bluetooth-ble-gamepad")]
    pub async fn spawn_with_llm_actions(
        _: String,
        llm: crate::llm::ollama_client::OllamaClient,
        state: Arc<crate::state::app_state::AppState>,
        tx: mpsc::UnboundedSender<String>,
        id: crate::state::ServerId,
        inst: String,
    ) -> Result<std::net::SocketAddr> {
        crate::server::bluetooth_ble::BluetoothBle::spawn_with_llm_actions(
            "NetGet-Gamepad".to_string(),
            llm,
            state,
            tx,
            id,
            format!("{}. Configure as BLE Gamepad.", inst),
        )
        .await
    }
}
#[cfg(not(feature = "bluetooth-ble-gamepad"))]
impl BluetoothBleGamepad {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE gamepad not enabled")
    }
}
