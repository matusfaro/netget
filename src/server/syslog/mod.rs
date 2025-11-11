//! Syslog server implementation using syslog_loose library
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
use actions::SYSLOG_MESSAGE_EVENT;
use crate::server::SyslogProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Syslog server that forwards messages to LLM
pub struct SyslogServer;

impl SyslogServer {
    /// Spawn Syslog server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        console_info!(status_tx, "[INFO] Syslog server listening on {}", local_addr);

        let protocol = Arc::new(SyslogProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Max UDP packet size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (Syslog "connection" = recent peer)
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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        console_info!(status_tx, "__UPDATE_UI__");

                        // DEBUG: Log summary
                        console_debug!(status_tx, "[DEBUG] Syslog received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload
                        let message_str = String::from_utf8_lossy(&data);
                        console_trace!(status_tx, "[TRACE] Syslog message: {}", message_str);

                        // Parse the syslog message
                        let parsed = match Self::parse_syslog_message(&data) {
                            Ok(p) => p,
                            Err(e) => {
                                console_error!(status_tx, "[ERROR] Failed to parse syslog message: {}", e);
                                continue;
                            }
                        };

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        // Spawn task to handle message with LLM
                        tokio::spawn(async move {
                            // Create syslog_message event
                            let event = Event::new(&SYSLOG_MESSAGE_EVENT, serde_json::json!({
                                "facility": parsed.facility,
                                "severity": parsed.severity,
                                "timestamp": parsed.timestamp,
                                "hostname": parsed.hostname,
                                "appname": parsed.appname,
                                "message": parsed.message
                            }));

                            debug!("Syslog calling LLM for message from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] Syslog calling LLM for message from {}", peer_addr));

                            // Call LLM
                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("Syslog got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] Syslog got {} protocol results", execution_result.protocol_results.len()));

                                    // Syslog is typically one-way (no response needed)
                                    // But LLM can perform actions like storing, forwarding, etc.
                                }
                                Err(e) => {
                                    error!("Syslog LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ Syslog LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Syslog receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Parse syslog message and extract relevant information
    pub fn parse_syslog_message(data: &[u8]) -> Result<ParsedSyslogInfo> {
        use syslog_loose::{parse_message, ProcId, Variant};

        // Convert bytes to string
        let message_str = String::from_utf8_lossy(data);

        // Parse using syslog_loose (handles both RFC 3164 and RFC 5424)
        // Try RFC 5424 first, then fall back to RFC 3164
        let parsed = parse_message(&message_str, Variant::Either);

        // Extract facility and severity strings
        let facility = match parsed.facility {
            Some(f) => format!("{:?}", f).to_lowercase(),
            None => "user".to_string(),
        };
        let severity = match parsed.severity {
            Some(s) => format!("{:?}", s).to_lowercase(),
            None => "notice".to_string(),
        };

        // Format timestamp
        let timestamp = match &parsed.timestamp {
            Some(ts) => format!("{:?}", ts),
            None => "unknown".to_string(),
        };

        // Extract hostname
        let hostname = parsed.hostname.map(|s| s.to_string()).unwrap_or_else(|| "unknown".to_string());

        // Extract app name (try structured data first, then process for RFC 3164)
        let appname = match &parsed.appname {
            Some(name) => name.to_string(),
            None => "unknown".to_string(),
        };

        // Extract process ID if available
        let _procid = match &parsed.procid {
            Some(ProcId::PID(pid)) => Some(format!("{}", pid)),
            Some(ProcId::Name(name)) => Some(name.to_string()),
            None => None,
        };

        // Extract message text
        let message_text = parsed.msg.to_string();

        Ok(ParsedSyslogInfo {
            facility,
            severity,
            timestamp,
            hostname,
            appname,
            message: message_text,
        })
    }
}

/// Parsed syslog message information
#[derive(Debug)]
pub struct ParsedSyslogInfo {
    pub facility: String,
    pub severity: String,
    pub timestamp: String,
    pub hostname: String,
    pub appname: String,
    pub message: String,
}
