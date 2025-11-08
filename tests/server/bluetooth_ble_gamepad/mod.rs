#![cfg(all(test, feature = "bluetooth-ble-gamepad"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_gamepad_server_startup() -> Result<()> {
    let mut ctx = setup_test_server("BLUETOOTH_BLE_GAMEPAD", 8938, json!({}), "Act as gamepad").await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
