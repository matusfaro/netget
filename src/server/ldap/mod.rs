//! LDAP server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

#[cfg(feature = "ldap")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "ldap")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "ldap")]
use crate::llm::ActionResult;
#[cfg(feature = "ldap")]
use actions::{LDAP_BIND_EVENT, LDAP_SEARCH_EVENT, LDAP_UNBIND_EVENT};
#[cfg(feature = "ldap")]
use crate::server::LdapProtocol;
#[cfg(feature = "ldap")]
use crate::protocol::Event;
#[cfg(feature = "ldap")]
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// LDAP server that handles directory operations with LLM
pub struct LdapServer;

#[cfg(feature = "ldap")]
impl LdapServer {
    /// Spawn LDAP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("LDAP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] LDAP server listening on {}", local_addr));

        let protocol = Arc::new(LdapProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await
                        );
                        debug!("LDAP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("→ LDAP connection {} from {}", connection_id, remote_addr));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let mut session = LdapSession {
                                stream,
                                connection_id,
                                server_id,
                                llm_client: llm_clone.clone(),
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone.clone(),
                                authenticated: false,
                                bind_dn: None,
                            };

                            // Handle LDAP session
                            if let Err(e) = session.handle().await {
                                error!("LDAP session error: {}", e);
                                let _ = status_clone.send(format!("[ERROR] LDAP session error: {}", e));
                            }

                            info!("LDAP connection {} closed", connection_id);
                            let _ = status_clone.send(format!("✗ LDAP connection {} closed", connection_id));
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept LDAP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(feature = "ldap")]
struct LdapSession {
    stream: tokio::net::TcpStream,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<LdapProtocol>,
    authenticated: bool,
    bind_dn: Option<String>,
}

#[cfg(feature = "ldap")]
impl LdapSession {
    async fn handle(&mut self) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        loop {
            // Read LDAP message (ASN.1 BER encoded)
            // LDAP messages are SEQUENCE { messageID INTEGER, protocolOp CHOICE, controls OPTIONAL }

            let mut buf = vec![0u8; 8192];
            let n = match self.stream.read(&mut buf).await {
                Ok(0) => break, // Connection closed
                Ok(n) => n,
                Err(e) => {
                    error!("LDAP read error: {}", e);
                    let _ = self.status_tx.send(format!("[ERROR] LDAP read error: {}", e));
                    break;
                }
            };

            buf.truncate(n);
            trace!("LDAP received {} bytes: {:02x?}", n, buf);
            let _ = self.status_tx.send(format!("[TRACE] LDAP received {} bytes", n));

            // Parse LDAP message
            match self.parse_ldap_message(&buf).await {
                Ok(Some(response)) => {
                    trace!("LDAP sending {} bytes: {:02x?}", response.len(), response);
                    let _ = self.status_tx.send(format!("[TRACE] LDAP sending {} bytes", response.len()));

                    self.stream.write_all(&response).await?;
                    self.stream.flush().await?;
                }
                Ok(None) => {
                    // No response needed (e.g., unbind)
                    break;
                }
                Err(e) => {
                    error!("LDAP parse error: {}", e);
                    let _ = self.status_tx.send(format!("[ERROR] LDAP parse error: {}", e));
                    break;
                }
            }
        }

