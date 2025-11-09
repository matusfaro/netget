#![cfg(all(test, feature = "bluetooth-ble-proximity"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_proximity_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_PROXIMITY", 8940, json!({}), "Act as proximity").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
