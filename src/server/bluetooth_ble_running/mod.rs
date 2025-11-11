//! BLE Running Speed and Cadence Service (0x1814)
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleRunning;
impl BluetoothBleRunning {
    #[cfg(feature = "bluetooth-ble-running")]
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
            format!(
                "{}. Configure as BLE Running Speed and Cadence Service (0x1814).",
                inst
            ),
        )
        .await
    }
}
#[cfg(not(feature = "bluetooth-ble-running"))]
impl BluetoothBleRunning {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE running not enabled")
    }
}

pub const RUNNING_SERVICE: u16 = 0x1814;
pub const RSC_MEASUREMENT: u16 = 0x2A53;
