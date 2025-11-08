#![cfg(all(test, feature = "bluetooth-ble-data-stream"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_data_stream_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_DATA_STREAM", 8940, json!({}), "Stream sensor data").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