        Ok(())
    }

    async fn parse_ldap_message(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>> {
        // Simple LDAP message parser
        // This is a simplified implementation that handles basic Bind, Search, and Unbind operations

        if data.len() < 7 {
            anyhow::bail!("LDAP message too short");
        }

        // Parse SEQUENCE tag (0x30)
        if data[0] != 0x30 {
            anyhow::bail!("Invalid LDAP message: expected SEQUENCE");
        }

        // Parse message length
        let (_msg_len, len_bytes) = parse_ber_length(&data[1..])?;
        let header_len = 1 + len_bytes;

        // Parse messageID (INTEGER)
        let msg_start = header_len;
        if data[msg_start] != 0x02 {
            anyhow::bail!("Invalid LDAP message: expected messageID INTEGER");
        }

        let (msg_id, id_bytes) = parse_ber_integer(&data[msg_start..])?;
        debug!("LDAP message ID: {}", msg_id);

        // Parse protocolOp (APPLICATION tag)
        let op_start = msg_start + id_bytes;
        let op_tag = data[op_start];

        match op_tag {
            0x60 => self.handle_bind_request(msg_id, &data[op_start..]).await,
            0x63 => self.handle_search_request(msg_id, &data[op_start..]).await,
            0x42 => self.handle_unbind_request().await,
            _ => {
                debug!("LDAP unsupported operation: 0x{:02x}", op_tag);
                let _ = self.status_tx.send(format!("[DEBUG] LDAP unsupported operation: 0x{:02x}", op_tag));
                Ok(Some(encode_ldap_error(msg_id, 2))) // protocolError
            }
        }
    }

    async fn handle_bind_request(&mut self, msg_id: i32, data: &[u8]) -> Result<Option<Vec<u8>>> {
        // Parse BindRequest: version, name (DN), authentication
        let (_, len_bytes) = parse_ber_length(&data[1..])?;
        let bind_start = 1 + len_bytes;

        // Parse version (INTEGER)
        if data[bind_start] != 0x02 {
            return Ok(Some(encode_ldap_error(msg_id, 2)));
        }
        let (version, version_bytes) = parse_ber_integer(&data[bind_start..])?;

        // Parse name/DN (OCTET STRING)
        let name_start = bind_start + version_bytes;
        if data[name_start] != 0x04 {
            return Ok(Some(encode_ldap_error(msg_id, 2)));
        }

        let (dn_bytes, dn_len_bytes) = parse_ber_length(&data[name_start + 1..])?;
        let dn_data_start = name_start + 1 + dn_len_bytes;
        let dn = String::from_utf8_lossy(&data[dn_data_start..dn_data_start + dn_bytes]).to_string();

        // Parse authentication (simple: OCTET STRING with context tag [0])
        let auth_start = dn_data_start + dn_bytes;
        let password = if auth_start < data.len() && (data[auth_start] == 0x80) {
            let (pwd_bytes, pwd_len_bytes) = parse_ber_length(&data[auth_start + 1..])?;
            let pwd_start = auth_start + 1 + pwd_len_bytes;
            String::from_utf8_lossy(&data[pwd_start..pwd_start + pwd_bytes]).to_string()
        } else {
            String::new()
        };

        debug!("LDAP Bind request: version={}, dn={}, password_len={}", version, dn, password.len());
        let _ = self.status_tx.send(format!("[DEBUG] LDAP Bind request: dn={}", dn));

        // Create Bind event for LLM
        let event = Event::new(&LDAP_BIND_EVENT, serde_json::json!({
            "message_id": msg_id,
            "version": version,
            "dn": dn,
            "password": password,
        }));

        // Get LLM response
        if let Ok(execution_result) = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        ).await {
            for protocol_result in execution_result.protocol_results {
                if let ActionResult::Output(data) = protocol_result {
                    // Update session state if bind succeeded
                    if let Ok(success) = check_bind_success(&data) {
                        if success {
                            self.authenticated = true;
                            self.bind_dn = Some(dn.clone());
                            info!("LDAP connection {} authenticated as {}", self.connection_id, dn);
                            let _ = self.status_tx.send(format!("✓ LDAP connection {} authenticated as {}", self.connection_id, dn));
                        }
                    }
                    return Ok(Some(data));
                }
            }
        }

        // Default: return bind failure
        Ok(Some(encode_bind_response(msg_id, 49, "Invalid credentials")))
    }

    async fn handle_search_request(&mut self, msg_id: i32, data: &[u8]) -> Result<Option<Vec<u8>>> {
        // Parse SearchRequest
        let (_, len_bytes) = parse_ber_length(&data[1..])?;
        let search_start = 1 + len_bytes;

        // Parse baseObject (OCTET STRING)
        if data[search_start] != 0x04 {
            return Ok(Some(encode_ldap_error(msg_id, 2)));
        }

        let (base_bytes, base_len_bytes) = parse_ber_length(&data[search_start + 1..])?;
        let base_start = search_start + 1 + base_len_bytes;
        let base_dn = String::from_utf8_lossy(&data[base_start..base_start + base_bytes]).to_string();

        // For simplicity, we'll just extract the base DN and let the LLM handle the rest
        debug!("LDAP Search request: base_dn={}", base_dn);
        let _ = self.status_tx.send(format!("[DEBUG] LDAP Search request: base_dn={}", base_dn));

        // Create Search event for LLM
        let event = Event::new(&LDAP_SEARCH_EVENT, serde_json::json!({
            "message_id": msg_id,
            "base_dn": base_dn,
            "authenticated": self.authenticated,
            "bind_dn": self.bind_dn.as_deref().unwrap_or(""),
        }));

        // Get LLM response
        if let Ok(execution_result) = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        ).await {
            for protocol_result in execution_result.protocol_results {
                if let ActionResult::Output(data) = protocol_result {
                    return Ok(Some(data));
                }
            }
        }

        // Default: return empty search result
        Ok(Some(encode_search_done(msg_id, 0, "")))
    }

    async fn handle_unbind_request(&mut self) -> Result<Option<Vec<u8>>> {
        debug!("LDAP Unbind request from {}", self.connection_id);
        let _ = self.status_tx.send(format!("[DEBUG] LDAP Unbind request from {}", self.connection_id));

        // Create Unbind event for LLM (informational)
        let event = Event::new(&LDAP_UNBIND_EVENT, serde_json::json!({
            "bind_dn": self.bind_dn.as_deref().unwrap_or(""),
        }));

        // Call LLM but don't wait for response (unbind is fire-and-forget)
        let _ = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        ).await;

        // No response for unbind
        Ok(None)
    }
}

