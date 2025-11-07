//! NTP client implementation
pub mod actions;

pub use actions::NtpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::ntp::actions::NTP_CLIENT_RESPONSE_RECEIVED_EVENT;

/// NTP client that queries NTP servers
pub struct NtpClient;

impl NtpClient {
    /// Connect to an NTP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote address
        let remote_sock_addr: SocketAddr = remote_addr
            .parse()
            .context(format!("Invalid NTP server address: {}", remote_addr))?;

        // Bind to any local port for UDP
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("Failed to bind UDP socket")?;

        let local_addr = socket.local_addr()?;

        info!("NTP client {} connected to {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] NTP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Wrap socket in Arc for sharing
        let socket = Arc::new(socket);
        let socket_clone = socket.clone();

        // Spawn task to handle LLM-directed queries
        tokio::spawn(async move {
            // Initial LLM call to get first action (usually query_time)
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(crate::client::ntp::actions::NtpClientProtocol::new());

                // Call LLM with connected event
                match call_llm_for_client(
                    &llm_client,
                    &app_state,
                    client_id.to_string(),
                    &instruction,
                    "",  // No memory initially
                    None,  // No event for initial call
                    protocol.as_ref(),
                    &status_tx,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates: _ }) => {
                        // Execute initial actions
                        for action in actions {
                            use crate::llm::actions::client_trait::Client;
                            match protocol.as_ref().execute_action(action) {
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data: _ }) if name == "ntp_query" => {
                                    // Send NTP query
                                    let ntp_packet = Self::build_ntp_request();
                                    if let Ok(_) = socket_clone.send_to(&ntp_packet, remote_sock_addr).await {
                                        trace!("NTP client {} sent query to {}", client_id, remote_sock_addr);

                                        // Wait for response
                                        let mut buffer = vec![0u8; 48];
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(5),
                                            socket_clone.recv_from(&mut buffer)
                                        ).await {
                                            Ok(Ok((n, from_addr))) => {
                                                trace!("NTP client {} received {} bytes from {}", client_id, n, from_addr);

                                                // Parse NTP response
                                                let timestamps = Self::parse_ntp_response(&buffer[..n]);

                                                // Call LLM with response event
                                                let event = Event::new(
                                                    &NTP_CLIENT_RESPONSE_RECEIVED_EVENT,
                                                    serde_json::json!({
                                                        "origin_timestamp": timestamps.origin_timestamp,
                                                        "receive_timestamp": timestamps.receive_timestamp,
                                                        "transmit_timestamp": timestamps.transmit_timestamp,
                                                        "stratum": timestamps.stratum,
                                                        "precision": timestamps.precision,
                                                    }),
                                                );

                                                let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                                                match call_llm_for_client(
                                                    &llm_client,
                                                    &app_state,
                                                    client_id.to_string(),
                                                    &instruction,
                                                    &memory,
                                                    Some(&event),
                                                    protocol.as_ref(),
                                                    &status_tx,
                                                ).await {
                                                    Ok(ClientLlmResult { actions: _, memory_updates }) => {
                                                        // Update memory
                                                        if let Some(mem) = memory_updates {
                                                            app_state.set_memory_for_client(client_id, mem).await;
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("LLM error for NTP client {}: {}", client_id, e);
                                                    }
                                                }
                                            }
                                            Ok(Err(e)) => {
                                                error!("NTP client {} recv error: {}", client_id, e);
                                            }
                                            Err(_) => {
                                                error!("NTP client {} timed out waiting for response", client_id);
                                            }
                                        }
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                    info!("NTP client {} disconnecting", client_id);
                                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for NTP client {}: {}", client_id, e);
                    }
                }
            }

            // Mark as disconnected after query completes
            app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        });

        Ok(local_addr)
    }

    /// Build an NTP request packet (48 bytes)
    fn build_ntp_request() -> Vec<u8> {
        let mut packet = vec![0u8; 48];

        // Set LI = 0, VN = 3, Mode = 3 (client)
        packet[0] = 0x1b;  // 00 011 011 = LI=0, VN=3, Mode=3

        // Set transmit timestamp to current time
        use std::time::{SystemTime, UNIX_EPOCH};
        let unix_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Convert Unix timestamp to NTP timestamp (add NTP epoch offset)
        let ntp_timestamp = unix_time + 2_208_988_800;

        // Write transmit timestamp (bytes 40-47)
        let ntp_timestamp_bytes = ntp_timestamp.to_be_bytes();
        packet[40..44].copy_from_slice(&ntp_timestamp_bytes[4..8]);
        // Fraction field (bytes 44-47) left as 0

        packet
    }

    /// Parse NTP response packet
    fn parse_ntp_response(data: &[u8]) -> NtpTimestamps {
        if data.len() < 48 {
            return NtpTimestamps::default();
        }

        // Extract stratum (byte 1)
        let stratum = data[1];

        // Extract precision (byte 3)
        let precision = data[3] as i8;

        // Extract origin timestamp (bytes 24-31)
        let origin_seconds = u32::from_be_bytes([data[24], data[25], data[26], data[27]]) as u64;
        let _origin_fraction = u32::from_be_bytes([data[28], data[29], data[30], data[31]]) as u64;
        let origin_timestamp = if origin_seconds > 2_208_988_800 {
            origin_seconds - 2_208_988_800
        } else {
            origin_seconds
        };

        // Extract receive timestamp (bytes 32-39)
        let receive_seconds = u32::from_be_bytes([data[32], data[33], data[34], data[35]]) as u64;
        let _receive_fraction = u32::from_be_bytes([data[36], data[37], data[38], data[39]]) as u64;
        let receive_timestamp = if receive_seconds > 2_208_988_800 {
            receive_seconds - 2_208_988_800
        } else {
            receive_seconds
        };

        // Extract transmit timestamp (bytes 40-47)
        let transmit_seconds = u32::from_be_bytes([data[40], data[41], data[42], data[43]]) as u64;
        let _transmit_fraction = u32::from_be_bytes([data[44], data[45], data[46], data[47]]) as u64;
        let transmit_timestamp = if transmit_seconds > 2_208_988_800 {
            transmit_seconds - 2_208_988_800
        } else {
            transmit_seconds
        };

        NtpTimestamps {
            origin_timestamp,
            receive_timestamp,
            transmit_timestamp,
            stratum,
            precision,
        }
    }
}

#[derive(Debug, Default)]
struct NtpTimestamps {
    origin_timestamp: u64,
    receive_timestamp: u64,
    transmit_timestamp: u64,
    stratum: u8,
    precision: i8,
}
