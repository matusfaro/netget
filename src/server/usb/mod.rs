//! USB protocol implementations
//!
//! This module provides virtual USB device protocols using USB/IP:
//! - usb-keyboard: HID Keyboard device
//! - usb-mouse: HID Mouse device (future)
//! - usb-serial: CDC ACM Serial device (future)
//! - usb: Low-level custom USB device (future)

#[cfg(feature = "usb-common")]
pub mod common;

#[cfg(feature = "usb-common")]
pub mod descriptors;

#[cfg(feature = "usb-keyboard")]
pub mod keyboard;

// Re-export protocol implementations
#[cfg(feature = "usb-keyboard")]
pub use keyboard::actions::UsbKeyboardProtocol;
