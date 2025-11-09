//! BLE File Transfer Service
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, ParameterDefinition};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

pub struct BluetoothBleFileTransferProtocol;
impl BluetoothBleFileTransferProtocol { pub fn new() -> Self { Self } }

impl Protocol for BluetoothBleFileTransferProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> { vec![] }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> { vec![] }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> { vec![] }
    fn protocol_name(&self) -> &'static str { "BLUETOOTH_BLE_FILE_TRANSFER" }
    fn get_event_types(&self) -> Vec<EventType> { vec![] }
    fn stack_name(&self) -> &'static str { "DATALINK" }
    fn keywords(&self) -> Vec<&'static str> { vec!["bluetooth", "ble", "file_transfer"] }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
        ProtocolMetadataV2::builder().state(DevelopmentState::Experimental)
            .implementation("BLE File Transfer").llm_control("File Transfer actions")
            .e2e_testing("Requires BLE device").notes("BLE File Transfer").build()
    }
    fn description(&self) -> &'static str { "BLE File Transfer" }
    fn example_prompt(&self) -> &'static str { "Act as a BLE file_transfer device" }
    fn group_name(&self) -> &'static str { "Network" }
}

impl Server for BluetoothBleFileTransferProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::bluetooth_ble_file_transfer::BluetoothBleFileTransfer::spawn_with_llm_actions(
                "NetGet-FileTransfer".to_string(), ctx.llm_client, ctx.app_state, ctx.status_tx, ctx.server_id, ctx.instruction
            ).await
        })
    }
    fn execute_action(&self, _: Option<crate::server::connection::ConnectionId>, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        Ok(ActionResult::Custom { name: action_type.to_string(), data: action })
    }
}
