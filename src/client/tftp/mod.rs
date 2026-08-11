//! TFTP (RFC 1350) client implementation
//!
//! The LLM drives the transfer: it chooses the operation (`tftp_read_file` /
//! `tftp_write_file`), acknowledges each inbound DATA block (`send_ack`), and supplies each
//! outbound DATA block (`send_data_block`). NetGet owns only the UDP socket and the packet
//! framing.

pub mod actions;
pub use actions::TftpClientProtocol;

use crate::client::llm_budget::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use actions::{
    TFTP_CLIENT_ACK_RECEIVED_EVENT, TFTP_CLIENT_CONNECTED_EVENT, TFTP_CLIENT_DATA_RECEIVED_EVENT,
    TFTP_CLIENT_ERROR_EVENT, TFTP_CLIENT_TRANSFER_COMPLETE_EVENT,
};

/// TFTP opcodes (RFC 1350 §5)
pub const OP_RRQ: u16 = 1;
pub const OP_WRQ: u16 = 2;
pub const OP_DATA: u16 = 3;
pub const OP_ACK: u16 = 4;
pub const OP_ERROR: u16 = 5;

/// Maximum payload of a TFTP DATA block; a shorter block terminates the transfer.
pub const TFTP_BLOCK_SIZE: usize = 512;

/// Build an RRQ (opcode 1) or WRQ (opcode 2) packet.
///
/// `opcode | filename | 0 | mode | 0`
pub fn build_request_packet(opcode: u16, filename: &str, mode: &str) -> Vec<u8> {
    let mut packet = Vec::with_capacity(4 + filename.len() + mode.len());
    packet.extend_from_slice(&opcode.to_be_bytes());
    packet.extend_from_slice(filename.as_bytes());
    packet.push(0);
    packet.extend_from_slice(mode.as_bytes());
    packet.push(0);
    packet
}

/// A decoded inbound TFTP packet.
#[derive(Debug, Clone, PartialEq)]
pub enum TftpPacket {
    Data { block: u16, payload: Vec<u8> },
    Ack { block: u16 },
    Error { code: u16, message: String },
}

impl TftpPacket {
    /// Decode a packet received from the server. Returns `None` for opcodes a client
    /// never receives (RRQ/WRQ) or for a truncated datagram.
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let opcode = u16::from_be_bytes([data[0], data[1]]);
        let block = u16::from_be_bytes([data[2], data[3]]);
        match opcode {
            OP_DATA => Some(TftpPacket::Data {
                block,
                payload: data[4..].to_vec(),
            }),
            OP_ACK => Some(TftpPacket::Ack { block }),
            OP_ERROR => {
                // ErrorCode(2) then a NUL-terminated netascii message.
                let msg_bytes = &data[4..];
                let end = msg_bytes
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(msg_bytes.len());
                Some(TftpPacket::Error {
                    code: block,
                    message: String::from_utf8_lossy(&msg_bytes[..end]).to_string(),
                })
            }
            _ => None,
        }
    }
}

/// Which direction the current transfer runs in.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Read,
    Write,
}

/// Per-client LLM state (mirrors the per-connection state machine servers use).
struct ClientData {
    memory: String,
}

pub struct TftpClient;

impl TftpClient {
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        let server_addr: SocketAddr = remote_addr
            .parse()
            .with_context(|| format!("Invalid TFTP server address: {}", remote_addr))?;

        let socket = Arc::new(
            UdpSocket::bind("0.0.0.0:0")
                .await
                .context("Failed to bind TFTP client UDP socket")?,
        );
        let local_addr = socket.local_addr()?;

