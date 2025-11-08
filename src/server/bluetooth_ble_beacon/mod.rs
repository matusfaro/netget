//! Bluetooth Low Energy (BLE) Beacon implementation
//!
//! Builds on bluetooth-ble to provide iBeacon and Eddystone beacon functionality.
//! Beacons are advertisement-only - they broadcast data without accepting connections.

pub mod actions;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::server::bluetooth_ble::BluetoothBle;

/// BLE Beacon server
pub struct BluetoothBleBeacon;

impl BluetoothBleBeacon {
    /// Spawn BLE beacon server
    #[cfg(feature = "bluetooth-ble-beacon")]
    pub async fn spawn_with_llm_actions(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        info!("Starting BLE Beacon server");

        // Create the underlying BLE server with beacon configuration
        let beacon_instruction = format!(
            "{}. Configure as a BLE beacon for advertising only. Do not accept connections. {}",
            instruction,
            "Use advertising packets to broadcast beacon data (iBeacon or Eddystone format)."
        );

        // Use the base bluetooth-ble server
        BluetoothBle::spawn_with_llm_actions(
            "NetGet-Beacon".to_string(),
            llm_client,
            app_state,
            status_tx,
            server_id,
            beacon_instruction,
        ).await
    }
}

#[cfg(not(feature = "bluetooth-ble-beacon"))]
impl BluetoothBleBeacon {
    pub async fn spawn_with_llm_actions(
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("BLE beacon support not enabled - compile with --features bluetooth-ble-beacon")
    }
}

/// iBeacon packet format
pub mod ibeacon {
    /// Build iBeacon advertising data
    ///
    /// Format:
    /// - Company ID: 0x004C (Apple)
    /// - Type: 0x02 (iBeacon)
    /// - Length: 0x15 (21 bytes)
    /// - UUID: 16 bytes
    /// - Major: 2 bytes (big-endian)
    /// - Minor: 2 bytes (big-endian)
    /// - TX Power: 1 byte (signed)
    pub fn build_advertising_data(
        uuid: &[u8; 16],
        major: u16,
        minor: u16,
        tx_power: i8,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(30);

        // iBeacon prefix
        data.extend_from_slice(&[
            0x02, 0x01, 0x06,           // Flags
            0x1A, 0xFF,                  // Manufacturer specific data (26 bytes)
            0x4C, 0x00,                  // Apple company ID
            0x02,                        // iBeacon type
            0x15,                        // iBeacon length (21 bytes)
        ]);

        // UUID (16 bytes)
        data.extend_from_slice(uuid);

        // Major (2 bytes, big-endian)
        data.extend_from_slice(&major.to_be_bytes());

        // Minor (2 bytes, big-endian)
        data.extend_from_slice(&minor.to_be_bytes());

        // TX Power (1 byte, signed)
        data.push(tx_power as u8);

        data
    }
}

/// Eddystone packet formats
pub mod eddystone {
    /// Eddystone service UUID
    pub const SERVICE_UUID: u16 = 0xFEAA;

    /// Frame types
    pub const FRAME_TYPE_UID: u8 = 0x00;
    pub const FRAME_TYPE_URL: u8 = 0x10;
    pub const FRAME_TYPE_TLM: u8 = 0x20;
    pub const FRAME_TYPE_EID: u8 = 0x30;

    /// URL scheme codes
    pub const URL_SCHEME_HTTP_WWW: u8 = 0x00;  // http://www.
    pub const URL_SCHEME_HTTPS_WWW: u8 = 0x01; // https://www.
    pub const URL_SCHEME_HTTP: u8 = 0x02;      // http://
    pub const URL_SCHEME_HTTPS: u8 = 0x03;     // https://

    /// Build Eddystone-UID advertising data
    pub fn build_uid_data(
        namespace: &[u8; 10],
        instance: &[u8; 6],
        tx_power: i8,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(31);

        // Eddystone service data
        data.extend_from_slice(&[
            0x03, 0x03,                  // Complete list of 16-bit UUIDs
            0xAA, 0xFE,                  // Eddystone service UUID
            0x17, 0x16,                  // Service data (23 bytes)
            0xAA, 0xFE,                  // Eddystone service UUID
            FRAME_TYPE_UID,              // Frame type: UID
            tx_power as u8,              // Calibrated TX power at 0m
        ]);

        // Namespace (10 bytes)
        data.extend_from_slice(namespace);

        // Instance (6 bytes)
        data.extend_from_slice(instance);

        // RFU (2 bytes reserved)
        data.extend_from_slice(&[0x00, 0x00]);

        data
    }

    /// Build Eddystone-URL advertising data
    pub fn build_url_data(url: &str, tx_power: i8) -> Result<Vec<u8>, &'static str> {
        let mut data = Vec::with_capacity(31);

        // Determine URL scheme
        let (scheme_code, url_body) = if url.starts_with("https://www.") {
            (URL_SCHEME_HTTPS_WWW, &url[12..])
        } else if url.starts_with("http://www.") {
            (URL_SCHEME_HTTP_WWW, &url[11..])
        } else if url.starts_with("https://") {
            (URL_SCHEME_HTTPS, &url[8..])
        } else if url.starts_with("http://") {
            (URL_SCHEME_HTTP, &url[7..])
        } else {
            return Err("URL must start with http:// or https://");
        };

        // URL too long
        if url_body.len() > 17 {
            return Err("URL too long (max 17 chars after scheme)");
        }

        // Eddystone service data
        data.extend_from_slice(&[
            0x03, 0x03,                  // Complete list of 16-bit UUIDs
            0xAA, 0xFE,                  // Eddystone service UUID
        ]);

        // Service data length (3 + url_body.len() bytes)
        data.push(3 + url_body.len() as u8);
        data.push(0x16);                 // Service data type

        data.extend_from_slice(&[
            0xAA, 0xFE,                  // Eddystone service UUID
            FRAME_TYPE_URL,              // Frame type: URL
            tx_power as u8,              // Calibrated TX power at 0m
            scheme_code,                 // URL scheme
        ]);

        // URL body (encoded)
        data.extend_from_slice(url_body.as_bytes());

        Ok(data)
    }

    /// Build Eddystone-TLM advertising data
    pub fn build_tlm_data(
        battery_voltage: u16,
        temperature: f32,
        adv_count: u32,
        uptime: u32,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(31);

        // Convert temperature to 8.8 fixed point
        let temp_fixed = (temperature * 256.0) as i16;

        // Eddystone service data
        data.extend_from_slice(&[
            0x03, 0x03,                  // Complete list of 16-bit UUIDs
            0xAA, 0xFE,                  // Eddystone service UUID
            0x11, 0x16,                  // Service data (17 bytes)
            0xAA, 0xFE,                  // Eddystone service UUID
            FRAME_TYPE_TLM,              // Frame type: TLM
            0x00,                        // TLM version
        ]);

        // Battery voltage (mV, big-endian)
        data.extend_from_slice(&battery_voltage.to_be_bytes());

        // Temperature (8.8 fixed point, big-endian)
        data.extend_from_slice(&temp_fixed.to_be_bytes());

        // Advertisement count (big-endian)
        data.extend_from_slice(&adv_count.to_be_bytes());

        // Uptime (0.1s resolution, big-endian)
        let uptime_deciseconds = uptime * 10;
        data.extend_from_slice(&uptime_deciseconds.to_be_bytes());

        data
    }
}