#[cfg(feature = "ldap")]
fn parse_ber_length(data: &[u8]) -> Result<(usize, usize)> {
    if data.is_empty() {
        anyhow::bail!("Empty data for BER length");
    }

    let first_byte = data[0];

    if first_byte & 0x80 == 0 {
        // Short form: length is in the first byte
        Ok((first_byte as usize, 1))
    } else {
        // Long form: first byte's lower 7 bits indicate how many bytes encode the length
        let num_len_bytes = (first_byte & 0x7F) as usize;
        if num_len_bytes == 0 || num_len_bytes > 4 {
            anyhow::bail!("Invalid BER length encoding");
        }

        if data.len() < 1 + num_len_bytes {
            anyhow::bail!("Insufficient data for BER length");
        }

        let mut length = 0usize;
        for i in 0..num_len_bytes {
            length = (length << 8) | data[1 + i] as usize;
        }

        Ok((length, 1 + num_len_bytes))
    }
}

#[cfg(feature = "ldap")]
fn parse_ber_integer(data: &[u8]) -> Result<(i32, usize)> {
    if data.len() < 2 || data[0] != 0x02 {
        anyhow::bail!("Invalid BER INTEGER");
    }

    let (length, len_bytes) = parse_ber_length(&data[1..])?;
    let value_start = 1 + len_bytes;

    if data.len() < value_start + length {
        anyhow::bail!("Insufficient data for INTEGER");
    }

    let mut value = 0i32;
    for i in 0..length {
        value = (value << 8) | data[value_start + i] as i32;
    }

    Ok((value, value_start + length))
}

#[cfg(feature = "ldap")]
fn encode_ber_length(length: usize) -> Vec<u8> {
    if length < 128 {
        vec![length as u8]
    } else if length < 256 {
        vec![0x81, length as u8]
    } else if length < 65536 {
        vec![0x82, (length >> 8) as u8, length as u8]
    } else {
        vec![0x83, (length >> 16) as u8, (length >> 8) as u8, length as u8]
    }
}

#[cfg(feature = "ldap")]
fn encode_ber_integer(value: i32) -> Vec<u8> {
    let mut result = vec![0x02]; // INTEGER tag

    if value >= 0 && value < 128 {
        result.push(0x01); // length
        result.push(value as u8);
    } else {
        result.push(0x04); // length (4 bytes)
        result.extend_from_slice(&value.to_be_bytes());
    }

    result
}

