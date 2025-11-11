//! BLE Remote Control E2E tests

#![cfg(all(test, feature = "bluetooth-ble-remote"))]

use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Test remote control server startup only
///
/// This test does NOT require BLE hardware - it only validates:
/// 1. Server can start with bluetooth-ble-remote protocol
/// 2. Server doesn't crash during initialization
/// 3. Server accepts instruction without errors
///
/// This is the only test that runs in CI (not marked #[ignore]).
#[tokio::test]
async fn test_remote_server_startup() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_REMOTE",
        8890,
        json!({}),
        "Act as a BLE remote control",
    )
    .await?;

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Just validate server started without crashing
    assert!(ctx.server_id.is_some(), "Server should have started");

    // Cleanup
    ctx.cleanup().await?;
    Ok(())
}

/// Test play/pause button
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_play_pause_button() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_REMOTE",
        8891,
        json!({}),
        "Act as a Bluetooth remote. When connected, press play/pause",
    )
    .await?;

    // Wait for server to start and LLM to press button
    sleep(Duration::from_secs(2)).await;

    // In a real test with BLE hardware, we would:
    // 1. Connect to HID service (0x1812)
    // 2. Subscribe to HID report notifications
    // 3. Validate play/pause report (0x01 0x00)
    // 4. Validate release report (0x00 0x00)

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

/// Test volume controls
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_volume_controls() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_REMOTE",
        8892,
        json!({}),
        "Act as a Bluetooth remote. Press volume up, then volume down",
    )
    .await?;

    // Wait for server to start and LLM to press buttons
    sleep(Duration::from_secs(3)).await;

    // In a real test with BLE hardware, we would:
    // 1. Connect and subscribe to HID reports
    // 2. Validate volume up report (0x40 0x00 - bit 6)
    // 3. Validate volume down report (0x80 0x00 - bit 7)

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

/// Test track navigation
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_track_navigation() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_REMOTE",
        8893,
        json!({}),
        "Act as a Bluetooth remote. Press next track 3 times",
    )
    .await?;

    // Wait for server to start and LLM to press buttons
    sleep(Duration::from_secs(3)).await;

    // In a real test with BLE hardware, we would:
    // 1. Connect and subscribe to HID reports
    // 2. Validate 3 next_track reports (0x02 0x00 - bit 1)
    // 3. Each followed by release (0x00 0x00)

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}
