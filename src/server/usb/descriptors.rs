//! USB descriptor builders for various device types
//!
//! This module provides functions to build USB descriptors for:
//! - HID devices (keyboard, mouse)
//! - CDC ACM devices (serial)
//! - Custom devices

#[cfg(feature = "usb-common")]
use crate::server::usb::common::*;

/// Standard USB device descriptor (18 bytes)
#[cfg(feature = "usb-common")]
pub fn build_device_descriptor(
    vendor_id: u16,
    product_id: u16,
    device_class: u8,
    device_subclass: u8,
    device_protocol: u8,
    manufacturer_str_index: u8,
    product_str_index: u8,
    serial_str_index: u8,
) -> Vec<u8> {
    vec![
        18,   // bLength
        descriptor_type::DEVICE, // bDescriptorType
        0x00, 0x02, // bcdUSB (USB 2.0)
        device_class, // bDeviceClass
        device_subclass, // bDeviceSubClass
        device_protocol, // bDeviceProtocol
        64,   // bMaxPacketSize0 (EP0)
        (vendor_id & 0xff) as u8, // idVendor (low byte)
        (vendor_id >> 8) as u8,   // idVendor (high byte)
        (product_id & 0xff) as u8, // idProduct (low byte)
        (product_id >> 8) as u8,   // idProduct (high byte)
        0x00, 0x01, // bcdDevice (1.0)
        manufacturer_str_index, // iManufacturer
        product_str_index,      // iProduct
        serial_str_index,       // iSerialNumber
        1,    // bNumConfigurations
    ]
}

/// HID keyboard report descriptor (boot protocol compatible)
/// Defines the format of keyboard input reports:
/// - Byte 0: Modifier keys (Ctrl, Shift, Alt, GUI)
/// - Byte 1: Reserved
/// - Bytes 2-7: Up to 6 simultaneous key presses
#[cfg(feature = "usb-keyboard")]
pub fn build_hid_keyboard_report_descriptor() -> Vec<u8> {
    vec![
        0x05, 0x01, // Usage Page (Generic Desktop)
        0x09, 0x06, // Usage (Keyboard)
        0xA1, 0x01, // Collection (Application)

        // Modifier keys (byte 0)
        0x05, 0x07, //   Usage Page (Key Codes)
        0x19, 0xE0, //   Usage Minimum (224) - Left Control
        0x29, 0xE7, //   Usage Maximum (231) - Right GUI
        0x15, 0x00, //   Logical Minimum (0)
        0x25, 0x01, //   Logical Maximum (1)
        0x75, 0x01, //   Report Size (1 bit)
        0x95, 0x08, //   Report Count (8 bits = 8 modifier keys)
        0x81, 0x02, //   Input (Data, Variable, Absolute) - Modifier byte

        // Reserved byte (byte 1)
        0x95, 0x01, //   Report Count (1)
        0x75, 0x08, //   Report Size (8 bits)
        0x81, 0x01, //   Input (Constant) - Reserved byte

        // LED output report (Num Lock, Caps Lock, Scroll Lock, etc.)
        0x95, 0x05, //   Report Count (5)
        0x75, 0x01, //   Report Size (1 bit)
        0x05, 0x08, //   Usage Page (LEDs)
        0x19, 0x01, //   Usage Minimum (1) - Num Lock
        0x29, 0x05, //   Usage Maximum (5) - Kana
        0x91, 0x02, //   Output (Data, Variable, Absolute) - LED bits

        // LED padding (3 bits)
        0x95, 0x01, //   Report Count (1)
        0x75, 0x03, //   Report Size (3 bits)
        0x91, 0x01, //   Output (Constant) - Padding

        // Key array (bytes 2-7)
        0x95, 0x06, //   Report Count (6) - Up to 6 simultaneous keys
        0x75, 0x08, //   Report Size (8 bits)
        0x15, 0x00, //   Logical Minimum (0)
        0x25, 0x65, //   Logical Maximum (101) - Max key code
        0x05, 0x07, //   Usage Page (Key Codes)
        0x19, 0x00, //   Usage Minimum (0)
        0x29, 0x65, //   Usage Maximum (101)
        0x81, 0x00, //   Input (Data, Array) - Key array

        0xC0, // End Collection
    ]
}

