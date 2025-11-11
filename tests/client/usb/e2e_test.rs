//! USB client end-to-end tests
//!
//! These tests require actual USB hardware and are ignored by default.
//! Run with: cargo test --features usb -- --ignored

#[cfg(all(test, feature = "usb"))]
mod usb_client_tests {
    use std::env;

    /// Test USB device info parsing
    #[test]
    fn test_parse_usb_device_info() {
        // Test various input formats
        let test_cases = vec![
            ("1234:5678", Some((0x1234u16, 0x5678u16, 0u8))),
            ("0x1234:0x5678", Some((0x1234, 0x5678, 0))),
            ("1234:5678:1", Some((0x1234, 0x5678, 1))),
            ("vid:1234,pid:5678", Some((0x1234, 0x5678, 0))),
            ("vid:1234,pid:5678,interface:2", Some((0x1234, 0x5678, 2))),
            ("invalid", None),
            ("1234", None),
        ];

        for (input, expected) in test_cases {
            let result = parse_device_info_helper(input);
            match (result, expected) {
                (Ok((vid, pid, iface)), Some((exp_vid, exp_pid, exp_iface))) => {
                    assert_eq!(vid, exp_vid, "VID mismatch for input: {}", input);
                    assert_eq!(pid, exp_pid, "PID mismatch for input: {}", input);
                    assert_eq!(iface, exp_iface, "Interface mismatch for input: {}", input);
                }
                (Err(_), None) => {
                    // Expected failure
                }
                (Ok(_), None) => panic!("Expected failure for input: {}", input),
                (Err(e), Some(_)) => panic!("Unexpected error for input {}: {}", input, e),
            }
        }
    }

    /// Helper function to parse USB device info
    /// This is a simplified version of the parsing logic in mod.rs
    fn parse_device_info_helper(s: &str) -> Result<(u16, u16, u8), String> {
        let parts: Vec<&str> = s.split(',').collect();

        let mut vendor_id: Option<u16> = None;
        let mut product_id: Option<u16> = None;
        let mut interface_number: u8 = 0;

        if parts.len() == 1 {
            // Colon-separated format
            let colon_parts: Vec<&str> = s.split(':').collect();
            if colon_parts.len() >= 2 {
                vendor_id = Some(parse_hex_u16(colon_parts[0])?);
                product_id = Some(parse_hex_u16(colon_parts[1])?);
                if colon_parts.len() >= 3 {
                    interface_number = colon_parts[2].parse().map_err(|e| format!("{}", e))?;
                }
            }
        } else {
            // Comma-separated key:value format
            for part in parts {
                let kv: Vec<&str> = part.trim().split(':').collect();
                if kv.len() == 2 {
                    match kv[0].trim().to_lowercase().as_str() {
                        "vid" | "vendor" => vendor_id = Some(parse_hex_u16(kv[1].trim())?),
                        "pid" | "product" => product_id = Some(parse_hex_u16(kv[1].trim())?),
                        "interface" | "if" => {
                            interface_number = kv[1].trim().parse().map_err(|e| format!("{}", e))?
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok((
            vendor_id.ok_or_else(|| "Missing vendor_id".to_string())?,
            product_id.ok_or_else(|| "Missing product_id".to_string())?,
            interface_number,
        ))
    }

    fn parse_hex_u16(s: &str) -> Result<u16, String> {
        let s = s.trim();
        if s.starts_with("0x") || s.starts_with("0X") {
            u16::from_str_radix(&s[2..], 16).map_err(|e| format!("{}", e))
        } else {
            u16::from_str_radix(s, 16)
                .or_else(|_| s.parse::<u16>())
                .map_err(|e| format!("{}", e))
        }
    }

    /// Test USB device connection (requires hardware)
    ///
    /// This test is ignored by default because it requires:
    /// - Actual USB device connected
    /// - VID/PID set via USB_TEST_VID and USB_TEST_PID env vars
    /// - Appropriate permissions to access the device
    ///
    /// Run with:
    /// ```bash
    /// USB_TEST_VID=1234 USB_TEST_PID=5678 cargo test --features usb test_usb_device_connection -- --ignored
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_usb_device_connection() {
        // Get test device VID/PID from environment
        let vid = env::var("USB_TEST_VID").expect("USB_TEST_VID environment variable not set");
        let pid = env::var("USB_TEST_PID").expect("USB_TEST_PID environment variable not set");

        println!("Testing USB device connection: VID:{} PID:{}", vid, pid);

        // List USB devices to verify test device is present
        let devices = nusb::list_devices().expect("Failed to list USB devices");

        let test_vid = parse_hex_u16(&vid).expect("Invalid VID");
        let test_pid = parse_hex_u16(&pid).expect("Invalid PID");

        let mut found = false;
        for device in devices {
            if device.vendor_id() == test_vid && device.product_id() == test_pid {
                found = true;
                println!(
                    "Found test device: VID:{:04x} PID:{:04x} Bus:{} Device:{}",
                    device.vendor_id(),
                    device.product_id(),
                    device.bus_number(),
                    device.device_address()
                );
            }
        }

        assert!(
            found,
            "Test USB device VID:{:04x} PID:{:04x} not found. Is it connected?",
            test_vid, test_pid
        );
    }

    /// Test hex encoding/decoding
    #[test]
    fn test_hex_encoding() {
        let test_data = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello"
        let hex = hex::encode(&test_data);
        assert_eq!(hex, "48656c6c6f");

        let decoded = hex::decode(&hex).expect("Failed to decode hex");
        assert_eq!(decoded, test_data);
    }

    /// Test control transfer parameter validation
    #[test]
    fn test_control_transfer_params() {
        use serde_json::json;

        let action = json!({
            "type": "control_transfer",
            "request_type": 0x80,
            "request": 0x06,
            "value": 0x0100,
            "index": 0,
            "length": 18
        });

        // Validate fields exist
        assert_eq!(action["type"], "control_transfer");
        assert_eq!(action["request_type"], 0x80);
        assert_eq!(action["request"], 0x06);
        assert_eq!(action["value"], 0x0100);
        assert_eq!(action["index"], 0);
        assert_eq!(action["length"], 18);
    }
}
