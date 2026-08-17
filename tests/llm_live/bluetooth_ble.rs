//! Live-LLM Bluetooth LE suite (event-level).
//!
//! BLE has no socket transport to drive on this machine — a GATT peripheral
//! needs a radio, and on Linux a D-Bus/BlueZ stack. What the model owns is
//! transport-independent: an ATT read of a characteristic becomes a
//! `bluetooth_read_request` event, and the model must answer it with a
//! `respond_to_read` carrying a **correctly encoded GATT value**.
//!
//! Protocol facts these cases encode (src/server/bluetooth_ble/{actions,mod}.rs
//! and the profile dirs):
//! - the base owns the vocabulary; every profile except the beacon delegates
//!   it, so heart-rate and battery servers answer the *base's* events;
//! - `characteristic_uuid` arrives as the full 128-bit lowercase form;
//! - all values are hex strings: Heart Rate Measurement (0x2A37) is
//!   `[flags, bpm]` with flags 0x00 for uint8 BPM, so 72 BPM is `"0048"`;
//!   Battery Level (0x2A19) is a single byte, so 100% is `"64"`;
//! - the beacon protocol does NOT delegate: it sees only `beacon_started` and
//!   the four beacon actions, and an iBeacon carries uuid/major/minor.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// Decode a GATT value the model produced: hex, optional `0x`, optional
/// separators — anything a model plausibly emits for a byte string.
fn decode_gatt_hex(v: &serde_json::Value) -> Result<Vec<u8>, String> {
    let s = v
        .as_str()
        .ok_or_else(|| format!("value must be a hex string, got {}", v))?;
    let cleaned: String = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':' && *c != '-')
        .collect();
    if cleaned.is_empty() || cleaned.len() % 2 != 0 {
        return Err(format!("{:?} is not an even-length hex string", s));
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).map_err(|e| format!("{}: {}", s, e)))
        .collect()
}

const HEART_RATE_MEASUREMENT: &str = "00002a37-0000-1000-8000-00805f9b34fb";
const BATTERY_LEVEL: &str = "00002a19-0000-1000-8000-00805f9b34fb";

