//! NTP server implementation

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for NTP stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
    let context = r#"You are handling NTP time synchronization requests (port 123).
Respond with current time in NTP format (seconds since 1900-01-01)."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "NTP response packet data (null if no response)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// NTP packet structure (simplified)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NtpPacket {
    pub leap_indicator: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll_interval: u8,
    pub precision: i8,
    pub root_delay: u32,
    pub root_dispersion: u32,
    pub reference_id: u32,
    pub reference_timestamp: u64,
    pub origin_timestamp: u64,
    pub receive_timestamp: u64,
    pub transmit_timestamp: u64,
}

impl NtpPacket {
    /// Parse NTP packet from bytes
    #[allow(dead_code)]
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 48 {
            return None;
        }

        let first_byte = data[0];
        Some(Self {
            leap_indicator: (first_byte >> 6) & 0x3,
            version: (first_byte >> 3) & 0x7,
            mode: first_byte & 0x7,
            stratum: data[1],
            poll_interval: data[2],
            precision: data[3] as i8,
            root_delay: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            root_dispersion: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            reference_id: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            reference_timestamp: u64::from_be_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]),
            origin_timestamp: u64::from_be_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            receive_timestamp: u64::from_be_bytes([
                data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
            ]),
            transmit_timestamp: u64::from_be_bytes([
                data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
            ]),
        })
    }
}

/// NTP server that forwards requests to LLM
pub struct NtpServer;

impl NtpServer {
    /// Spawn NTP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("NTP server listening on {}", local_addr);

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 48]; // NTP packet is 48 bytes

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;
                            let prompt_config = get_llm_prompt_config();
                            let conn_memory = String::new();

                            // Build event description
                            let event_description = format!(
                                "NTP request from {} ({} bytes)",
                                peer_addr, data.len()
                            );

                            let prompt = PromptBuilder::build_network_event_prompt(
                                &state_clone,
                                connection_id,
                                &conn_memory,
                                &event_description,
                                prompt_config,
                            ).await;

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    if let Err(e) = socket_clone.send_to(llm_output.as_bytes(), peer_addr).await {
                                        error!("Failed to send NTP response: {}", e);
                                    } else {
                                        let _ = status_clone.send(format!(
                                            "→ NTP response to {} ({} bytes)",
                                            peer_addr, llm_output.len()
                                        ));
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for NTP: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for NTP: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("NTP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}