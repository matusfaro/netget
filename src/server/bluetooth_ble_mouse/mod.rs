//! Bluetooth Low Energy (BLE) HID Mouse implementation
//!
//! Builds on bluetooth-ble to provide HID over GATT mouse functionality.
//! Supports connection tracking and targeted messages to specific devices.

pub mod actions;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

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

/// BLE HID Mouse server
pub struct BluetoothBleMouse {
    connections: Arc<Mutex<HashMap<ClientId, ClientConnection>>>,
    next_client_id: Arc<Mutex<u32>>,
}

impl BluetoothBleMouse {
    /// Spawn BLE HID mouse server
    #[cfg(feature = "bluetooth-ble-mouse")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE HID Mouse server: {}", device_name);

        // Create the underlying BLE server with HID mouse configuration
        let mouse_instruction = format!(
            "{}. Configure as a BLE HID mouse with HID Service (UUID: 0x1812). {}",
            instruction,
            "Add HID Report Map, HID Report Input, HID Information, and HID Control Point characteristics for mouse."
        );

        // Use the base bluetooth-ble server
        BluetoothBle::spawn_with_llm_actions(
            device_name,
            llm_client,
            app_state,
            status_tx,
            server_id,
            mouse_instruction,
        ).await
    }

    /// Track a new client connection
    pub async fn add_connection(&self, client_id: ClientId) {
        let conn = ClientConnection {
            id: client_id,
            connected_at: std::time::Instant::now(),
        };
        self.connections.lock().await.insert(client_id, conn);
        info!("BLE mouse client {} connected", client_id.0);
    }

    /// Remove a client connection
    pub async fn remove_connection(&self, client_id: ClientId) {
        self.connections.lock().await.remove(&client_id);
        info!("BLE mouse client {} disconnected", client_id.0);
    }

    /// Get all connected client IDs
    pub async fn get_connections(&self) -> Vec<ClientId> {
        self.connections.lock().await.keys().copied().collect()
    }
}

#[cfg(not(feature = "bluetooth-ble-mouse"))]
impl BluetoothBleMouse {
    pub async fn spawn_with_llm_actions(
        _device_name: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE mouse support not enabled - compile with --features bluetooth-ble-mouse")
    }
}

/// HID mouse button bits
pub mod hid_mouse_buttons {
    pub const BUTTON_LEFT: u8 = 0x01;
    pub const BUTTON_RIGHT: u8 = 0x02;
    pub const BUTTON_MIDDLE: u8 = 0x04;
}

/// HID Report Descriptor for mouse
pub const HID_MOUSE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x02,       // Usage (Mouse)
    0xA1, 0x01,       // Collection (Application)
    0x09, 0x01,       //   Usage (Pointer)
    0xA1, 0x00,       //   Collection (Physical)
    0x05, 0x09,       //     Usage Page (Buttons)
    0x19, 0x01,       //     Usage Minimum (1)
    0x29, 0x03,       //     Usage Maximum (3)
    0x15, 0x00,       //     Logical Minimum (0)
    0x25, 0x01,       //     Logical Maximum (1)
    0x95, 0x03,       //     Report Count (3)
    0x75, 0x01,       //     Report Size (1)
    0x81, 0x02,       //     Input (Data, Variable, Absolute) - Button bits
    0x95, 0x01,       //     Report Count (1)
    0x75, 0x05,       //     Report Size (5)
    0x81, 0x01,       //     Input (Constant) - Padding
    0x05, 0x01,       //     Usage Page (Generic Desktop)
    0x09, 0x30,       //     Usage (X)
    0x09, 0x31,       //     Usage (Y)
    0x09, 0x38,       //     Usage (Wheel)
    0x15, 0x81,       //     Logical Minimum (-127)
    0x25, 0x7F,       //     Logical Maximum (127)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x03,       //     Report Count (3)
    0x81, 0x06,       //     Input (Data, Variable, Relative) - X, Y, Wheel
    0xC0,             //   End Collection (Physical)
    0xC0,             // End Collection (Application)
];
