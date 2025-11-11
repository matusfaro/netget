#![cfg(all(test, feature = "bluetooth-ble-presenter"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_presenter_server_startup() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_PRESENTER",
        8940,
        json!({}),
        "Act as presenter",
    )
    .await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
