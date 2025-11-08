#![cfg(all(test, feature = "bluetooth-ble-cycling"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_cycling_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_CYCLING", 8950, json!({}), "Cycling at 25 km/h").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