/// A GATT read of Heart Rate Measurement must be answered with a correctly
/// encoded 2-byte measurement: flags byte then the BPM the instruction names.
#[tokio::test]
async fn ble_heart_rate_read_encodes_bpm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BLUETOOTH_BLE_HEART_RATE",
        "You are a BLE heart rate monitor exposing the Heart Rate Service \
         (0x180D). The wearer's current heart rate is 72 BPM. The Heart Rate \
         Measurement characteristic (0x2A37) is encoded as a flags byte of \
         0x00 followed by the heart rate as a single byte.",
        "bluetooth_read_request",
        json!({ "characteristic_uuid": HEART_RATE_MEASUREMENT, "offset": 0 }),
    )
    .expect_action("respond_to_read")
    .check(ParamCheck::custom(
        "value",
        "decodes to [flags=0x00, bpm=72] per the Heart Rate Measurement format",
        |v| {
            let bytes = decode_gatt_hex(v)?;
            if bytes.len() != 2 {
                return Err(format!(
                    "expected a 2-byte measurement (flags + uint8 BPM), got {} byte(s): {:02x?}",
                    bytes.len(),
                    bytes
                ));
            }
            if bytes[0] & 0x01 != 0 {
                return Err(format!(
                    "flags bit 0 set means uint16 BPM, but only one BPM byte follows: {:02x?}",
                    bytes
                ));
            }
            if bytes[1] != 72 {
                return Err(format!(
                    "BPM byte must be 72 (0x48) as instructed, got {} (0x{:02x})",
                    bytes[1], bytes[1]
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// A GATT read of Battery Level must be answered with the single percentage
/// byte the profile defines.
#[tokio::test]
async fn ble_battery_read_encodes_percentage() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BLUETOOTH_BLE_BATTERY",
        "You are a BLE battery service (0x180F). The battery is currently at \
         100 percent. The Battery Level characteristic (0x2A19) is a single \
         byte holding the percentage.",
        "bluetooth_read_request",
        json!({ "characteristic_uuid": BATTERY_LEVEL, "offset": 0 }),
    )
    .expect_action("respond_to_read")
    .check(ParamCheck::custom(
        "value",
        "decodes to a single byte equal to 100 (0x64)",
        |v| {
            let bytes = decode_gatt_hex(v)?;
            if bytes.len() != 1 {
                return Err(format!(
                    "Battery Level is exactly one byte, got {} byte(s): {:02x?}",
                    bytes.len(),
                    bytes
                ));
            }
            if bytes[0] != 100 {
                return Err(format!(
                    "battery byte must be 100 (0x64) as instructed, got {} (0x{:02x})",
                    bytes[0], bytes[0]
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// On startup the model must lay out the right GATT service for the profile.
#[tokio::test]
async fn ble_startup_adds_heart_rate_service() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BLUETOOTH_BLE_HEART_RATE",
        "Act as a BLE heart rate monitor. Expose the standard Heart Rate \
         Service with its Heart Rate Measurement characteristic.",
        "bluetooth_ble_started",
        json!({
            "device_name": "NetGet-HeartRate",
            "instruction": "Act as a BLE heart rate monitor."
        }),
    )
    .expect_action("add_service")
    .check(ParamCheck::custom(
        "uuid",
        "is the Heart Rate Service UUID (0x180D)",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase();
            if s.contains("180d") {
                Ok(())
            } else {
                Err(format!(
                    "expected the assigned Heart Rate Service UUID 0x180D (short \
                     or 128-bit form), got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::custom(
        "characteristics",
        "includes the Heart Rate Measurement characteristic (0x2A37)",
        |v| {
            let list = v
                .as_array()
                .ok_or_else(|| format!("characteristics must be an array, got {}", v))?;
            let found = list.iter().any(|c| {
                c["uuid"]
                    .as_str()
                    .map(|u| u.to_lowercase().contains("2a37"))
                    .unwrap_or(false)
            });
            if found {
                Ok(())
            } else {
                Err(format!(
                    "no characteristic with UUID 0x2A37 — a heart rate service \
                     without the measurement characteristic is unusable: {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A subscription to the measurement characteristic should be answered with a
/// notification carrying a well-formed measurement.
#[tokio::test]
async fn ble_subscribe_sends_measurement_notification() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BLUETOOTH_BLE_HEART_RATE",
        "You are a BLE heart rate monitor reading 80 BPM. When a client \
         subscribes to the Heart Rate Measurement characteristic (0x2A37), \
         push a notification with the current measurement, encoded as a flags \
         byte of 0x00 followed by the BPM byte.",
        "bluetooth_subscribe",
        json!({ "characteristic_uuid": HEART_RATE_MEASUREMENT, "subscribed": true }),
    )
    .expect_action("send_notification")
    .check(ParamCheck::contains("characteristic_uuid", "2a37"))
    .check(ParamCheck::custom(
        "value",
        "decodes to [0x00, 80] — the instructed measurement",
        |v| {
            let bytes = decode_gatt_hex(v)?;
            if bytes.len() != 2 {
                return Err(format!(
                    "expected a 2-byte measurement, got {} byte(s): {:02x?}",
                    bytes.len(),
                    bytes
                ));
            }
            if bytes[1] != 80 {
                return Err(format!(
                    "BPM byte must be 80 (0x50) as instructed, got {} (0x{:02x})",
                    bytes[1], bytes[1]
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// The beacon protocol owns its own vocabulary (it does not delegate), and an
/// iBeacon frame is identified by proximity UUID + major/minor.
#[tokio::test]
async fn ble_beacon_starts_ibeacon_frame() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BLUETOOTH_BLE_BEACON",
        "Act as an iBeacon with proximity UUID \
         e2c56db5-dffb-48d2-b060-d0f5a71096e0, major 1 and minor 100.",
        "beacon_started",
        json!({
            "device_name": "NetGet-Beacon",
            "adapter": "hci0",
            "instruction": "Act as an iBeacon with proximity UUID \
                            e2c56db5-dffb-48d2-b060-d0f5a71096e0, major 1, minor 100."
        }),
    )
    .expect_action("start_ibeacon")
    .check(ParamCheck::contains("uuid", "e2c56db5"))
    .check(ParamCheck::equals("major", json!(1)))
    .check(ParamCheck::equals("minor", json!(100)))
    .run()
    .await
}
