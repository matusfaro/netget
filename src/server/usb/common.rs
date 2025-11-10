//! Common USB/IP utilities shared across all USB protocols
//!
//! This module provides shared functionality for USB/IP protocol handling,
//! including connection management, URB processing, and helper utilities.


/// USB/IP protocol version (1.1.1)
#[cfg(feature = "usb-common")]
pub const USBIP_VERSION: u16 = 0x0111;

/// USB/IP operation codes
#[cfg(feature = "usb-common")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UsbIpOp {
    /// Request device list
    ReqDevlist = 0x8005,
    /// Reply device list
    RepDevlist = 0x0005,
    /// Import device request
    ReqImport = 0x8003,
    /// Import device reply (same opcode as RetSubmit in USB/IP protocol)
    RepImport = 0x0003,
    /// Submit URB
    CmdSubmit = 0x0001,
    /// URB reply (same opcode 0x0003 as RepImport - context determines which)
    /// NOTE: In USB/IP, RepImport and RetSubmit share opcode 0x0003.
    /// Use context (connection state) to distinguish them.
    // RetSubmit = 0x0003,  // Commented out - use RepImport variant for 0x0003
    /// Unlink URB
    CmdUnlink = 0x0002,
    /// Unlink reply
    RetUnlink = 0x0004,
}

/// USB/IP connection state
#[cfg(feature = "usb-common")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbIpConnectionState {
    /// Waiting for device list request
    WaitingForRequest,
    /// Device imported, processing URBs
    Active,
    /// Connection closed
    Closed,
}

/// Helper to create a human-readable device path (required by USB/IP protocol)
#[cfg(feature = "usb-common")]
pub fn format_device_path(bus_num: u32, dev_num: u32) -> String {
    format!("{}-{}", bus_num, dev_num)
}

/// Convert USB endpoint direction to string
#[cfg(feature = "usb-common")]
pub fn endpoint_direction_str(endpoint: u8) -> &'static str {
    if endpoint & 0x80 != 0 {
        "IN"
    } else {
        "OUT"
    }
}

/// Convert USB endpoint address to number (strip direction bit)
#[cfg(feature = "usb-common")]
pub fn endpoint_number(endpoint: u8) -> u8 {
    endpoint & 0x0f
}

/// USB device speed constants
#[cfg(feature = "usb-common")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum UsbSpeed {
    Unknown = 0,
    Low = 1,      // 1.5 Mbit/s
    Full = 2,     // 12 Mbit/s
    High = 3,     // 480 Mbit/s
    Wireless = 4, // 480 Mbit/s
    Super = 5,    // 5 Gbit/s
    SuperPlus = 6, // 10 Gbit/s
}

impl Default for UsbSpeed {
    fn default() -> Self {
        UsbSpeed::Full
    }
}

/// USB device class codes
#[cfg(feature = "usb-common")]
pub mod device_class {
    pub const USE_INTERFACE: u8 = 0x00;
    pub const AUDIO: u8 = 0x01;
    pub const COMM: u8 = 0x02; // CDC (Communication Device Class)
    pub const HID: u8 = 0x03;  // Human Interface Device
    pub const PHYSICAL: u8 = 0x05;
    pub const IMAGE: u8 = 0x06;
    pub const PRINTER: u8 = 0x07;
    pub const MASS_STORAGE: u8 = 0x08;
    pub const HUB: u8 = 0x09;
    pub const CDC_DATA: u8 = 0x0a;
    pub const SMART_CARD: u8 = 0x0b;
    pub const CONTENT_SECURITY: u8 = 0x0d;
    pub const VIDEO: u8 = 0x0e;
    pub const PERSONAL_HEALTHCARE: u8 = 0x0f;
    pub const AUDIO_VIDEO: u8 = 0x10;
    pub const DIAGNOSTIC: u8 = 0xdc;
    pub const WIRELESS: u8 = 0xe0;
    pub const MISCELLANEOUS: u8 = 0xef;
    pub const APPLICATION_SPECIFIC: u8 = 0xfe;
    pub const VENDOR_SPECIFIC: u8 = 0xff;
}

/// USB descriptor types
#[cfg(feature = "usb-common")]
pub mod descriptor_type {
    pub const DEVICE: u8 = 0x01;
    pub const CONFIGURATION: u8 = 0x02;
    pub const STRING: u8 = 0x03;
    pub const INTERFACE: u8 = 0x04;
    pub const ENDPOINT: u8 = 0x05;
    pub const DEVICE_QUALIFIER: u8 = 0x06;
    pub const OTHER_SPEED_CONFIGURATION: u8 = 0x07;
    pub const INTERFACE_POWER: u8 = 0x08;
    pub const HID: u8 = 0x21;
    pub const HID_REPORT: u8 = 0x22;
    pub const HID_PHYSICAL: u8 = 0x23;
}