#[cfg(feature = "ldap")]
fn encode_bind_response(msg_id: i32, result_code: u8, diagnostic_message: &str) -> Vec<u8> {
    // BindResponse ::= [APPLICATION 1] SEQUENCE {
    //     resultCode ENUMERATED,
    //     matchedDN LDAPDN,
    //     diagnosticMessage LDAPString,
    //     ... }

    let mut bind_resp = Vec::new();

    // resultCode (ENUMERATED - same encoding as INTEGER)
    bind_resp.push(0x0A); // ENUMERATED tag
    bind_resp.push(0x01); // length
    bind_resp.push(result_code);

    // matchedDN (OCTET STRING)
    bind_resp.push(0x04); // OCTET STRING tag
    bind_resp.push(0x00); // length (empty)

    // diagnosticMessage (OCTET STRING)
    bind_resp.push(0x04); // OCTET STRING tag
    let diag_bytes = diagnostic_message.as_bytes();
    bind_resp.extend_from_slice(&encode_ber_length(diag_bytes.len()));
    bind_resp.extend_from_slice(diag_bytes);

    // Wrap in BindResponse APPLICATION tag [1]
    let mut bind_msg = vec![0x61]; // APPLICATION 1
    bind_msg.extend_from_slice(&encode_ber_length(bind_resp.len()));
    bind_msg.extend_from_slice(&bind_resp);

    // Create LDAPMessage SEQUENCE
    encode_ldap_message(msg_id, bind_msg)
}

#[cfg(feature = "ldap")]
fn encode_search_done(msg_id: i32, result_code: u8, diagnostic_message: &str) -> Vec<u8> {
    // SearchResultDone ::= [APPLICATION 5] LDAPResult

    let mut result = Vec::new();

    // resultCode (ENUMERATED)
    result.push(0x0A);
    result.push(0x01);
    result.push(result_code);

    // matchedDN (empty)
    result.push(0x04);
    result.push(0x00);

    // diagnosticMessage
    result.push(0x04);
    let diag_bytes = diagnostic_message.as_bytes();
    result.extend_from_slice(&encode_ber_length(diag_bytes.len()));
    result.extend_from_slice(diag_bytes);

    // Wrap in SearchResultDone APPLICATION tag [5]
    let mut search_msg = vec![0x65]; // APPLICATION 5
    search_msg.extend_from_slice(&encode_ber_length(result.len()));
    search_msg.extend_from_slice(&result);

    encode_ldap_message(msg_id, search_msg)
}

#[cfg(feature = "ldap")]
fn encode_ldap_error(msg_id: i32, result_code: u8) -> Vec<u8> {
    encode_bind_response(msg_id, result_code, "")
}

#[cfg(feature = "ldap")]
fn encode_ldap_message(msg_id: i32, protocol_op: Vec<u8>) -> Vec<u8> {
    // LDAPMessage ::= SEQUENCE {
    //     messageID INTEGER,
    //     protocolOp CHOICE { ... }
    // }

    let mut content = Vec::new();
    content.extend_from_slice(&encode_ber_integer(msg_id));
    content.extend_from_slice(&protocol_op);

    let mut message = vec![0x30]; // SEQUENCE tag
    message.extend_from_slice(&encode_ber_length(content.len()));
    message.extend_from_slice(&content);

    message
}

#[cfg(feature = "ldap")]
fn check_bind_success(data: &[u8]) -> Result<bool> {
    // Check if bind response indicates success (resultCode = 0)
    // This is a simple check - looks for the pattern of a successful bind

    if data.len() < 12 {
        return Ok(false);
    }

    // Look for BindResponse tag (0x61) and check resultCode
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == 0x61 && i + 4 < data.len() {
            // Found BindResponse, check if resultCode (ENUMERATED) is 0
            let result_start = i + 2; // Skip tag and length
            if data[result_start] == 0x0A && data[result_start + 1] == 0x01 {
                return Ok(data[result_start + 2] == 0);
            }
        }
    }

    Ok(false)
}

#[cfg(not(feature = "ldap"))]
impl LdapServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: crate::llm::ollama_client::OllamaClient,
        _app_state: Arc<crate::state::app_state::AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("LDAP feature not enabled")
    }
}
