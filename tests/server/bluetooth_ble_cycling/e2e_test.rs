//! End-to-end Bluetooth LE Cycling Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-cycling"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_cycling_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Cycling Service Startup ===");

    let prompt = "Act as a BLE cycling speed and cadence sensor. Create the Cycling Speed and Cadence Service (UUID: 00001816-0000-1000-8000-00805f9b34fb). Set speed to 25 km/h, cadence to 90 RPM. Advertise as 'NetGet-Cycling'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("cycling")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BluetoothBLE",
                            "instruction": "Create cycling service",
                            "startup_params": {
                                "device_name": "NetGet-Cycling",
                                "services": [
                                    {
                                        "uuid": "00001816-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a5b-0000-1000-8000-00805f9b34fb",
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

    println!("✓ Cycling service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
