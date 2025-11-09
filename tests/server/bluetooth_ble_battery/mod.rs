//! BLE Battery Service E2E tests

#![cfg(all(test, feature = "bluetooth-ble-battery"))]

use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_battery_server_startup() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BATTERY",
        8900,
        json!({"initial_level": 80}),
        "Act as a BLE battery service at 80%"
    ).await?;

    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_battery_level_update() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BATTERY",
        8901,
        json!({}),
        "Set battery to 75%, then drain by 10%"
    ).await?;

    sleep(Duration::from_secs(2)).await;
    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}
