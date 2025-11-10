//! Bluetooth Low Energy (BLE) HID Keyboard implementation
//!
//! Builds on bluetooth-ble to provide HID over GATT keyboard functionality.
//! Supports connection tracking and targeted messages to specific devices.

pub mod actions;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::info;

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::server::bluetooth_ble::BluetoothBle;

/// Client connection ID for tracking connected devices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u32);

/// Per-client connection state
#[derive(Debug)]
pub struct ClientConnection {
    pub id: ClientId,
    pub connected_at: std::time::Instant,
}

/// BLE HID Keyboard server
pub struct BluetoothBleKeyboard {
    connections: Arc<Mutex<HashMap<ClientId, ClientConnection>>>,
    #[allow(dead_code)]
    next_client_id: Arc<Mutex<u32>>,
}

impl BluetoothBleKeyboard {
    /// Spawn BLE HID keyboard server
    #[cfg(feature = "bluetooth-ble-keyboard")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE HID Keyboard server: {}", device_name);

        // Create the underlying BLE server with HID keyboard configuration
        let keyboard_instruction = format!(
            "Configure as a BLE HID keyboard with HID Service (UUID: 0x1812). {} {}",
            instruction,
            "Add HID Report Map, HID Report Input, HID Information, and HID Control Point characteristics."
        );

        // Use the base bluetooth-ble server
        BluetoothBle::spawn_with_llm_actions(
            device_name,
            llm_client,
            app_state,
            status_tx,
            server_id,
            keyboard_instruction,
        ).await
    }

    /// Track a new client connection
    pub async fn add_connection(&self, client_id: ClientId) {
        let conn = ClientConnection {
            id: client_id,
            connected_at: std::time::Instant::now(),
        };
        self.connections.lock().await.insert(client_id, conn);
        info!("BLE keyboard client {} connected", client_id.0);
    }

    /// Remove a client connection
    pub async fn remove_connection(&self, client_id: ClientId) {
        self.connections.lock().await.remove(&client_id);
        info!("BLE keyboard client {} disconnected", client_id.0);
    }

    /// Get all connected client IDs
    pub async fn get_connections(&self) -> Vec<ClientId> {
        self.connections.lock().await.keys().copied().collect()
    }
}

#[cfg(not(feature = "bluetooth-ble-keyboard"))]
impl BluetoothBleKeyboard {
    pub async fn spawn_with_llm_actions(
        _device_name: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE keyboard support not enabled - compile with --features bluetooth-ble-keyboard")
    }
}

/// HID keyboard scan codes (USB HID specification)
pub mod hid_keycodes {
    pub const KEY_A: u8 = 0x04;
    pub const KEY_B: u8 = 0x05;
    pub const KEY_C: u8 = 0x06;
    // ... (full mapping in actual implementation)
    pub const KEY_ENTER: u8 = 0x28;
    pub const KEY_SPACE: u8 = 0x2C;

    /// Modifier bits
    pub const MOD_LEFT_CTRL: u8 = 0x01;
    pub const MOD_LEFT_SHIFT: u8 = 0x02;
    pub const MOD_LEFT_ALT: u8 = 0x04;
    pub const MOD_LEFT_GUI: u8 = 0x08;

    /// Convert ASCII character to HID keycode
    pub fn char_to_keycode(c: char) -> Option<(u8, u8)> {
        // Returns (modifiers, keycode)
        match c {
            'a'..='z' => Some((0, KEY_A + (c as u8 - b'a'))),
            'A'..='Z' => Some((MOD_LEFT_SHIFT, KEY_A + (c as u8 - b'A'))),
            ' ' => Some((0, KEY_SPACE)),
            '\n' => Some((0, KEY_ENTER)),
            _ => None,
        }
    }
}

/// HID Report Descriptor for keyboard
pub const HID_KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x06,       // Usage (Keyboard)
    0xA1, 0x01,       // Collection (Application)
    0x05, 0x07,       //   Usage Page (Key Codes)
    0x19, 0xE0,       //   Usage Minimum (224)
    0x29, 0xE7,       //   Usage Maximum (231)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x08,       //   Report Count (8)
    0x81, 0x02,       //   Input (Data, Variable, Absolute) - Modifier byte
    0x95, 0x01,       //   Report Count (1)
    0x75, 0x08,       //   Report Size (8)
    0x81, 0x01,       //   Input (Constant) - Reserved byte
    0x95, 0x06,       //   Report Count (6)
    0x75, 0x08,       //   Report Size (8)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x65,       //   Logical Maximum (101)
    0x05, 0x07,       //   Usage Page (Key Codes)
    0x19, 0x00,       //   Usage Minimum (0)
    0x29, 0x65,       //   Usage Maximum (101)
    0x81, 0x00,       //   Input (Data, Array) - Key array
    0xC0,             // End Collection
];
