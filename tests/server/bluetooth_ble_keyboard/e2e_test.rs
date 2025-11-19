//! End-to-end Bluetooth LE Keyboard Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-keyboard"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_keyboard_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Keyboard Service Startup ===");

    let prompt = "Act as a BLE keyboard. Create the Human Interface Device Service (UUID: 00001812-0000-1000-8000-00805f9b34fb) for keyboard input. Advertise as 'NetGet-Keyboard'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("keyboard")
                    .and_instruction_containing("Human Interface Device")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BLUETOOTH_BLE_KEYBOARD",
                            "instruction": "Create keyboard HID service",
                            "startup_params": {
                                "device_name": "NetGet-Keyboard",
                                "services": [
                                    {
                                        "uuid": "00001812-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a4d-0000-1000-8000-00805f9b34fb",
                                                "properties": ["read", "notify"],
                                                "value": "0000000000000000"
                                            }
                                        ]
                                    }
                                ]
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            })
    ).await?;

    println!("✓ Keyboard service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
