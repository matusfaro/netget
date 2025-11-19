//! End-to-end Bluetooth LE Thermometer Service tests for NetGet

#![cfg(all(test, feature = "bluetooth-ble-thermometer"))]

use crate::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

/// Test thermometer service startup
/// LLM calls: 1 (server startup)
#[tokio::test]
async fn test_thermometer_service_startup() -> E2EResult<()> {
    println!("\n=== E2E Test: Thermometer Service Startup ===");

    let prompt = "Act as a BLE thermometer. Create the Health Thermometer Service (UUID: 00001809-0000-1000-8000-00805f9b34fb) with Temperature Measurement characteristic (UUID: 00002a1c-0000-1000-8000-00805f9b34fb). Set temperature to 36.6°C. Advertise as 'NetGet-Thermometer'.";

    let server = helpers::start_netget_server(
        NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("thermometer")
                    .and_instruction_containing("Health Thermometer Service")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BLUETOOTH_BLE_THERMOMETER",
                            "instruction": "Create thermometer service at 36.6°C",
                            "startup_params": {
                                "device_name": "NetGet-Thermometer",
                                "services": [
                                    {
                                        "uuid": "00001809-0000-1000-8000-00805f9b34fb",
                                        "characteristics": [
                                            {
                                                "uuid": "00002a1c-0000-1000-8000-00805f9b34fb",
                                                "properties": ["read", "indicate"],
                                                "value": "00E6000001FF"
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

    println!("✓ Thermometer service started");
    tokio::time::sleep(Duration::from_secs(2)).await;

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
