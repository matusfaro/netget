//! BLE Presenter Service
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};

pub struct BluetoothBlePresenterProtocol;
impl BluetoothBlePresenterProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for BluetoothBlePresenterProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![]
    }
    fn get_async_actions(&self, _: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "BLUETOOTH_BLE_PRESENTER"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![]
    }
    fn stack_name(&self) -> &'static str {
        "DATALINK"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bluetooth", "presenter"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("BLE Presenter")
            .llm_control("Presenter actions")
            .e2e_testing("Requires BLE device")
            .notes("BLE Presenter")
            .build()
    }
    fn description(&self) -> &'static str {
        "BLE Presenter"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a BLE presenter device"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl Server for BluetoothBlePresenterProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            let instruction = ctx
                .state
                .get_server(ctx.server_id)
                .await
                .map(|s| s.instruction)
                .unwrap_or_default();
            crate::server::bluetooth_ble_presenter::BluetoothBlePresenter::spawn_with_llm_actions(
                "NetGet-Presenter".to_string(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                instruction,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;
        Ok(ActionResult::Custom {
            name: action_type.to_string(),
            data: action,
        })
    }
}
