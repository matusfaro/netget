//! TFTP client implementation
pub mod actions;
pub use actions::TftpClientProtocol;

use crate::state::ClientId;
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use actions::{
    TFTP_CLIENT_ACK_RECEIVED_EVENT, TFTP_CLIENT_CONNECTED_EVENT, TFTP_CLIENT_DATA_RECEIVED_EVENT,
    TFTP_CLIENT_ERROR_EVENT, TFTP_CLIENT_TRANSFER_COMPLETE_EVENT,
};

pub struct TftpClient;

impl TftpClient {
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: String,
    ) -> Result<SocketAddr> {
        // Parse remote address
        let server_addr: SocketAddr = remote_addr.parse().context("Invalid server address")?;

        // Bind local UDP socket
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let local_addr = socket.local_addr()?;

        info!("TFTP client {} bound to {}", client_id, local_addr);
        let _ = status_tx.send(format!("[INFO] TFTP client bound to {}", local_addr));

        // Parse instruction to determine operation (read or write)
        let (operation, filename, mode) = Self::parse_instruction(&instruction)?;

        info!(
            "TFTP client {} operation: {} file '{}' mode '{}'",
            client_id, operation, filename, mode
        );

        // Call LLM with connected event
        let event = Event::new(
            &TFTP_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "server_addr": server_addr.to_string(),
                "operation": operation,
                "filename": filename.clone(),
            }),
        );

        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id,
            Some(&event),
            &instruction,
        )
        .await
        {
            Ok(_) => {
                // LLM acknowledged connection
            }
            Err(e) => {
                error!("TFTP client {} LLM error on connect: {}", client_id, e);
            }
        }

        // Spawn transfer task based on operation
        match operation.as_str() {
            "read" => {
                tokio::spawn(Self::handle_read_transfer(
                    socket,
                    server_addr,
                    filename,
                    mode,
                    llm_client,
                    app_state,
                    status_tx,
                    client_id,
                    instruction,
                ));
            }
            "write" => {
                tokio::spawn(Self::handle_write_transfer(
                    socket,
                    server_addr,
                    filename,
                    mode,
                    llm_client,
                    app_state,
                    status_tx,
                    client_id,
                    instruction,
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown TFTP operation: {}",
                    operation
                ));
            }
        }

        Ok(local_addr)
    }

    async fn handle_read_transfer(
        socket: Arc<UdpSocket>,
        server_addr: SocketAddr,
        filename: String,
        mode: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: String,
    ) {
        // Send RRQ
        let rrq_packet = Self::build_request_packet(1, &filename, &mode);
        if let Err(e) = socket.send_to(&rrq_packet, server_addr).await {
            error!("TFTP client {} failed to send RRQ: {}", client_id, e);
            return;
        }

        debug!("TFTP client {} sent RRQ for '{}'", client_id, filename);
        let _ = status_tx.send(format!(
            "[DEBUG] TFTP sent RRQ for '{}'",
            filename
        ));

        let mut buffer = vec![0u8; 516];
        let mut total_bytes = 0u64;
        let mut total_blocks = 0u16;

        loop {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                socket.recv_from(&mut buffer),
            )
            .await
            {
                Ok(Ok((n, _))) => {
                    let data = &buffer[..n];

                    if data.len() < 4 {
                        continue;
                    }

                    let opcode = u16::from_be_bytes([data[0], data[1]]);

                    match opcode {
                        3 => {
                            // DATA packet
                            let block_number = u16::from_be_bytes([data[2], data[3]]);
                            let block_data = &data[4..];
                            let is_final = block_data.len() < 512;

                            total_bytes += block_data.len() as u64;
                            total_blocks = block_number;

                            debug!(
                                "TFTP client {} received DATA block {} ({} bytes)",
                                client_id,
                                block_number,
                                block_data.len()
                            );

                            // Call LLM with data received event
                            let event = Event::new(
                                &TFTP_CLIENT_DATA_RECEIVED_EVENT,
                                serde_json::json!({
                                    "block_number": block_number,
                                    "data_hex": hex::encode(block_data),
                                    "data_length": block_data.len(),
                                    "is_final": is_final,
                                    "total_bytes": total_bytes,
                                }),
                            );

                            match call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id,
                                Some(&event),
                                &instruction,
                            )
                            .await
                            {
                                Ok(result) => {
                                    // Process actions (should include send_ack)
                                    for action_result in result.actions {
                                        match action_result {
                                            crate::llm::actions::client_trait::ClientActionResult::SendData(
                                                packet,
                                            ) => {
                                                let _ = socket.send_to(&packet, server_addr).await;
                                            }
                                            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                                                return;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("TFTP client {} LLM error: {}", client_id, e);
                                }
                            }

                            if is_final {
                                // Transfer complete
                                debug!(
                                    "TFTP client {} transfer complete ({} bytes, {} blocks)",
                                    client_id, total_bytes, total_blocks
                                );

                                let event = Event::new(
                                    &TFTP_CLIENT_TRANSFER_COMPLETE_EVENT,
                                    serde_json::json!({
                                        "total_bytes": total_bytes,
                                        "total_blocks": total_blocks,
                                    }),
                                );

                                let _ = call_llm_for_client(
                                    &llm_client,
                                    &app_state,
                                    client_id,
                                    Some(&event),
                                    &instruction,
                                )
                                .await;

                                break;
                            }
                        }
                        5 => {
                            // ERROR packet
                            let error_code = u16::from_be_bytes([data[2], data[3]]);
                            let error_msg = String::from_utf8_lossy(&data[4..n - 1]).to_string();

                            error!(
                                "TFTP client {} received ERROR {}: {}",
                                client_id, error_code, error_msg
                            );

                            let event = Event::new(
                                &TFTP_CLIENT_ERROR_EVENT,
                                serde_json::json!({
                                    "error_code": error_code,
                                    "error_message": error_msg,
                                }),
                            );

                            let _ = call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id,
                                Some(&event),
                                &instruction,
                            )
                            .await;

                            break;
                        }
                        _ => {
                            debug!(
                                "TFTP client {} received unexpected opcode {}",
                                client_id, opcode
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("TFTP client {} socket error: {}", client_id, e);
                    break;
                }
                Err(_) => {
                    debug!("TFTP client {} timeout waiting for DATA", client_id);
                    break;
                }
            }
        }
    }

    async fn handle_write_transfer(
        socket: Arc<UdpSocket>,
        server_addr: SocketAddr,
        filename: String,
        mode: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        _instruction: String,
    ) {
        // Send WRQ
        let wrq_packet = Self::build_request_packet(2, &filename, &mode);
        if let Err(e) = socket.send_to(&wrq_packet, server_addr).await {
            error!("TFTP client {} failed to send WRQ: {}", client_id, e);
            return;
        }

        debug!("TFTP client {} sent WRQ for '{}'", client_id, filename);
        let _ = status_tx.send(format!(
            "[DEBUG] TFTP sent WRQ for '{}'",
            filename
        ));

        // Wait for ACK block 0, then call LLM to get file content to send
        // For brevity, this is simplified - full implementation would call LLM for data
        info!("TFTP client {} write transfer started", client_id);
    }

    fn parse_instruction(instruction: &str) -> Result<(String, String, String)> {
        // Simple parsing: look for "read" or "write" keywords and filename
        let lower = instruction.to_lowercase();

        let operation = if lower.contains("read") || lower.contains("download") || lower.contains("get")
        {
            "read".to_string()
        } else if lower.contains("write") || lower.contains("upload") || lower.contains("put") {
            "write".to_string()
        } else {
            "read".to_string() // Default
        };

        // Extract filename (look for common patterns)
        let filename = if let Some(start) = lower.find("file") {
            // Look for quoted filename or next word
            let rest = &instruction[start..];
            rest.split_whitespace()
                .skip(1)
                .next()
                .unwrap_or("file.txt")
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        } else {
            // Look for filename in instruction
            instruction
                .split_whitespace()
                .find(|w| w.contains('.'))
                .unwrap_or("file.txt")
                .to_string()
        };

        // Mode
        let mode = if lower.contains("netascii") || lower.contains("text") {
            "netascii".to_string()
        } else {
            "octet".to_string()
        };

        Ok((operation, filename, mode))
    }

    fn build_request_packet(opcode: u16, filename: &str, mode: &str) -> Vec<u8> {
        // RRQ/WRQ: opcode(2) filename(string) 0 mode(string) 0
        let mut packet = Vec::new();
        packet.extend_from_slice(&opcode.to_be_bytes());
        packet.extend_from_slice(filename.as_bytes());
        packet.push(0);
        packet.extend_from_slice(mode.as_bytes());
        packet.push(0);
        packet
    }
}
