//! NFC (Near Field Communication) virtual server implementation
//!
//! **Important**: This is a VIRTUAL/SIMULATION server only.
//! Most PC/SC readers are read-only and cannot emulate NFC tags/cards.
//! This server simulates what an NFC tag would do for testing purposes.
//!
//! For actual card emulation, you would need:
//! - Special hardware (e.g., smart card simulators, HCE-capable devices)
//! - Android HCE (Host Card Emulation) or iOS NFC
//!
//! This virtual server is useful for:
//! - Testing NFC client implementations
//! - Understanding NFC protocols
//! - Simulating tag responses without physical hardware

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::nfc::actions::*;
use crate::state::app_state::AppState;
use crate::state::server::ServerId;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

// Re-export protocol
pub use actions::NfcServerProtocol;

/// Virtual NFC tag state
struct VirtualNfcTag {
    #[allow(dead_code)]
    atr: String,                         // Answer to Reset
    #[allow(dead_code)]
    uid: String,                         // Tag UID
    #[allow(dead_code)]
    tag_type: String,                    // Tag type
    #[allow(dead_code)]
    ndef_records: Vec<Value>,            // NDEF message records
    #[allow(dead_code)]
    selected_application: Option<String>, // Currently selected application ID
}

impl VirtualNfcTag {
    fn new(uid: String, tag_type: String) -> Self {
        // Default ATR for Type 4 tag
        let atr = "3B8F8001804F0CA0000003060300030000000068".to_string();

        Self {
            atr,
            uid,
            tag_type,
            ndef_records: Vec::new(),
            selected_application: None,
        }
    }
}

/// NFC virtual server implementation
pub struct NfcServer;

impl NfcServer {
    /// Start virtual NFC tag server
    pub async fn start(
        bind_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        startup_params: Value,
    ) -> Result<SocketAddr> {
        info!("Starting virtual NFC tag server...");

        // Extract startup params
        let tag_type = startup_params["tag_type"]
            .as_str()
            .unwrap_or("generic")
            .to_string();
        let uid = startup_params["uid"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Generate random UID (7 bytes for Type 4)
                let random_bytes: Vec<u8> = (0..7).map(|_| rand::random::<u8>()).collect();
                hex::encode(random_bytes)
            });

        info!(
            "Virtual NFC tag: type={}, UID={}",
            tag_type, uid
        );
        let _ = status_tx.send(format!(
            "Virtual NFC tag started: type={}, UID={}",
            tag_type, uid
        ));

        // Create virtual tag state
        let _tag_state = Arc::new(tokio::sync::Mutex::new(VirtualNfcTag::new(
            uid.clone(),
            tag_type.clone(),
        )));

        // Call LLM with server started event
        let event = Event::new(&NFC_SERVER_STARTED_EVENT, json!({}));
        let protocol = NfcServerProtocol;

        let _result = call_llm(
            &llm_client,
            &app_state,
            server_id,
            None, // No connection ID for server-level event
            &event,
            &protocol,
        )
        .await?;

        // Note: No need to handle results for server startup event
        // Virtual tag doesn't process startup actions

        // NOTE: Since this is a virtual server, we don't actually listen on network
        // In a real implementation, you would:
        // 1. Use PC/SC card emulation API (if supported by hardware)
        // 2. Or implement HCE (Host Card Emulation) on Android
        // 3. Or use a smart card simulator device

        info!(
            "Virtual NFC tag server running (simulation only) at {}",
            bind_addr
        );
        Ok(bind_addr)
    }

    /// Handle async action
    #[allow(dead_code)]
    async fn handle_async_action(
        _tag_state: Arc<tokio::sync::Mutex<VirtualNfcTag>>,
        result: ActionResult,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match result {
            ActionResult::Custom { name, data } => {
                if name == "set_atr" {
                    let atr_hex = data["atr_hex"]
                        .as_str()
                        .ok_or_else(|| anyhow!("Missing atr_hex"))?;

                    debug!("Setting virtual tag ATR: {}", atr_hex);
                    let _ = status_tx.send(format!("Set ATR: {}", atr_hex));

                    // In a real implementation, this would configure hardware
                    // For virtual server, just log it
                } else if name == "set_ndef_message" {
                    let records = data["records"]
                        .as_array()
                        .ok_or_else(|| anyhow!("Missing records"))?;

                    debug!("Setting NDEF message with {} records", records.len());
                    let _ = status_tx.send(format!("Set NDEF message: {} records", records.len()));

                    // In a real implementation, this would store NDEF data
                    // For virtual server, just log it
                }
            }
            ActionResult::NoAction => {
                // No operation needed
            }
            _ => {
                warn!("Unhandled action result: {:?}", result);
            }
        }

        Ok(())
    }
}