/// Build a complete HID keyboard configuration descriptor
/// Includes: Configuration, Interface, HID, and Endpoint descriptors
#[cfg(feature = "usb-keyboard")]
pub fn build_hid_keyboard_config_descriptor() -> Vec<u8> {
    let mut desc = Vec::new();

    // Configuration descriptor (9 bytes)
    desc.extend_from_slice(&[
        9,    // bLength
        descriptor_type::CONFIGURATION, // bDescriptorType
        34, 0, // wTotalLength (34 bytes total) - will be calculated
        1,    // bNumInterfaces
        1,    // bConfigurationValue
        0,    // iConfiguration (no string)
        0xA0, // bmAttributes (bus powered, remote wakeup)
        50,   // bMaxPower (100mA)
    ]);

    // Interface descriptor (9 bytes)
    desc.extend_from_slice(&[
        9,    // bLength
        descriptor_type::INTERFACE, // bDescriptorType
        0,    // bInterfaceNumber
        0,    // bAlternateSetting
        1,    // bNumEndpoints (1 interrupt IN endpoint)
        device_class::HID, // bInterfaceClass (HID)
        1,    // bInterfaceSubClass (Boot Interface)
        1,    // bInterfaceProtocol (Keyboard)
        0,    // iInterface (no string)
    ]);

    // HID descriptor (9 bytes)
    let report_desc = build_hid_keyboard_report_descriptor();
    let report_desc_len = report_desc.len() as u16;
    desc.extend_from_slice(&[
        9,    // bLength
        descriptor_type::HID, // bDescriptorType (HID)
        0x11, 0x01, // bcdHID (HID 1.11)
        0x00, // bCountryCode (not localized)
        0x01, // bNumDescriptors
        descriptor_type::HID_REPORT, // bDescriptorType (Report)
        (report_desc_len & 0xff) as u8, // wDescriptorLength (low)
        (report_desc_len >> 8) as u8,   // wDescriptorLength (high)
    ]);

    // Endpoint descriptor (7 bytes) - Interrupt IN
    desc.extend_from_slice(&[
        7,    // bLength
        descriptor_type::ENDPOINT, // bDescriptorType
        0x81, // bEndpointAddress (EP1 IN)
        transfer_type::INTERRUPT, // bmAttributes (Interrupt)
        0x08, 0x00, // wMaxPacketSize (8 bytes)
        10,   // bInterval (10ms polling interval)
    ]);

    // Update total length
    let total_len = desc.len() as u16;
    desc[2] = (total_len & 0xff) as u8;
    desc[3] = (total_len >> 8) as u8;

    desc
}

/// Build a USB string descriptor
/// String descriptors are UTF-16LE encoded with a 2-byte header
#[cfg(feature = "usb-common")]
pub fn build_string_descriptor(s: &str) -> Vec<u8> {
    let mut desc = Vec::new();

    // Encode string as UTF-16LE
    let utf16: Vec<u16> = s.encode_utf16().collect();
    let len = 2 + utf16.len() * 2;

    desc.push(len as u8); // bLength
    desc.push(descriptor_type::STRING); // bDescriptorType

    // Add UTF-16LE encoded characters
    for ch in utf16 {
        desc.push((ch & 0xff) as u8);
        desc.push((ch >> 8) as u8);
    }

    desc
}

/// Build language ID string descriptor (string index 0)
/// US English (0x0409) is the most common
#[cfg(feature = "usb-common")]
pub fn build_language_id_descriptor() -> Vec<u8> {
    vec![
        4,    // bLength
        descriptor_type::STRING, // bDescriptorType
        0x09, 0x04, // wLANGID[0] (US English)
    ]
}

