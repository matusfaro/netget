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
use crate::protocol::Event;
use crate::server::NtpProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_trace};
use actions::NTP_REQUEST_EVENT;

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
        let _ = status_tx.send(format!("[INFO] NTP server listening on {}", local_addr));

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            // Large enough for extension fields / an authentication MAC. Only the first
            // 48 bytes are interpreted, but a short buffer would silently truncate the
            // datagram and misreport its size to the model.
            let mut buffer = vec![0u8; 1024];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (NTP "connection" = recent client)
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        console_debug!(status_tx, "NTP received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (always hex for NTP)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "NTP data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        tokio::spawn(async move {
                            // Get current Unix timestamp
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let current_unix_time = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();

                            // Version (bits 5-3) and mode (bits 2-0) of the request. The
                            // reply must come back in the client's own version.
                            let (client_version, client_mode) = match data.first() {
                                Some(b) => ((b >> 3) & 0x07, b & 0x07),
                                None => (4, 3),
                            };

                            // Parse client's transmit timestamp from request (bytes 40-47).
                            // RFC 5905 requires this to come back verbatim as the reply's
                            // origin timestamp; the client uses it to match the response to
                            // its request and rejects anything else.
                            let (client_transmit_unix, client_transmit_ntp) = if data.len() >= 48 {
                                let seconds =
                                    u32::from_be_bytes([data[40], data[41], data[42], data[43]])
                                        as u64;
                                let fraction =
                                    u32::from_be_bytes([data[44], data[45], data[46], data[47]])
                                        as u64;
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

                            // One protocol instance per request, carrying that request's
                            // origin timestamp and version, so overlapping requests cannot
                            // pick up each other's values.
                            let protocol =
                                NtpProtocol::for_request(client_transmit_ntp, client_version);

                            // Create NTP request event
                            let mut event_data = serde_json::json!({
                                "current_time": current_unix_time,
                                "client_version": client_version,
                                "client_mode": client_mode,
                                "bytes_received": data.len()
                            });

                            // Expose the raw 64-bit NTP value, which is what
                            // origin_timestamp needs; the Unix form is lossy and is
                            // provided only for readability.
                            if let Some(ntp_ts) = client_transmit_ntp {
                                event_data["client_transmit_timestamp"] = serde_json::json!(ntp_ts);
                            }
                            if let Some(unix_ts) = client_transmit_unix {
                                event_data["client_transmit_unix"] = serde_json::json!(unix_ts);
                            }

                            let event = Event::new(&NTP_REQUEST_EVENT, event_data);

                            debug!("NTP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!(
                                "[DEBUG] NTP calling LLM for request from {}",
                                peer_addr
                            ));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None, // NTP uses UDP, no persistent connection
                                &event,
                                &protocol,
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!(
                                        "NTP parsed {} actions",
                                        execution_result.raw_actions.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] NTP parsed {} actions",
                                        execution_result.raw_actions.len()
                                    ));

                                    // Process protocol results
                                    debug!(
                                        "NTP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] NTP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    ));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) =
                                            protocol_result.get_all_output().first()
                                        {
                                            let _ =
                                                socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!(
                                                "NTP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            );
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] NTP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            ));

                                            // TRACE: Log full payload (always hex for NTP)
                                            let hex_str = hex::encode(output_data);
                                            trace!("NTP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!(
                                                "[TRACE] NTP sent (hex): {}",
                                                hex_str
                                            ));

                                            let _ = status_clone.send(format!(
                                                "→ NTP response to {} ({} bytes)",
                                                peer_addr,
                                                output_data.len()
                                            ));
                                        } else {
                                            debug!("NTP protocol result has no output data");
                                            let _ = status_clone.send(
                                                "[DEBUG] NTP protocol result has no output data"
                                                    .to_string(),
                                            );
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

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}
