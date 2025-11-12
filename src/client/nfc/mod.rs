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
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client::{ClientConnectionState, ClientId};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

// Re-export protocol
pub use actions::NfcClientProtocol;

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
        let client_state = Arc::new(Mutex::new(NfcClientState {
            ctx: ctx.clone(),
            reader_name: selected_reader.clone(),
            card: None,
            connection_state: ClientConnectionState::Idle,
        }));

        // Spawn LLM integration loop
        #[cfg(feature = "nfc-client")]
        {
            let client_state_clone = client_state.clone();
            let llm_client_clone = llm_client.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::llm_integration_loop(
                    client_state_clone,
                    llm_client_clone,
                    app_state_clone,
                    status_tx_clone,
                    client_id,
                )
                .await
                {
                    error!("NFC client LLM integration loop error: {}", e);
                }
            });
        }

        // Send initial event to LLM: readers listed
        #[cfg(feature = "nfc-client")]
        {
            let readers_json: Vec<String> = readers.clone();
            let event = Event::new(&NFC_READERS_LISTED_EVENT, json!({ "readers": readers_json }));

            call_llm_for_client(
                llm_client,
                app_state.clone(),
                status_tx,
                client_id,
                Some(&event),
            )
            .await?;
        }

        // Return dummy socket address (NFC doesn't use network sockets)
        Ok(SocketAddr::from(([127, 0, 0, 1], 0)))
    }

    /// LLM integration loop - handles card detection and action execution
    #[cfg(feature = "nfc-client")]
    async fn llm_integration_loop(
        client_state: Arc<Mutex<NfcClientState>>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<()> {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Check if client still exists
            if !app_state.has_client(client_id).await {
                info!("NFC client {} no longer exists, stopping loop", client_id.0);
                break;
            }

            // Get actions from LLM
            let actions = match app_state.get_pending_client_actions(client_id).await {
                Some(actions) => actions,
                None => continue,
            };

            for action in actions {
                match Self::execute_action(&client_state, &action).await {
                    Ok(result) => match result {
                        ClientActionResult::Custom { name, data } => {
                            if name == "send_apdu" {
                                // Execute APDU command
                                if let Err(e) =
                                    Self::handle_send_apdu(&client_state, &llm_client, &app_state, &status_tx, client_id, &data).await
                                {
                                    error!("Failed to send APDU: {}", e);
                                    let _ = status_tx.send(format!("APDU error: {}", e));
                                }
                            } else if name == "read_ndef" {
                                // Read NDEF message
                                if let Err(e) = Self::handle_read_ndef(&client_state, &llm_client, &app_state, &status_tx, client_id).await {
                                    error!("Failed to read NDEF: {}", e);
                                    let _ = status_tx.send(format!("NDEF read error: {}", e));
                                }
                            } else if name == "write_ndef" {
                                // Write NDEF message
                                if let Err(e) = Self::handle_write_ndef(&client_state, &data).await {
                                    error!("Failed to write NDEF: {}", e);
                                    let _ = status_tx.send(format!("NDEF write error: {}", e));
                                }
                            }
                        }
                        ClientActionResult::Disconnect => {
                            info!("Disconnecting NFC card");
                            let mut state = client_state.lock().await;
                            state.card = None;
                            drop(state);

                            // Notify LLM
                            let event = Event::new(&NFC_CARD_DISCONNECTED_EVENT, json!({}));
                            let _ = call_llm_for_client(
                                llm_client.clone(),
                                app_state.clone(),
                                status_tx.clone(),
                                client_id,
                                Some(&event),
                            )
                            .await;
                        }
                        ClientActionResult::WaitForMore => {
                            // Do nothing
                        }
                        _ => {
                            warn!("Unhandled action result: {:?}", result);
                        }
                    },
                    Err(e) => {
                        error!("Failed to execute action: {}", e);
                        let _ = status_tx.send(format!("Action error: {}", e));
                    }
                }
            }

            app_state.clear_pending_client_actions(client_id).await;
        }

        Ok(())
    }

    /// Execute an action
    #[cfg(feature = "nfc-client")]
    async fn execute_action(
        _client_state: &Arc<Mutex<NfcClientState>>,
        action: &Value,
    ) -> Result<ClientActionResult> {
        let protocol = NfcClientProtocol;
        use crate::llm::actions::client_trait::Client;
        protocol.execute_action(action.clone())
    }

    /// Handle sending APDU command
    #[cfg(feature = "nfc-client")]
    async fn handle_send_apdu(
        client_state: &Arc<Mutex<NfcClientState>>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        data: &Value,
    ) -> Result<()> {
        let apdu_hex = data["apdu_hex"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing apdu_hex"))?;

        trace!("Sending APDU: {}", apdu_hex);

        let mut state = client_state.lock().await;

        // Ensure card is connected
        let card = state
            .card
            .as_mut()
            .ok_or_else(|| anyhow!("No card connected. Use 'connect_card' first."))?;

        // Decode hex APDU
        let apdu_bytes = hex::decode(apdu_hex.trim())?;

        // Send APDU
        let mut response_buf = [0u8; pcsc::MAX_BUFFER_SIZE];
        let response = card
            .transmit(&apdu_bytes, &mut response_buf)
            .context("Failed to transmit APDU")?;

        let response_hex = hex::encode(response);
        trace!("APDU response: {}", response_hex);

        // Parse status bytes (SW1 SW2)
        if response.len() < 2 {
            return Err(anyhow!("Invalid APDU response (too short)"));
        }

        let sw1 = response[response.len() - 2];
        let sw2 = response[response.len() - 1];
        let data_bytes = &response[..response.len() - 2];
        let data_hex = if !data_bytes.is_empty() {
            Some(hex::encode(data_bytes))
        } else {
            None
        };

        drop(state);

        // Send event to LLM
        let event = Event::new(
            &NFC_APDU_RESPONSE_EVENT,
            json!({
                "response_hex": response_hex,
                "sw1": format!("{:02X}", sw1),
                "sw2": format!("{:02X}", sw2),
                "data_hex": data_hex,
            }),
        );

        call_llm_for_client(
            llm_client.clone(),
            app_state.clone(),
            status_tx.clone(),
            client_id,
            Some(&event),
        )
        .await?;

        Ok(())
    }

    /// Handle reading NDEF message
    #[cfg(feature = "nfc-client")]
    async fn handle_read_ndef(
        _client_state: &Arc<Mutex<NfcClientState>>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _client_id: ClientId,
    ) -> Result<()> {
        // TODO: Implement NDEF reading via APDU commands
        // This requires:
        // 1. SELECT NDEF application (D2760000850101)
        // 2. SELECT Capability Container file
        // 3. READ Capability Container
        // 4. SELECT NDEF file
        // 5. READ NDEF message
        // 6. Parse NDEF using ndef-rs

        let _ = status_tx.send("NDEF reading not yet implemented".to_string());
        Err(anyhow!("NDEF reading not yet implemented"))
    }

    /// Handle writing NDEF message
    #[cfg(feature = "nfc-client")]
    async fn handle_write_ndef(
        _client_state: &Arc<Mutex<NfcClientState>>,
        _data: &Value,
    ) -> Result<()> {
        // TODO: Implement NDEF writing
        Err(anyhow!("NDEF writing not yet implemented"))
    }
}

/// NFC client state
#[cfg(feature = "nfc-client")]
struct NfcClientState {
    ctx: pcsc::Context,
    reader_name: String,
    card: Option<pcsc::Card>,
    connection_state: ClientConnectionState,
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
