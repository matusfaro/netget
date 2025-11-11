//! SIP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::SipProtocol;
use crate::state::app_state::AppState;
use crate::{console_error, console_trace};
use actions::{
    SIP_ACK_EVENT, SIP_BYE_EVENT, SIP_CANCEL_EVENT, SIP_INVITE_EVENT, SIP_OPTIONS_EVENT,
    SIP_REGISTER_EVENT,
};

/// SIP server that handles VoIP signaling
pub struct SipServer;

impl SipServer {
    /// Spawn SIP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("SIP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] SIP server listening on {}", local_addr));

        let protocol = Arc::new(SipProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Max UDP packet size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance
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
                        debug!("SIP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!(
                            "[DEBUG] SIP received {} bytes from {}",
                            n, peer_addr
                        ));

                        // TRACE: Log first 200 chars of message (SIP is text-based)
                        if let Ok(text) = String::from_utf8(data.clone()) {
                            let preview = if text.len() > 200 {
                                format!("{}...", &text[..200])
                            } else {
                                text.clone()
                            };
                            console_trace!(status_tx, "SIP message: {}", preview);
                        }

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Parse SIP message
                            let sip_message = match Self::parse_sip_message(&data) {
                                Ok(msg) => msg,
                                Err(e) => {
                                    warn!("SIP failed to parse message from {}: {}", peer_addr, e);
                                    let _ = status_clone.send(format!(
                                        "[WARN] SIP failed to parse message from {}: {}",
                                        peer_addr, e
                                    ));
                                    return;
                                }
                            };

                            debug!(
                                "SIP {} request from {} (Call-ID: {})",
                                sip_message.method, peer_addr, sip_message.call_id
                            );
                            let _ = status_clone.send(format!(
                                "[DEBUG] SIP {} request from {} (Call-ID: {})",
                                sip_message.method, peer_addr, sip_message.call_id
                            ));

                            // Create event based on SIP method
                            let event = Self::create_event(
                                &sip_message,
                                peer_addr,
                                local_addr,
                                connection_id,
                            );