/// HID keyboard input report builder
/// 8 bytes: [modifiers, reserved, key1, key2, key3, key4, key5, key6]
#[cfg(feature = "usb-keyboard")]
pub struct KeyboardReport {
    pub modifiers: u8,  // Bit flags for Ctrl, Shift, Alt, GUI
    pub keys: [u8; 6],  // Up to 6 simultaneous key presses
}

#[cfg(feature = "usb-keyboard")]
impl KeyboardReport {
    /// Create empty keyboard report (no keys pressed)
    pub fn new() -> Self {
        Self {
            modifiers: 0,
            keys: [0; 6],
        }
    }

    /// Convert to 8-byte array for USB transmission
    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.modifiers,
            0, // Reserved
            self.keys[0],
            self.keys[1],
            self.keys[2],
            self.keys[3],
            self.keys[4],
            self.keys[5],
        ]
    }
}

#[cfg(feature = "usb-keyboard")]
impl Default for KeyboardReport {
    fn default() -> Self {
        Self::new()
    }
}

/// HID keyboard modifier key bit positions
#[cfg(feature = "usb-keyboard")]
pub mod keyboard_modifiers {
    pub const LEFT_CTRL: u8 = 0x01;
    pub const LEFT_SHIFT: u8 = 0x02;
    pub const LEFT_ALT: u8 = 0x04;
    pub const LEFT_GUI: u8 = 0x08;
    pub const RIGHT_CTRL: u8 = 0x10;
    pub const RIGHT_SHIFT: u8 = 0x20;
    pub const RIGHT_ALT: u8 = 0x40;
    pub const RIGHT_GUI: u8 = 0x80;
}

/// HID keyboard usage codes (scan codes)
/// These are the values that go in the key array (bytes 2-7)
#[cfg(feature = "usb-keyboard")]
pub mod keyboard_usage {
    pub const NONE: u8 = 0x00;
    pub const ERROR_ROLLOVER: u8 = 0x01;
    pub const A: u8 = 0x04;
    pub const B: u8 = 0x05;
    pub const C: u8 = 0x06;
    pub const D: u8 = 0x07;
    pub const E: u8 = 0x08;
    pub const F: u8 = 0x09;
    pub const G: u8 = 0x0a;
    pub const H: u8 = 0x0b;
    pub const I: u8 = 0x0c;
    pub const J: u8 = 0x0d;
    pub const K: u8 = 0x0e;
    pub const L: u8 = 0x0f;
    pub const M: u8 = 0x10;
    pub const N: u8 = 0x11;
    pub const O: u8 = 0x12;
    pub const P: u8 = 0x13;
    pub const Q: u8 = 0x14;
    pub const R: u8 = 0x15;
    pub const S: u8 = 0x16;
    pub const T: u8 = 0x17;
    pub const U: u8 = 0x18;
    pub const V: u8 = 0x19;
    pub const W: u8 = 0x1a;
    pub const X: u8 = 0x1b;
    pub const Y: u8 = 0x1c;
    pub const Z: u8 = 0x1d;
    pub const NUM_1: u8 = 0x1e;
    pub const NUM_2: u8 = 0x1f;
    pub const NUM_3: u8 = 0x20;
    pub const NUM_4: u8 = 0x21;
    pub const NUM_5: u8 = 0x22;
    pub const NUM_6: u8 = 0x23;
    pub const NUM_7: u8 = 0x24;
    pub const NUM_8: u8 = 0x25;
    pub const NUM_9: u8 = 0x26;
    pub const NUM_0: u8 = 0x27;
    pub const ENTER: u8 = 0x28;
    pub const ESCAPE: u8 = 0x29;
    pub const BACKSPACE: u8 = 0x2a;
    pub const TAB: u8 = 0x2b;
    pub const SPACE: u8 = 0x2c;
    pub const MINUS: u8 = 0x2d;
    pub const EQUALS: u8 = 0x2e;
    pub const LEFT_BRACKET: u8 = 0x2f;
    pub const RIGHT_BRACKET: u8 = 0x30;
    pub const BACKSLASH: u8 = 0x31;
    pub const SEMICOLON: u8 = 0x33;
    pub const QUOTE: u8 = 0x34;
    pub const GRAVE: u8 = 0x35;
    pub const COMMA: u8 = 0x36;
    pub const PERIOD: u8 = 0x37;
    pub const SLASH: u8 = 0x38;
    pub const CAPS_LOCK: u8 = 0x39;
    pub const F1: u8 = 0x3a;
    pub const F2: u8 = 0x3b;
    pub const F3: u8 = 0x3c;
    pub const F4: u8 = 0x3d;
    pub const F5: u8 = 0x3e;
    pub const F6: u8 = 0x3f;
    pub const F7: u8 = 0x40;
    pub const F8: u8 = 0x41;
    pub const F9: u8 = 0x42;
    pub const F10: u8 = 0x43;
    pub const F11: u8 = 0x44;
    pub const F12: u8 = 0x45;
}

