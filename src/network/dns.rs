//! DNS server implementation using hickory-server

use crate::llm::actions::protocol_trait::ProtocolActions;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{execute_actions, ActionResponse};
use crate::network::connection::ConnectionId;
use crate::network::DnsProtocol;
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
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance (DNS "connection" = recent query)
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
                            protocol_info: ProtocolConnectionInfo::Dns {
                                recent_queries: vec![("query".to_string(), now)],
                            },
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        debug!("DNS received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!(
                            "[DEBUG] DNS received {} bytes from {}",
                            n, peer_addr
                        ));

                        // TRACE: Log full payload (hex for binary DNS)
                        let hex_str = hex::encode(&data);
                        trace!("DNS data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DNS data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;

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

                                    let event_description = if query_descriptions.is_empty() {
                                        format!(
                                            "DNS query from {} ({} bytes, ID: {})",
                                            peer_addr,
                                            data.len(),
                                            query_id
                                        )
                                    } else {
                                        format!(
                                            "DNS query from {}: {}",
                                            peer_addr,
                                            query_descriptions.join(", ")
                                        )
                                    };

                                    // Get protocol actions
                                    let protocol_actions = protocol_clone.get_sync_actions();
                                    let prompt = PromptBuilder::build_network_event_action_prompt(
                                        &state_clone,
                                        &event_description,
                                        protocol_actions,
                                    )
                                    .await;

                                    // Log the full prompt being sent to LLM
                                    debug!("DNS LLM prompt:\n{}", prompt);
                                    let _ = status_clone
                                        .send(format!("[DEBUG] DNS LLM prompt:\n{}", prompt));

                                    match llm_clone.generate(&model, &prompt).await {
                                        Ok(llm_output) => {
                                            debug!("DNS LLM raw output: {}", llm_output);
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] DNS LLM raw output: {}",
                                                llm_output
                                            ));

                                            match ActionResponse::from_str(&llm_output) {
                                                Ok(action_response) => {
                                                    debug!(
                                                        "DNS parsed {} actions",
                                                        action_response.actions.len()
                                                    );
                                                    let _ = status_clone.send(format!(
                                                        "[DEBUG] DNS parsed {} actions",
                                                        action_response.actions.len()
                                                    ));

                                                    match execute_actions(
                                                        action_response.actions,
                                                        &state_clone,
                                                        Some(protocol_clone.as_ref()),
                                                    )
                                                    .await
                                                    {
                                                        Ok(result) => {
                                                            for msg in result.messages {
                                                                let _ = status_clone.send(msg);
                                                            }
                                                            debug!(
                                                                "DNS got {} protocol results",
                                                                result.protocol_results.len()
                                                            );
                                                            let _ = status_clone.send(format!("[DEBUG] DNS got {} protocol results", result.protocol_results.len()));

                                                            for protocol_result in
                                                                result.protocol_results
                                                            {
                                                                if let Some(output_data) =
                                                                    protocol_result
                                                                        .get_all_output()
                                                                        .first()
                                                                {
                                                                    let _ = socket_clone
                                                                        .send_to(
                                                                            output_data,
                                                                            peer_addr,
                                                                        )
                                                                        .await;

                                                                    // DEBUG: Log summary
                                                                    debug!(
                                                                        "DNS sent {} bytes to {}",
                                                                        output_data.len(),
                                                                        peer_addr
                                                                    );
                                                                    let _ = status_clone.send(format!("[DEBUG] DNS sent {} bytes to {}", output_data.len(), peer_addr));

                                                                    // TRACE: Log full payload (hex for binary DNS)
                                                                    let hex_str =
                                                                        hex::encode(output_data);
                                                                    trace!(
                                                                        "DNS sent (hex): {}",
                                                                        hex_str
                                                                    );
                                                                    let _ = status_clone.send(format!("[TRACE] DNS sent (hex): {}", hex_str));

                                                                    let _ = status_clone.send(format!("→ DNS response to {} ({} bytes)", peer_addr, output_data.len()));
                                                                } else {
                                                                    debug!("DNS protocol result has no output data");
                                                                    let _ = status_clone.send("[DEBUG] DNS protocol result has no output data".to_string());
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!(
                                                                "DNS action execution error: {}",
                                                                e
                                                            );
                                                            let _ = status_clone.send(format!(
                                                                "✗ DNS action execution error: {e}"
                                                            ));
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("DNS failed to parse LLM response as actions: {}", e);
                                                    let _ = status_clone.send(format!(
                                                        "✗ DNS failed to parse LLM response: {e}"
                                                    ));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("DNS LLM generation error: {}", e);
                                            let _ =
                                                status_clone.send(format!("✗ DNS LLM error: {e}"));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse DNS query: {}", e);
                                    let _ = status_clone
                                        .send(format!("✗ Failed to parse DNS query: {e}"));

                                    // Fall back to hex representation for malformed queries
                                    let hex_str = hex::encode(&data);
                                    let event_description = format!(
                                        "Malformed DNS query from {} ({} bytes, hex: {})",
                                        peer_addr,
                                        data.len(),
                                        hex_str
                                    );

                                    debug!("DNS malformed query: {}", event_description);
                                    let _ =
                                        status_clone.send(format!("[DEBUG] {event_description}"));
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

        error!("DNS legacy spawn_with_llm is deprecated, use spawn_with_llm_actions");
        let _ = status_tx.send(
            "✗ DNS legacy mode no longer supported, please restart with action-based mode"
                .to_string(),
        );

        Ok(local_addr)
    }
}
