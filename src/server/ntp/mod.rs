//! NTP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::NTP_REQUEST_EVENT;
use crate::server::NtpProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;


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

    /// Spawn NTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("NTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(NtpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 48];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance (NTP "connection" = recent client)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Ntp {
                                recent_clients: vec![(peer_addr, now)],
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        debug!("NTP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] NTP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for NTP)
                        let hex_str = hex::encode(&data);
                        trace!("NTP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] NTP data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Get current Unix timestamp
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let current_unix_time = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();

                            // Parse client's transmit timestamp from request (bytes 40-47)
                            // This should be echoed back as origin_timestamp in the response
                            // Extract full 64-bit NTP timestamp (seconds + fraction)
                            let (client_transmit_unix, client_transmit_ntp) = if data.len() >= 48 {
                                let seconds = u32::from_be_bytes([data[40], data[41], data[42], data[43]]) as u64;
                                let fraction = u32::from_be_bytes([data[44], data[45], data[46], data[47]]) as u64;
                                let ntp_timestamp = (seconds << 32) | fraction; // Full 64-bit NTP timestamp

                                // Convert seconds part to Unix timestamp for the LLM prompt
                                let unix_ts = if seconds > 2_208_988_800 {
                                    Some(seconds - 2_208_988_800)
                                } else {
                                    None
                                };

                                (unix_ts, Some(ntp_timestamp))
                            } else {
                                (None, None)
                            };

                            // Create NTP request event
                            let mut event_data = serde_json::json!({
                                "current_time": current_unix_time,
                                "bytes_received": data.len()
                            });

                            if let Some(unix_ts) = client_transmit_unix {
                                event_data["client_transmit_timestamp"] = serde_json::json!(unix_ts);
                            }

                            let event = Event::new(&NTP_REQUEST_EVENT, event_data);

                            debug!("NTP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] NTP calling LLM for request from {}", peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,  // NTP uses UDP, no persistent connection
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(mut execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("NTP parsed {} actions", execution_result.raw_actions.len());
                                    let _ = status_clone.send(format!("[DEBUG] NTP parsed {} actions", execution_result.raw_actions.len()));

                                    // Auto-inject client's transmit timestamp as origin_timestamp if LLM didn't provide it
                                    if let Some(ntp_ts) = client_transmit_ntp {
                                        for action in &mut execution_result.raw_actions {
                                            if action.get("type").and_then(|v| v.as_str()) == Some("send_ntp_time_response") {
                                                // Only set if LLM didn't provide origin_timestamp
                                                if !action.get("origin_timestamp").is_some() {
                                                    if let Some(obj) = action.as_object_mut() {
                                                        // Insert raw NTP timestamp (will be recognized as NTP format in parse_timestamp)
                                                        obj.insert("origin_timestamp".to_string(), serde_json::json!(ntp_ts));
                                                        debug!("NTP auto-injected origin_timestamp: 0x{:016x}", ntp_ts);
                                                        let _ = status_clone.send(format!("[DEBUG] NTP auto-injected origin_timestamp: 0x{:016x}", ntp_ts));
                                                    }
                                                } else {
                                                    debug!("NTP using LLM-provided origin_timestamp");
                                                    let _ = status_clone.send("[DEBUG] NTP using LLM-provided origin_timestamp".to_string());
                                                }
                                            }
                                        }
                                    }

                                    // Process protocol results
                                    debug!("NTP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] NTP got {} protocol results", execution_result.protocol_results.len()));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!("NTP sent {} bytes to {}", output_data.len(), peer_addr);
                                            let _ = status_clone.send(format!("[DEBUG] NTP sent {} bytes to {}", output_data.len(), peer_addr));

                                            // TRACE: Log full payload (always hex for NTP)
                                            let hex_str = hex::encode(output_data);
                                            trace!("NTP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] NTP sent (hex): {}", hex_str));

                                            let _ = status_clone.send(format!("→ NTP response to {} ({} bytes)", peer_addr, output_data.len()));
                                        } else {
                                            debug!("NTP protocol result has no output data");
                                            let _ = status_clone.send("[DEBUG] NTP protocol result has no output data".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("NTP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ NTP LLM error: {}", e));
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
