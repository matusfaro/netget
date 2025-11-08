#![cfg(all(test, feature = "bluetooth-ble-weight-scale"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_weight_scale_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_WEIGHT_SCALE", 8970, json!({}), "Weight scale at 70 kg").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
