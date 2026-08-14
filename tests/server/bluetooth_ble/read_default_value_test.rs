//! DEFECT 1 regression: the read path that falls back to a characteristic's stored value.
//!
//! When the LLM answers a `bluetooth_read_request` without a `respond_to_read`/`send_read_response`
//! action, the server falls back to the characteristic's last stored value. That fallback used to
//! acquire the `ServerData` lock with `futures::executor::block_on(server_data.lock())` inside a
//! synchronous `unwrap_or_else` closure — a blocking lock taken on a tokio worker thread. That is
//! the antipattern the root `CLAUDE.md` calls out: on a runtime thread it can panic ("Cannot block
//! the current thread from within a runtime"), and because this loop runs under `tokio::spawn` the
//! panic is swallowed, so the read dies while the server still looks healthy. It is now awaited on
//! the async path.
//!
//! This test drives the real `BluetoothBle::event_loop` (via `run_event_loop_without_radio`, no
//! Bluetooth hardware) with a mock LLM that answers the read with an empty action list — the LLM
//! call succeeds but produces no `respond_to_read`, so the async fallback is the only path left.
//! A `Success` reply proves the fallback ran (the failure path answers `UnlikelyError` instead);
//! a swallowed panic or a blocked worker in the former `block_on` would show up as the responder
//! never answering — caught by the 20s timeout.

#![cfg(all(test, feature = "bluetooth-ble"))]

use super::super::super::helpers::mock_builder::MockLlmBuilder;
use super::super::super::helpers::mock_ollama::MockOllamaServer;

use ble_peripheral_rust::gatt::peripheral_event::{
    PeripheralEvent, PeripheralRequest, RequestResponse,
};
use netget::llm::ollama_client::OllamaClient;
use netget::server::bluetooth_ble::BluetoothBle;
use netget::state::app_state::AppState;
use netget::state::ServerId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Full 128-bit forms, because the read request carries `Uuid::to_string()` (the long form) and
/// the stored-value map is keyed by exactly the string `add_service` was given. Using the long
/// form on both sides is what makes the fallback lookup hit.
const SERVICE: &str = "0000180d-0000-1000-8000-00805f9b34fb";
const CHARACTERISTIC: &str = "00002a37-0000-1000-8000-00805f9b34fb";

#[tokio::test]
async fn read_falls_back_to_stored_value_without_blocking() -> TestResult {
    // The mock answers the read with no actions: the LLM call succeeds but yields no
    // `respond_to_read`, so the read must take the async fallback that reads the stored-value map
    // (empty here, so an empty value) and answer `Success`. This is the exact line that used to
    // acquire the lock with `futures::executor::block_on` on a tokio worker.
    let mock_config = MockLlmBuilder::new()
        .on_event("bluetooth_read_request")
        .respond_with_actions(serde_json::json!([]))
        .expect_at_least(1)
        .and()
        .build();

    let mock = MockOllamaServer::start(mock_config).await?;

    let (event_tx, event_rx) = mpsc::channel::<PeripheralEvent>(8);
    tokio::spawn(BluetoothBle::run_event_loop_without_radio(
        event_rx,
        ServerId::new(1),
        OllamaClient::new(mock.base_url()),
        Arc::new(AppState::new()),
        mpsc::unbounded_channel::<String>().0,
    ));

    let (responder, response) = oneshot::channel();
    event_tx
        .send(PeripheralEvent::ReadRequest {
            request: PeripheralRequest {
                client: "test-central".to_string(),
                service: uuid::Uuid::parse_str(SERVICE)?,
                characteristic: uuid::Uuid::parse_str(CHARACTERISTIC)?,
            },
            offset: 0,
            responder,
        })
        .await?;

    let reply = tokio::time::timeout(Duration::from_secs(20), response)
        .await
        .map_err(|_| {
            "No ATT read response within 20s — the fallback read path never answered (a swallowed \
             panic in the former block_on would look exactly like this)"
        })??;

    assert_eq!(
        reply.response,
        RequestResponse::Success,
        "the LLM call succeeded, so the read must answer Success via the async fallback — the \
         failure path would answer UnlikelyError"
    );
    assert!(
        reply.value.is_empty(),
        "nothing is stored for this characteristic, so the fallback yields an empty value, got \
         {:?}",
        reply.value
    );

    mock.verify_calls().await?;
    Ok(())
}
