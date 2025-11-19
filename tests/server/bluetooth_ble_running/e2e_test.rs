//! End-to-end Bluetooth LE Running Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-running"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_running_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Running Service Startup ===");

    let prompt = "Act as a BLE running sensor. Create the Running Speed and Cadence Service (UUID: 00001814-0000-1000-8000-00805f9b34fb) with speed 10 km/h, cadence 170 steps/min. Advertise as 'NetGet-Running'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("running")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BLUETOOTH_BLE_RUNNING",
                            "instruction": "Create running service",
                            "startup_params": {
                                "device_name": "NetGet-Running",
                                "services": [
                                    {
                                        "uuid": "00001814-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a53-0000-1000-8000-00805f9b34fb",
                                                "properties": ["notify"],
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

    println!("✓ Running service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