        info!(
            "TFTP client {} bound to {} (server {})",
            client_id, local_addr, server_addr
        );
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] TFTP client {} bound to {}",
            client_id, local_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let client_data = Arc::new(Mutex::new(ClientData {
            memory: String::new(),
        }));
        let protocol = Arc::new(TftpClientProtocol::new());

        // Ask the model what to do with this server.
        let event = Event::new(
            &TFTP_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "server_addr": server_addr.to_string(),
                "local_addr": local_addr.to_string(),
            }),
        );

        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();

        let memory_snapshot = client_data.lock().await.memory.clone();
        let llm_result = call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory_snapshot,
            Some(&event),
            protocol.as_ref() as &dyn Client,
            &status_tx,
        )
        .await;

        let actions = match llm_result {
            Ok(ClientLlmResult {
                actions,
                memory_updates,
            }) => {
                if let Some(mem) = memory_updates {
                    client_data.lock().await.memory = mem;
                }
                actions
            }
            Err(e) => {
                error!("TFTP client {} LLM error on connect: {}", client_id, e);
                let _ = status_tx.send(format!("[CLIENT] TFTP LLM error on connect: {}", e));
                Vec::new()
            }
        };

        // Only a read or write request starts a transfer. Anything else leaves the socket
        // bound and idle — the client never invents a request the model did not ask for.
        let mut started = false;
        for action in actions {
            match protocol.execute_action(action) {
                Ok(ClientActionResult::Custom { name, data }) => {
                    let filename = data
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let mode = data
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("octet")
                        .to_string();
                    if filename.is_empty() {
                        warn!("TFTP client {} got {} with empty filename", client_id, name);
                        continue;
                    }
                    let direction = match name.as_str() {
                        "tftp_read_file" => Direction::Read,
                        "tftp_write_file" => Direction::Write,
                        other => {
                            warn!(
                                "TFTP client {} unhandled custom action {}",
                                client_id, other
                            );
                            continue;
                        }
                    };

                    let handle = tokio::spawn(Self::run_transfer(
                        direction,
                        socket.clone(),
                        server_addr,
                        filename,
                        mode,
                        llm_client.clone(),
                        app_state.clone(),
                        status_tx.clone(),
                        client_id,
                        instruction.clone(),
                        client_data.clone(),
                        protocol.clone(),
                    ));
                    app_state.register_client_task(client_id, handle).await;
                    started = true;
                }
                Ok(ClientActionResult::Disconnect) => {
                    info!("TFTP client {} disconnecting on model request", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    return Ok(local_addr);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("TFTP client {} rejected action: {}", client_id, e);
                }
            }
        }

        if !started {
            debug!(
                "TFTP client {} idle: model requested no transfer",
                client_id
            );
        }

        Ok(local_addr)
    }

    /// Drive one RRQ or WRQ transfer to completion.
    #[allow(clippy::too_many_arguments)]
    async fn run_transfer(
        direction: Direction,
        socket: Arc<UdpSocket>,
        server_addr: SocketAddr,
        filename: String,
        mode: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: String,
        client_data: Arc<Mutex<ClientData>>,
        protocol: Arc<TftpClientProtocol>,
    ) {
        let opcode = match direction {
            Direction::Read => OP_RRQ,
            Direction::Write => OP_WRQ,
        };
        let request = build_request_packet(opcode, &filename, &mode);
        if let Err(e) = socket.send_to(&request, server_addr).await {
            error!("TFTP client {} failed to send request: {}", client_id, e);
            app_state
                .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                .await;
            return;
        }
        debug!(
            "TFTP client {} sent {} for '{}' ({})",
            client_id,
            if opcode == OP_RRQ { "RRQ" } else { "WRQ" },
            filename,
            mode
        );
        let _ = status_tx.send(format!(
            "[CLIENT] TFTP {} '{}'",
            if opcode == OP_RRQ { "RRQ" } else { "WRQ" },
            filename
        ));

        // The server answers from a freshly allocated TID, so the peer address of the first
        // reply — not the well-known port — is where subsequent packets go (RFC 1350 §4).
        let mut transfer_addr = server_addr;
        let mut learned_tid = false;

        let mut buffer = vec![0u8; 4 + TFTP_BLOCK_SIZE];
        let mut total_bytes: u64 = 0;
        let mut total_blocks: u16 = 0;

        loop {
            let received = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                socket.recv_from(&mut buffer),
            )
            .await;

            let (n, peer) = match received {
                Ok(Ok(pair)) => pair,
                Ok(Err(e)) => {
                    error!("TFTP client {} socket error: {}", client_id, e);
                    app_state
                        .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                        .await;
                    return;
                }
                Err(_) => {
                    warn!("TFTP client {} timed out waiting for a reply", client_id);
                    let _ = status_tx.send("[CLIENT] TFTP timeout".to_string());
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    return;
                }
            };

            if !learned_tid {
                transfer_addr = peer;
                learned_tid = true;
                trace!("TFTP client {} learned server TID {}", client_id, peer);
            }

            let packet = match TftpPacket::decode(&buffer[..n]) {
                Some(p) => p,
                None => {
                    debug!("TFTP client {} ignoring undecodable datagram", client_id);
                    continue;
                }
            };

            let (event, terminal) = match &packet {
                TftpPacket::Data { block, payload } => {
                    let is_final = payload.len() < TFTP_BLOCK_SIZE;
                    total_bytes += payload.len() as u64;
                    total_blocks = *block;
                    (
                        Event::new(
                            &TFTP_CLIENT_DATA_RECEIVED_EVENT,
                            serde_json::json!({
                                "block_number": block,
                                "data_hex": hex::encode(payload),
                                "data_length": payload.len(),
                                "is_final": is_final,
                                "total_bytes": total_bytes,
                            }),
                        ),
                        is_final,
                    )
                }
                TftpPacket::Ack { block } => {
                    total_blocks = *block;
                    (
                        Event::new(
                            &TFTP_CLIENT_ACK_RECEIVED_EVENT,
                            serde_json::json!({ "block_number": block }),
                        ),
                        false,
                    )
                }
                TftpPacket::Error { code, message } => {
                    error!(
                        "TFTP client {} received ERROR {}: {}",
                        client_id, code, message
                    );
                    let _ = status_tx.send(format!("[CLIENT] TFTP error {}: {}", code, message));
                    (
                        Event::new(
                            &TFTP_CLIENT_ERROR_EVENT,
                            serde_json::json!({
                                "error_code": code,
                                "error_message": message,
                            }),
                        ),
                        true,
                    )
                }
            };

            let memory_snapshot = client_data.lock().await.memory.clone();
            let llm_result = call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory_snapshot,
                Some(&event),
                protocol.as_ref() as &dyn Client,
                &status_tx,
            )
            .await;

            let mut disconnect = false;
            let mut wrote_short_block = false;
            match llm_result {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }
                    for action in actions {
                        match protocol.execute_action(action) {
                            Ok(ClientActionResult::SendData(bytes)) => {
                                // A DATA block shorter than 512 bytes ends a write transfer.
                                if bytes.len() >= 4
                                    && u16::from_be_bytes([bytes[0], bytes[1]]) == OP_DATA
                                {
                                    let payload_len = bytes.len() - 4;
                                    total_bytes += payload_len as u64;
                                    if payload_len < TFTP_BLOCK_SIZE {
                                        wrote_short_block = true;
                                    }
                                }
                                if let Err(e) = socket.send_to(&bytes, transfer_addr).await {
                                    error!("TFTP client {} send failed: {}", client_id, e);
                                }
                            }
                            Ok(ClientActionResult::Disconnect) => disconnect = true,
                            Ok(_) => {}
                            Err(e) => {
                                warn!("TFTP client {} rejected action: {}", client_id, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("TFTP client {} LLM error: {}", client_id, e);
                }
            }

            if terminal || disconnect || wrote_short_block {
                if !matches!(packet, TftpPacket::Error { .. }) {
                    let complete = Event::new(
                        &TFTP_CLIENT_TRANSFER_COMPLETE_EVENT,
                        serde_json::json!({
                            "total_bytes": total_bytes,
                            "total_blocks": total_blocks,
                        }),
                    );
                    let memory_snapshot = client_data.lock().await.memory.clone();
                    if let Err(e) = call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory_snapshot,
                        Some(&complete),
                        protocol.as_ref() as &dyn Client,
                        &status_tx,
                    )
                    .await
                    {
                        error!("TFTP client {} LLM error on completion: {}", client_id, e);
                    }
                    info!(
                        "TFTP client {} transfer complete ({} bytes, {} blocks)",
                        client_id, total_bytes, total_blocks
                    );
                    let _ = status_tx.send(format!(
                        "[CLIENT] TFTP transfer complete: {} bytes",
                        total_bytes
                    ));
                }
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
                return;
            }
        }
    }
}
