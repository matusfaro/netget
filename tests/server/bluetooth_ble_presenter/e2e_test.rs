//! End-to-end Bluetooth LE Presenter Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-presenter"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_presenter_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Presenter Service Startup ===");

    let prompt = "Act as a BLE presentation remote. Create HID service for presenter controls (next slide, previous slide, laser pointer). Advertise as 'NetGet-Presenter'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("presenter")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BLUETOOTH_BLE_PRESENTER",
                            "instruction": "Create presenter HID service",
                            "startup_params": {
                                "device_name": "NetGet-Presenter",
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

    println!("✓ Presenter service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
