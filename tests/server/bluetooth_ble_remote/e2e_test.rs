//! End-to-end Bluetooth LE Remote Control Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-remote"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_remote_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Remote Control Service Startup ===");

    let prompt = "Act as a BLE remote control. Create HID service for media controls (play, pause, volume, track navigation). Advertise as 'NetGet-Remote'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("remote")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BluetoothBLE",
                            "instruction": "Create remote control HID service",
                            "startup_params": {
                                "device_name": "NetGet-Remote",
                                "services": [
                                    {
                                        "uuid": "00001812-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a4d-0000-1000-8000-00805f9b34fb",
                                                "properties": ["read", "notify"],
                                                "value": "00"
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

    println!("✓ Remote control service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