/// USB standard requests (bmRequestType)
#[cfg(feature = "usb-common")]
pub mod request_type {
    // Direction
    pub const HOST_TO_DEVICE: u8 = 0x00;
    pub const DEVICE_TO_HOST: u8 = 0x80;

    // Type
    pub const STANDARD: u8 = 0x00;
    pub const CLASS: u8 = 0x20;
    pub const VENDOR: u8 = 0x40;

    // Recipient
    pub const DEVICE: u8 = 0x00;
    pub const INTERFACE: u8 = 0x01;
    pub const ENDPOINT: u8 = 0x02;
    pub const OTHER: u8 = 0x03;
}

/// USB standard request codes (bRequest)
#[cfg(feature = "usb-common")]
pub mod request {
    pub const GET_STATUS: u8 = 0x00;
    pub const CLEAR_FEATURE: u8 = 0x01;
    pub const SET_FEATURE: u8 = 0x03;
    pub const SET_ADDRESS: u8 = 0x05;
    pub const GET_DESCRIPTOR: u8 = 0x06;
    pub const SET_DESCRIPTOR: u8 = 0x07;
    pub const GET_CONFIGURATION: u8 = 0x08;
    pub const SET_CONFIGURATION: u8 = 0x09;
    pub const GET_INTERFACE: u8 = 0x0a;
    pub const SET_INTERFACE: u8 = 0x0b;
    pub const SYNCH_FRAME: u8 = 0x0c;
}

/// HID-specific request codes
#[cfg(feature = "usb-common")]
pub mod hid_request {
    pub const GET_REPORT: u8 = 0x01;
    pub const GET_IDLE: u8 = 0x02;
    pub const GET_PROTOCOL: u8 = 0x03;
    pub const SET_REPORT: u8 = 0x09;
    pub const SET_IDLE: u8 = 0x0a;
    pub const SET_PROTOCOL: u8 = 0x0b;
}

/// CDC-specific request codes
#[cfg(feature = "usb-common")]
pub mod cdc_request {
    pub const SEND_ENCAPSULATED_COMMAND: u8 = 0x00;
    pub const GET_ENCAPSULATED_RESPONSE: u8 = 0x01;
    pub const SET_LINE_CODING: u8 = 0x20;
    pub const GET_LINE_CODING: u8 = 0x21;
    pub const SET_CONTROL_LINE_STATE: u8 = 0x22;
    pub const SEND_BREAK: u8 = 0x23;
}

/// Endpoint transfer types
#[cfg(feature = "usb-common")]
pub mod transfer_type {
    pub const CONTROL: u8 = 0;
    pub const ISOCHRONOUS: u8 = 1;
    pub const BULK: u8 = 2;
    pub const INTERRUPT: u8 = 3;
}

/// Helper to format USB setup packet for logging
#[cfg(feature = "usb-common")]
pub fn format_setup_packet(
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
) -> String {
    let direction = if request_type & 0x80 != 0 {
        "IN"
    } else {
        "OUT"
    };
    let req_type = match request_type & 0x60 {
        0x00 => "STANDARD",
        0x20 => "CLASS",
        0x40 => "VENDOR",
        _ => "RESERVED",
    };
    let recipient = match request_type & 0x1f {
        0x00 => "DEVICE",
        0x01 => "INTERFACE",
        0x02 => "ENDPOINT",
        0x03 => "OTHER",
        _ => "RESERVED",
    };

    format!(
        "Setup[{} {} {}] req={:#04x} val={:#06x} idx={:#06x} len={}",
        direction, req_type, recipient, request, value, index, length
    )
}

/// Logging helper: hex dump data in a readable format
#[cfg(feature = "usb-common")]
pub fn hex_dump(data: &[u8], max_bytes: usize) -> String {
    if data.is_empty() {
        return "[]".to_string();
    }

    let truncated = if data.len() > max_bytes {
        &data[..max_bytes]
    } else {
        data
    };

    let hex: String = truncated.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");

    if data.len() > max_bytes {
        format!("[{} ... ({} bytes total)]", hex, data.len())
    } else {
        format!("[{}]", hex)
    }
}