                            // Call LLM with SIP event
                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                Some(connection_id),
                                &event,
                                protocol_clone.as_ref(),
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    // Extract action from execution result
                                    if let Some(action) = execution_result.raw_actions.first() {
                                        // Build SIP response from action JSON
                                        let response =
                                            Self::build_sip_response(&sip_message, action);

                                        // Send SIP response
                                        match socket_clone.send_to(&response, peer_addr).await {
                                            Ok(sent) => {
                                                debug!(
                                                    "SIP sent {} byte response to {}",
                                                    sent, peer_addr
                                                );
                                                let _ = status_clone.send(format!(
                                                    "[DEBUG] SIP sent {} byte response to {}",
                                                    sent, peer_addr
                                                ));
                                            }
                                            Err(e) => {
                                                error!("SIP failed to send response: {}", e);
                                                let _ = status_clone.send(format!(
                                                    "[ERROR] SIP failed to send response: {}",
                                                    e
                                                ));
                                            }
                                        }
                                    } else {
                                        debug!(
                                            "SIP no action taken for {} request",
                                            sip_message.method
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("SIP LLM error: {}", e);
                                    let _ =
                                        status_clone.send(format!("[ERROR] SIP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "SIP recv error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Parse SIP message from bytes
    fn parse_sip_message(data: &[u8]) -> Result<SipMessage> {
        let text = String::from_utf8(data.to_vec())?;
        let lines: Vec<&str> = text.lines().collect();

        if lines.is_empty() {
            anyhow::bail!("Empty SIP message");
        }

        // Parse request line (e.g., "REGISTER sip:example.com SIP/2.0")
        let request_line: Vec<&str> = lines[0].split_whitespace().collect();
        if request_line.len() < 3 {
            anyhow::bail!("Invalid SIP request line");
        }

        let method = request_line[0].to_string();
        let request_uri = request_line[1].to_string();

        // Parse headers
        let mut call_id = String::new();
        let mut from = String::new();
        let mut to = String::new();
        let mut via = Vec::new();
        let mut cseq = String::new();
        let mut contact = None;
        let mut expires = None;
        let mut content_type = None;
        let mut content_length = 0;

        let mut i = 1;
        while i < lines.len() {
            let line = lines[i];
            if line.is_empty() {
                // End of headers
                i += 1;
                break;
            }

            if let Some(colon_pos) = line.find(':') {
                let (header_name, header_value) = line.split_at(colon_pos);
                let header_value = header_value[1..].trim();

                match header_name.to_lowercase().as_str() {
                    "call-id" => call_id = header_value.to_string(),
                    "from" | "f" => from = header_value.to_string(),
                    "to" | "t" => to = header_value.to_string(),
                    "via" | "v" => via.push(header_value.to_string()),
                    "cseq" => cseq = header_value.to_string(),
                    "contact" | "m" => contact = Some(header_value.to_string()),
                    "expires" => expires = header_value.parse().ok(),
                    "content-type" | "c" => content_type = Some(header_value.to_string()),
                    "content-length" | "l" => content_length = header_value.parse().unwrap_or(0),
                    _ => {}
                }
            }

            i += 1;
        }

        // Parse body (SDP)
        let mut body = String::new();
        if content_length > 0 && i < lines.len() {
            body = lines[i..].join("\r\n");
        }

        Ok(SipMessage {
            method,
            request_uri,
            call_id,
            from,
            to,
            via,
            cseq,
            contact,
            expires,
            content_type,
            body: if body.is_empty() { None } else { Some(body) },
        })
    }

    /// Create event from SIP message
    fn create_event(
        sip_message: &SipMessage,
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
        connection_id: ConnectionId,
    ) -> Event {
        let base_data = serde_json::json!({
            "peer_addr": peer_addr.to_string(),
            "local_addr": local_addr.to_string(),
            "connection_id": connection_id.to_string(),
            "call_id": sip_message.call_id,
            "from": sip_message.from,
            "to": sip_message.to,
            "cseq": sip_message.cseq,
        });

        match sip_message.method.as_str() {
            "REGISTER" => {
                let mut data = base_data;
                if let Some(contact) = &sip_message.contact {
                    data["contact"] = serde_json::json!(contact);
                }
                if let Some(expires) = sip_message.expires {
                    data["expires"] = serde_json::json!(expires);
                }
                Event {
                    event_type: &SIP_REGISTER_EVENT,
                    data,
                }
            }
            "INVITE" => {
                let mut data = base_data;
                if let Some(body) = &sip_message.body {
                    data["sdp"] = serde_json::json!(body);
                }
                Event {
                    event_type: &SIP_INVITE_EVENT,
                    data,
                }
            }
            "BYE" => Event {
                event_type: &SIP_BYE_EVENT,
                data: base_data,
            },
            "ACK" => Event {
                event_type: &SIP_ACK_EVENT,
                data: base_data,
            },
            "OPTIONS" => Event {
                event_type: &SIP_OPTIONS_EVENT,
                data: base_data,
            },
            "CANCEL" => Event {
                event_type: &SIP_CANCEL_EVENT,
                data: base_data,
            },
            _ => {
                // Unknown method, treat as OPTIONS event
                Event {
                    event_type: &SIP_OPTIONS_EVENT,
                    data: base_data,
                }
            }
        }
    }

    /// Build SIP response from action JSON
    fn build_sip_response(request: &SipMessage, response_action: &serde_json::Value) -> Vec<u8> {
        let response_data = response_action
            .as_object()
            .expect("Action should be an object");
        let status_code = response_data
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as u16;

        let reason_phrase = response_data
            .get("reason_phrase")
            .and_then(|v| v.as_str())
            .unwrap_or("OK");

        // Build status line
        let mut response = format!("SIP/2.0 {} {}\r\n", status_code, reason_phrase);

        // Add Via headers (copy from request)
        for via in &request.via {
            response.push_str(&format!("Via: {}\r\n", via));
        }

        // Add From header (copy from request)
        response.push_str(&format!("From: {}\r\n", request.from));

        // Add To header (copy from request, add tag if not present)
        let to_header = if request.to.contains(";tag=") {
            request.to.clone()
        } else {
            // Generate a tag for the response
            format!("{};tag={}", request.to, Self::generate_tag())
        };
        response.push_str(&format!("To: {}\r\n", to_header));

        // Add Call-ID header (copy from request)
        response.push_str(&format!("Call-ID: {}\r\n", request.call_id));

        // Add CSeq header (copy from request)
        response.push_str(&format!("CSeq: {}\r\n", request.cseq));

        // Add Contact header for successful REGISTER
        if request.method == "REGISTER" && status_code == 200 {
            if let Some(contact) = &request.contact {
                response.push_str(&format!("Contact: {}\r\n", contact));
            }
        }

        // Add Expires header for REGISTER responses
        if request.method == "REGISTER" {
            let expires = response_data
                .get("expires")
                .and_then(|v| v.as_u64())
                .unwrap_or(3600);
            response.push_str(&format!("Expires: {}\r\n", expires));
        }

        // Add Allow header for OPTIONS responses
        if request.method == "OPTIONS" {
            if let Some(allow_methods) = response_data.get("allow_methods") {
                if let Some(methods) = allow_methods.as_array() {
                    let methods_str: Vec<String> = methods
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(String::from)
                        .collect();
                    response.push_str(&format!("Allow: {}\r\n", methods_str.join(", ")));
                }
            }
        }

        // Add body (SDP) for successful INVITE responses
        let body = if request.method == "INVITE" && status_code == 200 {
            response_data
                .get("sdp")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            None
        };

        // Add Content-Length and body
        if let Some(body_text) = &body {
            response.push_str("Content-Type: application/sdp\r\n");
            response.push_str(&format!("Content-Length: {}\r\n", body_text.len()));
            response.push_str("\r\n");
            response.push_str(body_text);
        } else {
            response.push_str("Content-Length: 0\r\n");
            response.push_str("\r\n");
        }

        response.into_bytes()
    }

    /// Generate a random tag for SIP responses
    fn generate_tag() -> String {
        use rand::Rng;
        let tag: u32 = rand::thread_rng().gen();
        format!("{:x}", tag)
    }
}

/// Parsed SIP message
#[derive(Debug, Clone)]
struct SipMessage {
    method: String,
    #[allow(dead_code)]
    request_uri: String,
    call_id: String,
    from: String,
    to: String,
    via: Vec<String>,
    cseq: String,
    contact: Option<String>,
    expires: Option<u32>,
    #[allow(dead_code)]
    content_type: Option<String>,
    body: Option<String>,
}
