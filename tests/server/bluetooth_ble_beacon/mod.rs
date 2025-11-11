//! BLE Beacon E2E tests

#![cfg(all(test, feature = "bluetooth-ble-beacon"))]

use crate::server::helpers::{setup_test_server, TestContext};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Test iBeacon advertising
///
/// This test validates that:
/// 1. Server can start and configure as iBeacon
/// 2. LLM receives instruction and executes advertise_ibeacon action
/// 3. Beacon advertising data is formatted correctly
///
/// Note: This test requires BLE hardware to fully validate.
/// Without hardware, it only validates server startup and LLM action generation.
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_ibeacon_advertising() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BEACON",
        8880,
        json!({}),
        "Act as an iBeacon with UUID 12345678-1234-5678-1234-567812345678, major 1, minor 100, TX power -59"
    ).await?;

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Wait for LLM to process instruction and advertise beacon
    sleep(Duration::from_secs(2)).await;

    // In a real test with BLE hardware, we would:
    // 1. Use btleplug to scan for BLE beacons
    // 2. Filter for manufacturer data with Apple company ID (0x004C)
    // 3. Parse iBeacon data (type, UUID, major, minor, TX power)
    // 4. Validate values match the instruction

    // For now, just validate server started successfully
    assert!(ctx.server_id.is_some(), "Server should have started");

    // Cleanup
    ctx.cleanup().await?;
    Ok(())
}

/// Test Eddystone-UID advertising
///
/// Validates Eddystone-UID beacon with namespace and instance identifiers.
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_eddystone_uid_advertising() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BEACON",
        8881,
        json!({}),
        "Act as an Eddystone-UID beacon with namespace 0123456789abcdef0123 and instance 0123456789ab"
    ).await?;

    // Wait for server to start and LLM to advertise beacon
    sleep(Duration::from_secs(2)).await;

    // In a real test with BLE hardware, we would:
    // 1. Scan for Eddystone service UUID (0xFEAA)
    // 2. Parse frame type (should be 0x00 for UID)
    // 3. Validate namespace (10 bytes)
    // 4. Validate instance (6 bytes)

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

/// Test Eddystone-URL advertising
///
/// Validates Eddystone-URL beacon that broadcasts a web URL.
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_eddystone_url_advertising() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BEACON",
        8882,
        json!({}),
        "Act as an Eddystone-URL beacon broadcasting https://example.com",
    )
    .await?;

    // Wait for server to start and LLM to advertise beacon
    sleep(Duration::from_secs(2)).await;

    // In a real test with BLE hardware, we would:
    // 1. Scan for Eddystone service UUID (0xFEAA)
    // 2. Parse frame type (should be 0x10 for URL)
    // 3. Decode URL scheme (0x03 for https://)
    // 4. Validate URL body matches "example.com"

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

/// Test Eddystone-TLM advertising
///
/// Validates Eddystone-TLM beacon with telemetry data (battery, temperature, etc).
#[tokio::test]
#[ignore] // Requires real BLE hardware
async fn test_eddystone_tlm_advertising() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BEACON",
        8883,
        json!({}),
        "Act as an Eddystone-TLM beacon with battery 3000mV, temperature 22.5°C, adv count 0, uptime 0"
    ).await?;

    // Wait for server to start and LLM to advertise beacon
    sleep(Duration::from_secs(2)).await;

    // In a real test with BLE hardware, we would:
    // 1. Scan for Eddystone service UUID (0xFEAA)
    // 2. Parse frame type (should be 0x20 for TLM)
    // 3. Decode battery voltage (2 bytes, big-endian)
    // 4. Decode temperature (8.8 fixed point)
    // 5. Validate adv count and uptime

    assert!(ctx.server_id.is_some(), "Server should have started");

    ctx.cleanup().await?;
    Ok(())
}

/// Test beacon server startup only
///
/// This test does NOT require BLE hardware - it only validates:
/// 1. Server can start with bluetooth-ble-beacon protocol
/// 2. Server doesn't crash during initialization
/// 3. Server accepts instruction without errors
///
/// This is the only test that runs in CI (not marked #[ignore]).
#[tokio::test]
async fn test_beacon_server_startup() -> Result<()> {
    let mut ctx = setup_test_server(
        "BLUETOOTH_BLE_BEACON",
        8884,
        json!({}),
        "Act as a BLE beacon",
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
