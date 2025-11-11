//! DNS server implementation using hickory-server
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use actions::DNS_QUERY_EVENT;
use crate::server::DnsProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use anyhow::Result;
use hickory_proto::op::Message as DnsMessage;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// DNS server that integrates with LLM for query handling
pub struct DnsServer;

impl DnsServer {
    /// Spawn DNS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("DNS server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(DnsProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 512]; // Standard DNS packet size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (DNS "connection" = recent query)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
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
                        console_debug!(status_tx, "[DEBUG] DNS received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (hex for binary DNS)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "[TRACE] DNS data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Parse DNS query using hickory-proto
                            match DnsMessage::from_vec(&data) {
                                Ok(query) => {
                                    // Extract query information
                                    let query_id = query.id();
                                    let queries = query.queries();

                                    let mut query_descriptions = Vec::new();
                                    for q in queries {
                                        let qname = q.name().to_string();
                                        let qtype = q.query_type();
                                        let qclass = q.query_class();
                                        query_descriptions.push(format!(
                                            "{} {} {} (ID: {})",
                                            qname, qtype, qclass, query_id
                                        ));

                                        // DEBUG: Log parsed query
                                        debug!("DNS query: {} {} {}", qname, qtype, qclass);
                                        let _ = status_clone.send(format!(
                                            "[DEBUG] DNS query: {} {} {}",
                                            qname, qtype, qclass
                                        ));
                                    }

                                    // Create DNS query event
                                    let first_query = queries.first();
                                    let domain = first_query.map(|q| q.name().to_string()).unwrap_or_default();
                                    let query_type = first_query.map(|q| q.query_type().to_string()).unwrap_or_default();

                                    let event = Event::new(&DNS_QUERY_EVENT, serde_json::json!({
                                        "query_id": query_id,
                                        "domain": domain,
                                        "query_type": query_type
                                    }));

                                    debug!("DNS calling LLM for query from {}", peer_addr);
                                    let _ = status_clone.send(format!("[DEBUG] DNS calling LLM for query from {}", peer_addr));

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

                                            debug!("DNS got {} protocol results", execution_result.protocol_results.len());
                                            let _ = status_clone.send(format!("[DEBUG] DNS got {} protocol results", execution_result.protocol_results.len()));

                                            for protocol_result in execution_result.protocol_results {
                                                                if let Some(output_data) = protocol_result.get_all_output().first() {
                                                                    let _ = socket_clone.send_to(output_data, peer_addr).await;

                                                                    // DEBUG: Log summary
                                                                    debug!("DNS sent {} bytes to {}", output_data.len(), peer_addr);
                                                                    let _ = status_clone.send(format!("[DEBUG] DNS sent {} bytes to {}", output_data.len(), peer_addr));

                                                                    // TRACE: Log full payload (hex for binary DNS)
                                                                    let hex_str = hex::encode(output_data);
                                                                    trace!("DNS sent (hex): {}", hex_str);
                                                                    let _ = status_clone.send(format!("[TRACE] DNS sent (hex): {}", hex_str));

                                                                    let _ = status_clone.send(format!("→ DNS response to {} ({} bytes)", peer_addr, output_data.len()));
                                                } else {
                                                    debug!("DNS protocol result has no output data");
                                                    let _ = status_clone.send("[DEBUG] DNS protocol result has no output data".to_string());
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("DNS LLM call failed: {}", e);
                                            let _ = status_clone.send(format!("✗ DNS LLM error: {e}"));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse DNS query: {}", e);
                                    let _ = status_clone.send(format!("✗ Failed to parse DNS query: {e}"));

                                    // Fall back to hex representation for malformed queries
                                    let hex_str = hex::encode(&data);
                                    let event_description = format!(
                                        "Malformed DNS query from {} ({} bytes, hex: {})",
                                        peer_addr, data.len(), hex_str
                                    );

                                    debug!("DNS malformed query: {}", event_description);
                                    let _ = status_clone.send(format!("[DEBUG] {event_description}"));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("DNS receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Legacy spawn method - deprecated
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = UdpSocket::bind(listen_addr).await?;
        let local_addr = socket.local_addr()?;

        console_error!(status_tx, "✗ DNS legacy mode no longer supported, please restart with action-based mode");

        Ok(local_addr)
    }
}
