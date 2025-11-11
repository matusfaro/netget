#![cfg(all(test, feature = "bluetooth-ble-file_transfer"))]
use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_file_transfer_server_startup() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_FILE_TRANSFER",
        8944,
        json!({}),
        "Act as file_transfer",
    )
    .await?;
    sleep(Duration::from_millis(500)).await;
    assert!(ctx.server_id.is_some());
    ctx.cleanup().await?;
    Ok(())
}
