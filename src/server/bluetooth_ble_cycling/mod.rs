//! BLE Cycling Speed and Cadence Service (0x1816)
pub mod actions;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct BluetoothBleCycling;
impl BluetoothBleCycling {
    #[cfg(feature = "bluetooth-ble-cycling")]
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
                "{}. Configure as BLE Cycling Speed and Cadence Service (0x1816).",
                inst
            ),
        )
        .await
    }
}
#[cfg(not(feature = "bluetooth-ble-cycling"))]
impl BluetoothBleCycling {
    pub async fn spawn_with_llm_actions(
        _: String,
        _: crate::llm::ollama_client::OllamaClient,
        _: Arc<crate::state::app_state::AppState>,
        _: mpsc::UnboundedSender<String>,
        _: crate::state::ServerId,
        _: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE cycling not enabled")
    }
}

pub const CYCLING_SERVICE: u16 = 0x1816;
pub const CSC_MEASUREMENT: u16 = 0x2A5B;
