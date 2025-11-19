//! End-to-end Bluetooth LE Gamepad Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-gamepad"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_gamepad_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Gamepad Service Startup ===");

    let prompt = "Act as a BLE gamepad. Create the Human Interface Device Service (UUID: 00001812-0000-1000-8000-00805f9b34fb) for gamepad with buttons and analog sticks. Advertise as 'NetGet-Gamepad'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("gamepad")
                    .and_instruction_containing("Human Interface Device")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BLUETOOTH_BLE_GAMEPAD",
                            "instruction": "Create gamepad HID service",
                            "startup_params": {
                                "device_name": "NetGet-Gamepad",
                                "services": [
                                    {
                                        "uuid": "00001812-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a4d-0000-1000-8000-00805f9b34fb",
                                                "properties": ["read", "notify"],
                                                "value": "00000000"
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

    println!("✓ Gamepad service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