/// Helper to convert character to HID keyboard usage code and modifiers
/// Returns (usage_code, needs_shift)
#[cfg(feature = "usb-keyboard")]
pub fn char_to_usage(ch: char) -> Option<(u8, bool)> {
    match ch {
        'a'..='z' => Some((keyboard_usage::A + (ch as u8 - b'a'), false)),
        'A'..='Z' => Some((keyboard_usage::A + (ch as u8 - b'A'), true)),
        '1'..='9' => Some((keyboard_usage::NUM_1 + (ch as u8 - b'1'), false)),
        '0' => Some((keyboard_usage::NUM_0, false)),
        ' ' => Some((keyboard_usage::SPACE, false)),
        '\n' => Some((keyboard_usage::ENTER, false)),
        '\t' => Some((keyboard_usage::TAB, false)),
        '-' => Some((keyboard_usage::MINUS, false)),
        '_' => Some((keyboard_usage::MINUS, true)),
        '=' => Some((keyboard_usage::EQUALS, false)),
        '+' => Some((keyboard_usage::EQUALS, true)),
        '[' => Some((keyboard_usage::LEFT_BRACKET, false)),
        '{' => Some((keyboard_usage::LEFT_BRACKET, true)),
        ']' => Some((keyboard_usage::RIGHT_BRACKET, false)),
        '}' => Some((keyboard_usage::RIGHT_BRACKET, true)),
        '\\' => Some((keyboard_usage::BACKSLASH, false)),
        '|' => Some((keyboard_usage::BACKSLASH, true)),
        ';' => Some((keyboard_usage::SEMICOLON, false)),
        ':' => Some((keyboard_usage::SEMICOLON, true)),
        '\'' => Some((keyboard_usage::QUOTE, false)),
        '"' => Some((keyboard_usage::QUOTE, true)),
        '`' => Some((keyboard_usage::GRAVE, false)),
        '~' => Some((keyboard_usage::GRAVE, true)),
        ',' => Some((keyboard_usage::COMMA, false)),
        '<' => Some((keyboard_usage::COMMA, true)),
        '.' => Some((keyboard_usage::PERIOD, false)),
        '>' => Some((keyboard_usage::PERIOD, true)),
        '/' => Some((keyboard_usage::SLASH, false)),
        '?' => Some((keyboard_usage::SLASH, true)),
        '!' => Some((keyboard_usage::NUM_1, true)),
        '@' => Some((keyboard_usage::NUM_2, true)),
        '#' => Some((keyboard_usage::NUM_3, true)),
        '$' => Some((keyboard_usage::NUM_4, true)),
        '%' => Some((keyboard_usage::NUM_5, true)),
        '^' => Some((keyboard_usage::NUM_6, true)),
        '&' => Some((keyboard_usage::NUM_7, true)),
        '*' => Some((keyboard_usage::NUM_8, true)),
        '(' => Some((keyboard_usage::NUM_9, true)),
        ')' => Some((keyboard_usage::NUM_0, true)),
        _ => None,
    }
}
