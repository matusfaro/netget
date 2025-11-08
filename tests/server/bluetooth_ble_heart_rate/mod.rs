//! BLE Heart Rate Service tests
#![cfg(all(test, feature = "bluetooth-ble-heart-rate"))]

use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_heart_rate_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_HEART_RATE", 8910, json!({}), "Act as a heart rate monitor at 72 BPM").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
