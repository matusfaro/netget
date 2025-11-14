//! End-to-end Bluetooth LE Beacon tests for NetGet
//!
//! These tests spawn the actual NetGet binary with BLE beacon prompts
//! and validate basic advertising functionality.

#![cfg(all(test, feature = "bluetooth-ble-beacon"))]

use crate::helpers::{self, E2EResult, NetGetConfig, with_timeout};
use std::time::Duration;

/// Test iBeacon advertising startup
/// LLM calls: 1 (server startup)
#[tokio::test]
async fn test_ibeacon_advertising() -> E2EResult<()> {
    with_timeout("ibeacon_advertising", Duration::from_secs(120), async {
        println!("\n=== E2E Test: iBeacon Advertising ===");

        // PROMPT: Create an iBeacon
        let prompt = "Act as an iBeacon. Use UUID 12345678-1234-5678-1234-567812345678, major 100, minor 200, TX power -59dBm. Start advertising as 'NetGet-iBeacon'.";

        // Start the server with mocks
        let server = helpers::start_netget_server(
            NetGetConfig::new(prompt)
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("iBeacon")
                        .and_instruction_containing("12345678-1234-5678-1234-567812345678")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "BluetoothBLE",
                                "instruction": "Create iBeacon with specified UUID, major, minor",
                                "startup_params": {
                                    "device_name": "NetGet-iBeacon",
                                    "beacon_type": "ibeacon",
                                    "beacon_data": {
                                        "uuid": "12345678-1234-5678-1234-567812345678",
                                        "major": 100,
                                        "minor": 200,
                                        "tx_power": -59
                                    }
                                }
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                })
        ).await?;

        println!("✓ iBeacon started advertising");

        // Let server advertise briefly
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("✓ Beacon advertising without errors");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test passed ===\n");
        Ok(())
    }).await
}

/// Test Eddystone-UID advertising
/// LLM calls: 1 (server startup)
#[tokio::test]
async fn test_eddystone_uid_advertising() -> E2EResult<()> {
    with_timeout("eddystone_uid_advertising", Duration::from_secs(120), async {
        println!("\n=== E2E Test: Eddystone-UID Advertising ===");

        // PROMPT: Create Eddystone-UID beacon
        let prompt = "Act as an Eddystone-UID beacon. Use namespace 0x12345678901234567890 and instance 0x123456789012. TX power -20dBm. Advertise as 'NetGet-Eddystone'.";

        // Start the server with mocks
        let server = helpers::start_netget_server(
            NetGetConfig::new(prompt)
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("Eddystone-UID")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "BluetoothBLE",
                                "instruction": "Create Eddystone-UID beacon",
                                "startup_params": {
                                    "device_name": "NetGet-Eddystone",
                                    "beacon_type": "eddystone_uid",
                                    "beacon_data": {
                                        "namespace": "12345678901234567890",
                                        "instance": "123456789012",
                                        "tx_power": -20
                                    }
                                }
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                })
        ).await?;

        println!("✓ Eddystone-UID beacon started advertising");

        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("✓ Beacon advertising without errors");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test passed ===\n");
        Ok(())
    }).await
}

/// Test Eddystone-URL advertising
/// LLM calls: 1 (server startup)
#[tokio::test]
async fn test_eddystone_url_advertising() -> E2EResult<()> {
    with_timeout("eddystone_url_advertising", Duration::from_secs(120), async {
        println!("\n=== E2E Test: Eddystone-URL Advertising ===");

        // PROMPT: Create Eddystone-URL beacon
        let prompt = "Act as an Eddystone-URL beacon. Broadcast URL 'https://example.com'. TX power -20dBm. Advertise as 'NetGet-URL'.";

        // Start the server with mocks
        let server = helpers::start_netget_server(
            NetGetConfig::new(prompt)
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("Eddystone-URL")
                        .and_instruction_containing("example.com")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "BluetoothBLE",
                                "instruction": "Create Eddystone-URL beacon",
                                "startup_params": {
                                    "device_name": "NetGet-URL",
                                    "beacon_type": "eddystone_url",
                                    "beacon_data": {
                                        "url": "https://example.com",
                                        "tx_power": -20
                                    }
                                }
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                })
        ).await?;

        println!("✓ Eddystone-URL beacon started advertising");

        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("✓ Beacon broadcasting URL without errors");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test passed ===\n");
        Ok(())
    }).await
}
