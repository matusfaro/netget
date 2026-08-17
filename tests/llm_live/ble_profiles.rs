//! Live-LLM suite for the BLE profiles that wrap the base stack.
//!
//! All of them delegate their vocabulary to `bluetooth_ble` (five events, six
//! actions — see bluetooth_ble.rs), so what distinguishes one profile from
//! another is entirely **domain**: which GATT service it lays out, which
//! characteristic a client reads, and how that characteristic's value is
//! encoded. Those are the facts each case asserts, taken from the profile's
//! own `mod.rs` constants and `actions.rs` startup examples.
//!
//! Where a profile's measurement characteristic carries no documented example
//! bytes (true of cycling, running, weight-scale and thermometer, whose only
//! documented value belongs to the Feature/Type characteristic), the case
//! drives the characteristic that *is* specified rather than inventing an
//! encoding the tree never states.
//!
//! COVERS: bluetooth_ble_cycling: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_running: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_thermometer: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_weight_scale: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_environmental: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_proximity: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_presenter: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_gamepad: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_remote: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_mouse: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_data_stream: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_file_transfer: bluetooth_ble_started, bluetooth_write_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_read_request
//! COVERS: bluetooth_ble_battery: bluetooth_ble_started, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_heart_rate: bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble_keyboard: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request
//! COVERS: bluetooth_ble: bluetooth_ble_started, bluetooth_read_request, bluetooth_subscribe, bluetooth_state_changed, bluetooth_write_request

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// A GATT value as the model wrote it: hex, `0x`-tolerant, separator-tolerant.
fn gatt_bytes(v: &serde_json::Value) -> Result<Vec<u8>, String> {
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

/// Generates the five base-stack events for one profile, each asserting that
/// profile's own GATT facts.
///
/// `$service`/`$characteristic` are the profile's assigned UUIDs; `$value` is
/// the byte string the tree documents for that characteristic; `$role`
/// describes what the characteristic is, so the instruction reads like the
/// operator's, not like a restatement of the assertion.
macro_rules! ble_profile_suite {
    (
        module: $module:ident,
        protocol: $protocol:literal,
        persona: $persona:literal,
        service_uuid_fragment: $service_frag:literal,
        characteristic: $characteristic:literal,
        characteristic_fragment: $char_frag:literal,
        role: $role:literal,
        value_hex: $value:literal,
        value_meaning: $meaning:literal,
    ) => {
        mod $module {
            use super::*;

            /// Startup: lay out the profile's assigned GATT service.
            #[tokio::test]
            async fn started_adds_the_profile_service() -> E2EResult<()> {
                if !live_llm_enabled() {
                    return Ok(());
                }
                EventCase::new(
                    $protocol,
                    concat!($persona, " Lay out the GATT service this profile is defined by."),
                    "bluetooth_ble_started",
                    json!({
                        "device_name": "NetGet-Live",
                        "instruction": $persona
                    }),
                )
                .expect_action("add_service")
                .check(ParamCheck::custom(
                    "uuid",
                    concat!("is the profile's assigned service UUID (", $service_frag, ")"),
                    |v| {
                        let s = v.as_str().unwrap_or("").to_lowercase();
                        if s.contains($service_frag) {
                            Ok(())
                        } else {
                            Err(format!(
                                concat!("expected the assigned service UUID 0x", $service_frag, ", got {:?}"),
                                v
                            ))
                        }
                    },
                ))
                .run()
                .await
            }

            /// A client reads the profile's specified characteristic.
            #[tokio::test]
            async fn read_returns_the_specified_value() -> E2EResult<()> {
                if !live_llm_enabled() {
                    return Ok(());
                }
                EventCase::new(
                    $protocol,
                    concat!(
                        $persona,
                        " The ", $role, " characteristic currently holds ", $meaning,
                        ", which is encoded as the bytes ", $value, "."
                    ),
                    "bluetooth_read_request",
                    json!({ "characteristic_uuid": $characteristic, "offset": 0 }),
                )
                .expect_action("respond_to_read")
                .check(ParamCheck::custom(
                    "value",
                    concat!("decodes to the specified bytes ", $value),
                    |v| {
                        let got = gatt_bytes(v)?;
                        let want = gatt_bytes(&json!($value))?;
                        if got == want {
                            Ok(())
                        } else {
                            Err(format!(
                                "expected {:02x?} (the value the profile specifies), got {:02x?}",
                                want, got
                            ))
                        }
                    },
                ))
                .run()
                .await
            }

            /// A client subscribed: push the value as a notification on the
            /// characteristic it subscribed to.
            #[tokio::test]
            async fn subscribe_notifies_that_characteristic() -> E2EResult<()> {
                if !live_llm_enabled() {
                    return Ok(());
                }
                EventCase::new(
                    $protocol,
                    concat!(
                        $persona,
                        " When a client subscribes to a characteristic, push it the current \
                         value straight away. The ", $role, " characteristic holds ", $meaning,
                        ", encoded as the bytes ", $value, "."
                    ),
                    "bluetooth_subscribe",
                    json!({ "characteristic_uuid": $characteristic, "subscribed": true }),
                )
                .expect_action("send_notification")
                .check(ParamCheck::contains("characteristic_uuid", $char_frag))
                .check(ParamCheck::custom("value", "is a decodable GATT byte string", |v| {
                    gatt_bytes(v).map(|_| ())
                }))
                .run()
                .await
            }

            /// The adapter powered on: a peripheral nobody can see is useless,
            /// so the answer is to advertise.
            #[tokio::test]
            async fn adapter_powered_on_starts_advertising() -> E2EResult<()> {
                if !live_llm_enabled() {
                    return Ok(());
                }
                EventCase::new(
                    $protocol,
                    concat!(
                        $persona,
                        " Whenever the Bluetooth adapter comes up, begin advertising so a \
                         central can discover and connect to you."
                    ),
                    "bluetooth_state_changed",
                    json!({ "state": "powered_on" }),
                )
                .expect_action("start_advertising")
                .run()
                .await
            }

            /// A client wrote to a characteristic: acknowledge the write.
            #[tokio::test]
            async fn write_is_acknowledged() -> E2EResult<()> {
                if !live_llm_enabled() {
                    return Ok(());
                }
                EventCase::new(
                    $protocol,
                    concat!(
                        $persona,
                        " Accept what a client writes to your characteristics and acknowledge \
                         the write, so the client knows it landed."
                    ),
                    "bluetooth_write_request",
                    json!({
                        "characteristic_uuid": $characteristic,
                        "value": "01",
                        "offset": 0
                    }),
                )
                .expect_action("respond_to_write")
                .run()
                .await
            }
        }
    };
}

// Cycling Speed and Cadence (0x1816). Documented value belongs to CSC Feature
// (0x2A5C): "0300" = feature bitfield 0x0003, little-endian.
ble_profile_suite! {
    module: cycling,
    protocol: "BLUETOOTH_BLE_CYCLING",
    persona: "You are a BLE Cycling Speed and Cadence sensor (service 0x1816).",
    service_uuid_fragment: "1816",
    characteristic: "00002a5c-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a5c",
    role: "CSC Feature (0x2A5C)",
    value_hex: "0300",
    value_meaning: "the feature bitfield 0x0003 in little-endian order",
}

// Running Speed and Cadence (0x1814); RSC Feature (0x2A54) = "0300".
ble_profile_suite! {
    module: running,
    protocol: "BLUETOOTH_BLE_RUNNING",
    persona: "You are a BLE Running Speed and Cadence sensor (service 0x1814).",
    service_uuid_fragment: "1814",
    characteristic: "00002a54-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a54",
    role: "RSC Feature (0x2A54)",
    value_hex: "0300",
    value_meaning: "the feature bitfield 0x0003 in little-endian order",
}

// Health Thermometer (0x1809); Temperature Type (0x2A1D) = "02" (Body).
ble_profile_suite! {
    module: thermometer,
    protocol: "BLUETOOTH_BLE_THERMOMETER",
    persona: "You are a BLE Health Thermometer (service 0x1809).",
    service_uuid_fragment: "1809",
    characteristic: "00002a1d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a1d",
    role: "Temperature Type (0x2A1D)",
    value_hex: "02",
    value_meaning: "temperature type 2",
}

// Weight Scale (0x181D); Weight Scale Feature (0x2A9E) = "01000000".
ble_profile_suite! {
    module: weight_scale,
    protocol: "BLUETOOTH_BLE_WEIGHT_SCALE",
    persona: "You are a BLE Weight Scale (service 0x181D).",
    service_uuid_fragment: "181d",
    characteristic: "00002a9e-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a9e",
    role: "Weight Scale Feature (0x2A9E)",
    value_hex: "01000000",
    value_meaning: "the feature bitfield 0x00000001 in little-endian order",
}

// Environmental Sensing (0x181A). The one profile whose measurement value is
// fully specified: Temperature (0x2A6E) is int16 LE in 0.01 C, so 21.54 C is
// 2154 = 0x086A, written little-endian as "6a08".
ble_profile_suite! {
    module: environmental,
    protocol: "BLUETOOTH_BLE_ENVIRONMENTAL",
    persona: "You are a BLE Environmental Sensing peripheral (service 0x181A).",
    service_uuid_fragment: "181a",
    characteristic: "00002a6e-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a6e",
    role: "Temperature (0x2A6E)",
    value_hex: "6a08",
    value_meaning: "21.54 degrees Celsius, as a signed 16-bit value in units of 0.01 C, little-endian",
}

// Proximity: Alert Level (0x2A06), single byte, "00" = No Alert.
ble_profile_suite! {
    module: proximity,
    protocol: "BLUETOOTH_BLE_PROXIMITY",
    persona: "You are a BLE Proximity peripheral (Immediate Alert 0x1802, Link Loss 0x1803).",
    service_uuid_fragment: "180",
    characteristic: "00002a06-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a06",
    role: "Alert Level (0x2A06)",
    value_hex: "00",
    value_meaning: "alert level 0, meaning no alert",
}

// HID presenter: 8-byte keyboard input report, idle = all zeroes.
ble_profile_suite! {
    module: presenter,
    protocol: "BLUETOOTH_BLE_PRESENTER",
    persona: "You are a BLE HID presentation clicker (HID service 0x1812).",
    service_uuid_fragment: "1812",
    characteristic: "00002a4d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a4d",
    role: "HID Report (0x2A4D)",
    value_hex: "0000000000000000",
    value_meaning: "the idle keyboard report, eight zero bytes with no key held",
}

// HID gamepad: 2-byte button bitfield, idle = "0000".
ble_profile_suite! {
    module: gamepad,
    protocol: "BLUETOOTH_BLE_GAMEPAD",
    persona: "You are a BLE HID gamepad (HID service 0x1812).",
    service_uuid_fragment: "1812",
    characteristic: "00002a4d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a4d",
    role: "HID Report (0x2A4D)",
    value_hex: "0000",
    value_meaning: "the idle report, sixteen button bits all clear",
}

// HID consumer-control remote: actions.rs ships a 1-byte report, idle "00".
ble_profile_suite! {
    module: remote,
    protocol: "BLUETOOTH_BLE_REMOTE",
    persona: "You are a BLE HID consumer-control remote (HID service 0x1812).",
    service_uuid_fragment: "1812",
    characteristic: "00002a4d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a4d",
    role: "HID Report (0x2A4D)",
    value_hex: "00",
    value_meaning: "the idle report, no button held",
}

// HID mouse: actions.rs ships a 3-byte report [buttons, dX, dY], idle
// "000000".
ble_profile_suite! {
    module: mouse,
    protocol: "BLUETOOTH_BLE_MOUSE",
    persona: "You are a BLE HID mouse (HID service 0x1812).",
    service_uuid_fragment: "1812",
    characteristic: "00002a4d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a4d",
    role: "HID Report (0x2A4D)",
    value_hex: "000000",
    value_meaning: "the idle report: no button held and no movement",
}

// Custom streaming service 0xF00D; the control characteristic 0xF00F is the
// readable one.
ble_profile_suite! {
    module: data_stream,
    protocol: "BLUETOOTH_BLE_DATA_STREAM",
    persona: "You are a BLE data-streaming peripheral (custom service 0xF00D).",
    service_uuid_fragment: "f00d",
    characteristic: "0000f00f-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "f00f",
    role: "stream control (0xF00F)",
    value_hex: "01",
    value_meaning: "control byte 1, meaning streaming is enabled",
}

// Nordic DFU-style file transfer service 0xFE59.
ble_profile_suite! {
    module: file_transfer,
    protocol: "BLUETOOTH_BLE_FILE_TRANSFER",
    persona: "You are a BLE file-transfer peripheral (service 0xFE59).",
    service_uuid_fragment: "fe59",
    characteristic: "00008ec9-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "8ec9",
    role: "transfer control point (0x8EC9)",
    value_hex: "00",
    value_meaning: "control byte 0, meaning idle",
}

// The base stack itself, driven as a Heart Rate peripheral (its own
// example_prompt): base events, base actions, standard 0x180D layout.
ble_profile_suite! {
    module: base_stack,
    protocol: "BLUETOOTH_BLE",
    persona: "You are a BLE GATT peripheral exposing the Heart Rate Service (0x180D).",
    service_uuid_fragment: "180d",
    characteristic: "00002a38-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a38",
    role: "Body Sensor Location (0x2A38)",
    value_hex: "01",
    value_meaning: "sensor location 1, meaning the chest",
}

// Keyboard and battery already have their own domain cases in
// bluetooth_ble.rs; these fill in their remaining base events.
ble_profile_suite! {
    module: keyboard,
    protocol: "BLUETOOTH_BLE_KEYBOARD",
    persona: "You are a BLE HID keyboard (HID service 0x1812).",
    service_uuid_fragment: "1812",
    characteristic: "00002a4d-0000-1000-8000-00805f9b34fb",
    characteristic_fragment: "2a4d",
    role: "HID Report (0x2A4D)",
    value_hex: "0000000000000000",
    value_meaning: "the idle keyboard report, eight zero bytes with no key held",
}

/// Battery: the remaining base events (its read case lives in
/// bluetooth_ble.rs, which asserts the single 0-100 byte).
mod battery {
    use super::*;

    #[tokio::test]
    async fn started_adds_the_battery_service() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_BATTERY",
            "You are a BLE Battery Service peripheral (0x180F). Lay out the \
             GATT service this profile is defined by, with its Battery Level \
             characteristic.",
            "bluetooth_ble_started",
            json!({
                "device_name": "NetGet-Battery",
                "instruction": "Act as a Bluetooth battery reporting 100%."
            }),
        )
        .expect_action("add_service")
        .check(ParamCheck::custom(
            "uuid",
            "is the Battery Service UUID (0x180F)",
            |v| {
                let s = v.as_str().unwrap_or("").to_lowercase();
                if s.contains("180f") {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the Battery Service UUID 0x180F, got {:?}",
                        v
                    ))
                }
            },
        ))
        .run()
        .await
    }

    #[tokio::test]
    async fn subscribe_notifies_battery_level() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_BATTERY",
            "You are a BLE battery service currently at 90 percent. When a \
             client subscribes to the Battery Level characteristic (0x2A19), \
             push the current level, which is a single byte holding the \
             percentage.",
            "bluetooth_subscribe",
            json!({
                "characteristic_uuid": "00002a19-0000-1000-8000-00805f9b34fb",
                "subscribed": true
            }),
        )
        .expect_action("send_notification")
        .check(ParamCheck::contains("characteristic_uuid", "2a19"))
        .check(ParamCheck::custom(
            "value",
            "is a single byte equal to 90 (0x5A)",
            |v| {
                let bytes = gatt_bytes(v)?;
                if bytes.len() != 1 {
                    return Err(format!(
                        "Battery Level is exactly one byte, got {} byte(s): {:02x?}",
                        bytes.len(),
                        bytes
                    ));
                }
                if bytes[0] != 90 {
                    return Err(format!(
                        "expected 90 (0x5A) as instructed, got {} (0x{:02x})",
                        bytes[0], bytes[0]
                    ));
                }
                Ok(())
            },
        ))
        .run()
        .await
    }

    #[tokio::test]
    async fn adapter_powered_on_starts_advertising() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_BATTERY",
            "You are a BLE battery service. Whenever the Bluetooth adapter \
             comes up, begin advertising so a central can discover you.",
            "bluetooth_state_changed",
            json!({ "state": "powered_on" }),
        )
        .expect_action("start_advertising")
        .run()
        .await
    }

    #[tokio::test]
    async fn write_is_acknowledged() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_BATTERY",
            "You are a BLE battery service. Accept what a client writes to \
             your characteristics and acknowledge the write.",
            "bluetooth_write_request",
            json!({
                "characteristic_uuid": "00002a19-0000-1000-8000-00805f9b34fb",
                "value": "5a",
                "offset": 0
            }),
        )
        .expect_action("respond_to_write")
        .run()
        .await
    }
}

/// Heart rate: the two base events its own domain file does not cover.
mod heart_rate {
    use super::*;

    #[tokio::test]
    async fn adapter_powered_on_starts_advertising() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_HEART_RATE",
            "You are a BLE heart rate monitor. Whenever the Bluetooth adapter \
             comes up, begin advertising so a fitness app can find you.",
            "bluetooth_state_changed",
            json!({ "state": "powered_on" }),
        )
        .expect_action("start_advertising")
        .run()
        .await
    }

    #[tokio::test]
    async fn write_is_acknowledged() -> E2EResult<()> {
        if !live_llm_enabled() {
            return Ok(());
        }
        EventCase::new(
            "BLUETOOTH_BLE_HEART_RATE",
            "You are a BLE heart rate monitor. A client may write to the Heart \
             Rate Control Point; accept the write and acknowledge it.",
            "bluetooth_write_request",
            json!({
                "characteristic_uuid": "00002a39-0000-1000-8000-00805f9b34fb",
                "value": "01",
                "offset": 0
            }),
        )
        .expect_action("respond_to_write")
        .run()
        .await
    }
}
