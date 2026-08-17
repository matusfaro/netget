//! SIP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::SipProtocol;
use crate::state::app_state::AppState;
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
        Log::new(Some(&status_tx)).info(format!("SIP server listening on {}", local_addr));

        let protocol = Arc::new(SipProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Max UDP packet size
            let log = Log::new(Some(&status_tx));

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

                        // Summary + full payload FileOnly: the sip_* event templates
                        // render the equivalent line to the TUI.
                        log.debug(format!("SIP received {} bytes from {}", n, peer_addr));
                        if let Ok(text) = String::from_utf8(data.clone()) {
                            let preview = crate::utils::truncate_for_log(&text, 200);
                            log.trace(format!("SIP message: {}", preview));
                        }

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let log = Log::new(Some(&status_clone));
                            // Parse SIP message
                            let sip_message = match Self::parse_sip_message(&data) {
                                Ok(msg) => msg,
                                Err(e) => {
                                    log.warn(format!(
                                        "SIP failed to parse message from {}: {}",
                                        peer_addr, e
                                    ));
                                    return;
                                }
                            };

                            // Request summary FileOnly: the sip_* event template renders
                            // the equivalent line to the TUI.
                            log.debug(format!(
                                "SIP {} request from {} (Call-ID: {})",
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
                                                // Summary FileOnly: the send action template
                                                // already reports the send to the TUI.
                                                log.debug(format!(
                                                    "SIP sent {} byte response to {}",
                                                    sent, peer_addr
                                                ));
                                            }
                                            Err(e) => {
                                                log.error(format!(
                                                    "SIP failed to send response: {}",
                                                    e
                                                ));
                                            }
                                        }

                                        // Media flow: SIP is signaling only, but when built with
                                        // the `rtp` feature and the accept action carries an
                                        // `rtp_audio` description, we honour the SDP we just
                                        // negotiated by streaming real RTP to the caller's
                                        // advertised media address. This is what makes a SIP INVITE
                                        // result in RTP actually arriving, rather than a 200 OK with
                                        // an SDP that points at nothing.
                                        #[cfg(feature = "rtp")]
                                        {
                                            let status_code = action
                                                .get("status_code")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(200);
                                            if sip_message.method == "INVITE" && status_code == 200
                                            {
                                                if let Some(rtp_audio) = action.get("rtp_audio") {
                                                    Self::stream_invite_media(
                                                        sip_message.body.as_deref(),
                                                        rtp_audio.clone(),
                                                        status_clone.clone(),
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        log.debug(format!(
                                            "SIP no action taken for {} request",
                                            sip_message.method
                                        ));
                                    }
                                }
                                Err(e) => {
                                    // Non-fatal: a 503 (wire fallback) is built and sent
                                    // below for everything except ACK, so this is WARN.
                                    log.warn(format!(
                                        "SIP LLM error for {} from {} (Call-ID {}): {}",
                                        sip_message.method, peer_addr, sip_message.call_id, e
                                    ));

                                    // ACK is the one method that must never be answered
                                    // (RFC 3261 §17: ACK is not a transaction that takes a
                                    // response), so silence is correct there and only there.
                                    if sip_message.method == "ACK" {
                                        log.debug("SIP ACK needs no response; nothing sent");
                                        return;
                                    }

                                    // Everything else gets a 503 Service Unavailable built
                                    // from the request, so the Via/From/To/Call-ID/CSeq match
                                    // and the client's transaction actually completes. Left
                                    // silent, the UAC retransmits on timers A/E and only gives
                                    // up after Timer B/F (32s).
                                    if crate::llm::is_overload_error(&e) {
                                        log.warn(format!(
                                            "SIP 503 to {} (Call-ID {}): LLM capacity exhausted",
                                            peer_addr, sip_message.call_id
                                        ));
                                    }
                                    let response = Self::build_sip_response(
                                        &sip_message,
                                        &serde_json::json!({
                                            "status_code": 503,
                                            "reason_phrase": "Service Unavailable",
                                            "retry_after": 5
                                        }),
                                    );
                                    match socket_clone.send_to(&response, peer_addr).await {
                                        Ok(sent) => {
                                            log.debug(format!(
                                                "SIP 503 Service Unavailable to {} ({} bytes)",
                                                peer_addr, sent
                                            ));
                                        }
                                        Err(send_err) => {
                                            log.error(format!(
                                                "SIP failed to send 503 to {}: {}",
                                                peer_addr, send_err
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        log.error(format!("SIP recv error: {}", e));
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

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
        // A missing `status_code` is a malformed action, not a decision, so it must not
        // become 200. On REGISTER a defaulted 200 is an accepted registration and on INVITE
        // an accepted call - granted because a field was forgotten. SIP has no default
        // status, so the honest answer is the server's own 500; the model's actual choices
        // (401, 403, 486) are unaffected because they name the field.
        let (status_code, default_reason) = match response_data
            .get("status_code")
            .and_then(|v| v.as_u64())
        {
            Some(code) => (code as u16, "OK"),
            None => {
                tracing::warn!(
                    "SIP action carried no status_code; answering 500 rather than defaulting \
                     to 200, which would accept the request"
                );
                (500u16, "Server Internal Error")
            }
        };

        let reason_phrase = response_data
            .get("reason_phrase")
            .and_then(|v| v.as_str())
            .unwrap_or(default_reason);

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

        // Add Retry-After when the action asks for one. RFC 3261 §20.33: a 503 SHOULD
        // carry it, so the caller retries after a bounded wait instead of failing over
        // to another proxy or giving up on the URI entirely.
        if let Some(retry_after) = response_data.get("retry_after").and_then(|v| v.as_u64()) {
            response.push_str(&format!("Retry-After: {}\r\n", retry_after));
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

    /// Stream RTP audio to the media address the caller advertised in its INVITE SDP.
    ///
    /// Only compiled with the `rtp` feature. The `rtp_audio` value is the same structured media
    /// description the RTP protocol understands (`content`, `tone_hz`, `payload_type`,
    /// `duration_ms`) — the model describes what the call carries, and the shared media engine
    /// (`crate::server::rtp::media`) owns the samples and RTP framing. A fresh ephemeral UDP
    /// socket is used because RTP must not share the SIP signaling port.
    #[cfg(feature = "rtp")]
    fn stream_invite_media(
        caller_sdp: Option<&str>,
        rtp_audio: serde_json::Value,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        use crate::server::rtp::media::{self, AudioCodec, RtpPacketizer};

        let Some((ip, port)) = caller_sdp.and_then(parse_sdp_audio_target) else {
            Log::new(Some(&status_tx)).warn(
                "SIP INVITE had rtp_audio but no parseable m=audio target in the caller's SDP",
            );
            return;
        };
        let target = std::net::SocketAddr::new(ip, port);

        let codec = rtp_audio
            .get("payload_type")
            .and_then(|v| v.as_str())
            .map(AudioCodec::parse)
            .unwrap_or(Ok(AudioCodec::Pcmu))
            .unwrap_or(AudioCodec::Pcmu);
        let content = media::parse_audio_content(&rtp_audio)
            .unwrap_or(media::AudioContent::Tone { hz: 440.0 });
        let duration_ms = rtp_audio
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000);

        tokio::spawn(async move {
            let log = Log::new(Some(&status_tx));
            let socket =
                match tokio::net::UdpSocket::bind(std::net::SocketAddr::from(([0u8, 0, 0, 0], 0)))
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        log.error(format!("SIP RTP socket bind failed: {}", e));
                        return;
                    }
                };
            let payload = match media::synthesize(codec, &content, duration_ms) {
                Ok(p) => p,
                Err(e) => {
                    log.warn(format!("SIP RTP synthesis: {}", e));
                    return;
                }
            };
            let mut packetizer =
                RtpPacketizer::new(rand::random(), codec.payload_type(), None, None);
            let packets = packetizer.packetize(&payload, media::G711_SAMPLES_PER_FRAME);
            let mut sent = 0u64;
            for pkt in &packets {
                if socket.send_to(pkt, target).await.is_err() {
                    break;
                }
                sent += 1;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            log.info(format!(
                "SIP call media: {} RTP {} packet(s) to {}",
                sent,
                codec.rtpmap_name(),
                target
            ));
        });
    }
}

/// Extract the media target (connection IP + audio port) from an SDP offer.
///
/// Reads the `c=IN IP4 <ip>` connection line and the first `m=audio <port> ...` line. Returns
/// None if either is missing or unparseable.
#[cfg(feature = "rtp")]
fn parse_sdp_audio_target(sdp: &str) -> Option<(std::net::IpAddr, u16)> {
    let mut ip: Option<std::net::IpAddr> = None;
    let mut port: Option<u16> = None;
    for line in sdp.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("c=IN IP4 ") {
            ip = rest
                .trim()
                .split('/')
                .next()
                .and_then(|s| s.trim().parse().ok());
        } else if let Some(rest) = line.strip_prefix("m=audio ") {
            port = rest.split_whitespace().next().and_then(|s| s.parse().ok());
        }
    }
    Some((ip?, port?))
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
