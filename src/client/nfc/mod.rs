//! NFC (Near Field Communication) client implementation
//!
//! Uses PC/SC (Personal Computer/Smart Card) API for cross-platform NFC reader support:
//! - Windows: Native WinSCard.dll
//! - macOS: Native PCSC framework
//! - Linux: PCSC lite library (pcscd daemon)
//!
//! Supports ISO14443 A/B cards, MIFARE, NFC tags via APDU commands and NDEF messages.

pub mod actions;

use crate::client::nfc::actions::*;
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::ClientId;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::info;

// Re-export protocol
pub use actions::NfcClientProtocol;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// NFC client implementation
pub struct NfcClient;

impl NfcClient {
    /// Connect to NFC reader and start LLM integration loop
    pub async fn connect_with_llm_actions(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Value,
    ) -> Result<SocketAddr> {
        info!("Starting NFC client via PC/SC...");

        // Extract reader selection from startup params
        let reader_index = startup_params["reader_index"].as_u64().unwrap_or(0) as usize;
        let reader_name = startup_params["reader_name"].as_str().map(|s| s.to_string());

        // Create PC/SC context
        #[cfg(feature = "nfc-client")]
        let ctx = pcsc::Context::establish(pcsc::Scope::User)
            .context("Failed to establish PC/SC context. Is pcscd running (Linux)?")?;

        // List available readers
        #[cfg(feature = "nfc-client")]
        let readers_buf = ctx
            .list_readers_owned()
            .context("Failed to list PC/SC readers")?;

        #[cfg(feature = "nfc-client")]
        let readers: Vec<String> = readers_buf
            .iter()
            .map(|r| r.to_string_lossy().to_string())
            .collect();

        #[cfg(feature = "nfc-client")]
        if readers.is_empty() {
            return Err(anyhow!(
                "No PC/SC readers found. Please connect an NFC reader (e.g., ACR122U)"
            ));
        }

        #[cfg(feature = "nfc-client")]
        info!("Found {} PC/SC reader(s): {:?}", readers.len(), readers);

        // Select reader
        #[cfg(feature = "nfc-client")]
        let selected_reader = if let Some(name) = reader_name {
            readers
                .iter()
                .find(|r| r.contains(&name))
                .ok_or_else(|| anyhow!("Reader '{}' not found", name))?
                .clone()
        } else {
            readers
                .get(reader_index)
                .ok_or_else(|| anyhow!("Reader index {} out of range", reader_index))?
                .clone()
        };

        #[cfg(feature = "nfc-client")]
        info!("Using PC/SC reader: {}", selected_reader);
        #[cfg(feature = "nfc-client")]
        let _ = status_tx.send(format!("Using NFC reader: {}", selected_reader));

        // Create client state
        #[cfg(feature = "nfc-client")]
        let _client_state = Arc::new(Mutex::new(NfcClientState {
            ctx: ctx.clone(),
            reader_name: selected_reader.clone(),
            card: None,
            connection_state: ConnectionState::Idle,
        }));

        // TODO: Async client actions not yet supported
        // NFC client currently only supports initial reader listing
        // Future: Add support for async actions (connect_card, etc.) via event-driven pattern

        // Send initial event to LLM: readers listed
        #[cfg(feature = "nfc-client")]
        {
            let readers_json: Vec<String> = readers.clone();
            let event = Event::new(&NFC_READERS_LISTED_EVENT, json!({ "readers": readers_json }));

            // Get default instruction from startup params or use default
            let instruction = startup_params["instruction"]
                .as_str()
                .unwrap_or("Monitor NFC reader and respond to card events");

            // Create protocol instance for action definitions
            let protocol = Arc::new(NfcClientProtocol);

            // Call LLM with initial event
            let _result = call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                instruction,
                "", // No memory yet
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await?;

            // TODO: Execute returned actions when async client action support is added
        }

        // Return dummy socket address (NFC doesn't use network sockets)
        Ok(SocketAddr::from(([127, 0, 0, 1], 0)))
    }

    // TODO: Implement LLM-driven NFC card operations when async client action support is added
    // This will include:
    // - Async actions: connect_card, disconnect_card
    // - Sync actions: send_apdu, read_ndef, write_ndef
    // - Event-driven pattern: card_detected → send_apdu → apdu_response → next action
}

/// NFC client state
#[cfg(feature = "nfc-client")]
struct NfcClientState {
    ctx: pcsc::Context,
    reader_name: String,
    card: Option<pcsc::Card>,
    connection_state: ConnectionState,
}

// Stub implementation when feature is disabled
#[cfg(not(feature = "nfc-client"))]
impl NfcClient {
    pub async fn connect_with_llm_actions(
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _client_id: ClientId,
        _startup_params: Value,
    ) -> Result<SocketAddr> {
        Err(anyhow!("NFC client support not compiled (feature 'nfc-client' disabled)"))
    }
}
