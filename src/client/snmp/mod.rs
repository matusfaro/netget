//! SNMP client implementation
pub mod actions;

pub use actions::SnmpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::snmp::actions::{SNMP_CLIENT_CONNECTED_EVENT, SNMP_CLIENT_RESPONSE_RECEIVED_EVENT};

// SNMP protocol support
use rasn_snmp::{v1, v2, v2c};
use rasn_smi::v1::{SimpleSyntax as V1SimpleSyntax, ObjectSyntax as V1ObjectSyntax};
use rasn_smi::v2::{SimpleSyntax as V2SimpleSyntax, ObjectSyntax as V2ObjectSyntax};
use rasn::types::{Integer, ObjectIdentifier};
use rasn::ber;
use serde_json::Value;

/// SNMP client configuration
#[derive(Debug, Clone)]
struct SnmpConfig {
    community: String,
    version: SnmpVersion,
    timeout_ms: u64,
    retries: u32,
}

impl Default for SnmpConfig {
    fn default() -> Self {
        Self {
            community: "public".to_string(),
            version: SnmpVersion::V2c,
            timeout_ms: 5000,
            retries: 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SnmpVersion {
    V1,
    V2c,
}

/// Parse startup parameters
fn parse_startup_params(params: Option<crate::protocol::StartupParams>) -> SnmpConfig {
    let mut config = SnmpConfig::default();

    if let Some(params) = params {
        if let Some(community) = params.get_optional_string("community") {
            config.community = community;
        }
        if let Some(version) = params.get_optional_string("version") {
            config.version = match version.to_lowercase().as_str() {
                "v1" | "1" => SnmpVersion::V1,
                "v2c" | "v2" | "2c" | "2" => SnmpVersion::V2c,
                _ => SnmpVersion::V2c,
            };
        }
        if let Some(timeout) = params.get_optional_i64("timeout_ms") {
            config.timeout_ms = timeout as u64;
        }
        if let Some(retries) = params.get_optional_i64("retries") {
            config.retries = retries as u32;
        }
    }

    config
}

/// Helper function to parse OID string to ObjectIdentifier
fn parse_oid(oid_str: &str) -> ObjectIdentifier {
    let components: Vec<u32> = oid_str
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();

    if components.is_empty() {
        // Return a default OID if parsing fails
        ObjectIdentifier::new_unchecked(vec![1, 3, 6, 1, 2, 1, 1, 1, 0].into())
    } else {
        ObjectIdentifier::new_unchecked(components.into())
    }
}

/// SNMP client that connects to an SNMP agent
pub struct SnmpClient;

impl SnmpClient {
    /// Connect to an SNMP agent with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse configuration
        let config = parse_startup_params(startup_params);
        debug!("SNMP client config: community={}, version={:?}, timeout={}ms, retries={}",
            config.community, config.version, config.timeout_ms, config.retries);

        // Bind UDP socket to any local port
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .context("Failed to bind UDP socket")?;

        let local_addr = socket.local_addr()?;

        // Connect to remote agent (sets default destination for send/recv)
        socket.connect(&remote_addr).await
            .context(format!("Failed to connect to SNMP agent at {}", remote_addr))?;

        let remote_sock_addr: SocketAddr = remote_addr.parse()
            .context("Invalid remote address")?;

        info!("SNMP client {} connected to {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] SNMP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(SnmpClientProtocol::new());
            let event = Event::new(
                &SNMP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            let llm_clone = llm_client.clone();
            let state_clone = app_state.clone();
            let status_clone = status_tx.clone();
            let protocol_clone = protocol.clone();
            let socket = Arc::new(socket);
            let socket_clone = socket.clone();
            let config = Arc::new(config);

            // Spawn initial LLM call
            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_clone,
                    &state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol_clone.as_ref(),
                    &status_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute initial actions
                        Self::execute_actions(
                            actions,
                            &protocol_clone,
                            &socket_clone,
                            client_id,
                            &config,
                            &llm_clone,
                            &state_clone,
                            &status_clone,
                        ).await;
                    }
                    Err(e) => {
                        error!("Initial LLM call failed for SNMP client {}: {}", client_id, e);
                    }
                }
            });
        }

        Ok(local_addr)
    }

    /// Execute SNMP actions from LLM
    async fn execute_actions(
        actions: Vec<Value>,
        protocol: &Arc<SnmpClientProtocol>,
        socket: &Arc<UdpSocket>,
        client_id: ClientId,
        config: &Arc<SnmpConfig>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::client_trait::ClientActionResult;

        for action in actions {
            match protocol.execute_action(action) {
                Ok(ClientActionResult::Custom { name, data }) => {
                    match name.as_str() {
                        "snmp_get" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();

                                if let Err(e) = Self::send_get_request(
                                    socket, &oid_strings, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GET: {}", e);
                                }
                            }
                        }
                        "snmp_getnext" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();

                                if let Err(e) = Self::send_getnext_request(
                                    socket, &oid_strings, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GETNEXT: {}", e);
                                }
                            }
                        }
                        "snmp_getbulk" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
                                let non_repeaters = data.get("non_repeaters")
                                    .and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                let max_repetitions = data.get("max_repetitions")
                                    .and_then(|v| v.as_i64()).unwrap_or(10) as i32;

                                if let Err(e) = Self::send_getbulk_request(
                                    socket, &oid_strings, non_repeaters, max_repetitions,
                                    config, client_id, llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GETBULK: {}", e);
                                }
                            }
                        }
                        "snmp_set" => {
                            if let Some(variables) = data.get("variables").and_then(|v| v.as_array()) {
                                if let Err(e) = Self::send_set_request(
                                    socket, variables, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP SET: {}", e);
                                }
                            }
                        }
                        _ => {
                            debug!("Unknown SNMP action: {}", name);
                        }
                    }
                }
                Ok(ClientActionResult::Disconnect) => {
                    info!("SNMP client {} disconnecting", client_id);
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                }
                Ok(ClientActionResult::WaitForMore) => {
                    // No action needed
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to execute SNMP action: {}", e);
                }
            }
        }
    }

    /// Send SNMP GET request
    async fn send_get_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        // Build GET request based on version
        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_get_request(oids, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_get_request(oids, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending GET for {} OIDs", client_id, oids.len());
        trace!("SNMP GET request (hex): {}", hex::encode(&request_bytes));

        // Send request and wait for response
        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP GETNEXT request
    async fn send_getnext_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_getnext_request(oids, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_getnext_request(oids, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending GETNEXT for {} OIDs", client_id, oids.len());

        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetNextRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP GETBULK request (v2c only)
    async fn send_getbulk_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        non_repeaters: i32,
        max_repetitions: i32,
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        if matches!(config.version, SnmpVersion::V1) {
            return Err(anyhow::anyhow!("GETBULK is only supported in SNMPv2c"));
        }

        let request_id = rand::random::<i32>();
        let request_bytes = Self::build_v2c_getbulk_request(
            oids, &config.community, request_id, non_repeaters, max_repetitions
        )?;

        debug!("SNMP client {} sending GETBULK (non_repeaters={}, max_repetitions={})",
            client_id, non_repeaters, max_repetitions);

        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetBulkRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP SET request
    async fn send_set_request(
        socket: &Arc<UdpSocket>,
        variables: &[Value],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_set_request(variables, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_set_request(variables, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending SET for {} variables", client_id, variables.len());

        Self::send_request_and_handle_response(
            socket, &request_bytes, "SetRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send request and handle response
    async fn send_request_and_handle_response(
        socket: &Arc<UdpSocket>,
        request_bytes: &[u8],
        request_type: &str,
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let timeout_duration = Duration::from_millis(config.timeout_ms);
        let mut retries = config.retries;

        loop {
            // Send request
            socket.send(request_bytes).await?;

            // Wait for response with timeout
            let mut buffer = vec![0u8; 65535];
            match timeout(timeout_duration, socket.recv(&mut buffer)).await {
                Ok(Ok(n)) => {
                    let response_data = &buffer[..n];
                    trace!("SNMP response (hex): {}", hex::encode(response_data));

                    // Parse response and call LLM
                    Self::handle_response(
                        response_data, request_type, client_id, llm_client,
                        app_state, status_tx, protocol, socket, config
                    ).await?;

                    return Ok(());
                }
                Ok(Err(e)) => {
                    return Err(e.into());
                }
                Err(_) => {
                    // Timeout
                    if retries > 0 {
                        retries -= 1;
                        debug!("SNMP client {} request timeout, retrying ({} left)", client_id, retries);
                        continue;
                    } else {
                        return Err(anyhow::anyhow!("SNMP request timeout after {} retries", config.retries));
                    }
                }
            }
        }
    }

    /// Handle SNMP response
    async fn handle_response(
        response_data: &[u8],
        request_type: &str,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
        socket: &Arc<UdpSocket>,
        config: &Arc<SnmpConfig>,
    ) -> Result<()> {
        // Try parsing as v2c first
        let (variables, error_status) = if let Ok(msg) = ber::decode::<v2c::Message<v2::Pdus>>(response_data) {
            Self::extract_v2c_response(&msg)?
        } else if let Ok(msg) = ber::decode::<v1::Message<v1::Pdus>>(response_data) {
            Self::extract_v1_response(&msg)?
        } else {
            return Err(anyhow::anyhow!("Failed to parse SNMP response"));
        };

        debug!("SNMP client {} received response with {} variables, error_status={}",
            client_id, variables.len(), error_status);

        // Call LLM with response
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &SNMP_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "request_type": request_type,
                    "variables": variables,
                    "error_status": error_status,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute follow-up actions (boxed to avoid infinite recursion)
                    Box::pin(Self::execute_actions(
                        actions, protocol, socket, client_id, config,
                        llm_client, app_state, status_tx
                    )).await;
                }
                Err(e) => {
                    error!("LLM error for SNMP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Extract variables from v2c response
    fn extract_v2c_response(msg: &v2c::Message<v2::Pdus>) -> Result<(Vec<Value>, i32)> {
        let (var_binds, error_status) = match &msg.data {
            v2::Pdus::Response(resp) => (&resp.0.variable_bindings, resp.0.error_status as i32),
            _ => return Err(anyhow::anyhow!("Unexpected PDU type in response")),
        };

        let variables: Vec<Value> = var_binds.iter().map(|vb| {
            serde_json::json!({
                "oid": vb.name.to_string(),
                "value": Self::format_v2_value(&vb.value),
            })
        }).collect();

        Ok((variables, error_status))
    }

    /// Extract variables from v1 response
    fn extract_v1_response(msg: &v1::Message<v1::Pdus>) -> Result<(Vec<Value>, i32)> {
        let (var_binds, error_status) = match &msg.data {
            v1::Pdus::GetResponse(resp) => {
                let err_status = match &resp.0.error_status {
                    Integer::Primitive(v) => *v as i32,
                    Integer::Variable(big) => big.to_string().parse().unwrap_or(0),
                };
                (&resp.0.variable_bindings, err_status)
            }
            _ => return Err(anyhow::anyhow!("Unexpected PDU type in response")),
        };

        let variables: Vec<Value> = var_binds.iter().map(|vb| {
            serde_json::json!({
                "oid": vb.name.to_string(),
                "value": Self::format_v1_value(&vb.value),
            })
        }).collect();

        Ok((variables, error_status))
    }

    /// Format v2 value for JSON
    fn format_v2_value(value: &v2::VarBindValue) -> Value {
        use v2::VarBindValue;
        match value {
            VarBindValue::Unspecified => serde_json::json!(null),
            VarBindValue::NoSuchObject => serde_json::json!("(no such object)"),
            VarBindValue::NoSuchInstance => serde_json::json!("(no such instance)"),
            VarBindValue::EndOfMibView => serde_json::json!("(end of MIB view)"),
            VarBindValue::Value(obj_syntax) => Self::format_object_syntax_v2(obj_syntax),
        }
    }

    fn format_object_syntax_v2(syntax: &V2ObjectSyntax) -> Value {
        match syntax {
            V2ObjectSyntax::Simple(V2SimpleSyntax::Integer(n)) => {
                match n {
                    Integer::Primitive(val) => serde_json::json!(val),
                    Integer::Variable(val) => serde_json::json!(val.to_string()),
                }
            },
            V2ObjectSyntax::Simple(V2SimpleSyntax::String(s)) => serde_json::json!(String::from_utf8_lossy(s)),
            V2ObjectSyntax::Simple(V2SimpleSyntax::ObjectId(_)) => serde_json::json!("(object-id)"),
            V2ObjectSyntax::ApplicationWide(_) => serde_json::json!("(application-wide)"),
        }
    }

    /// Format v1 value for JSON
    fn format_v1_value(value: &V1ObjectSyntax) -> Value {
        match value {
            V1ObjectSyntax::Simple(V1SimpleSyntax::Number(n)) => {
                match n {
                    Integer::Primitive(val) => serde_json::json!(val),
                    Integer::Variable(val) => serde_json::json!(val.to_string()),
                }
            },
            V1ObjectSyntax::Simple(V1SimpleSyntax::String(s)) => serde_json::json!(String::from_utf8_lossy(s)),
            V1ObjectSyntax::Simple(V1SimpleSyntax::Object(_)) => serde_json::json!("(object-id)"),
            V1ObjectSyntax::Simple(V1SimpleSyntax::Empty) => serde_json::json!(null),
            V1ObjectSyntax::ApplicationWide(_) => serde_json::json!("(application-wide)"),
        }
    }

    // Request builders (simplified - using rasn-snmp encoding)

    fn build_v2c_get_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: parse_oid(oid),
                value: v2::VarBindValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetRequest(v2::GetRequest(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: Integer::Primitive(1),  // v2c uses version 1
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v2c GET request: {}", e))
    }

    fn build_v2c_getnext_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: parse_oid(oid),
                value: v2::VarBindValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetNextRequest(v2::GetNextRequest(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: Integer::Primitive(1),
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v2c GETNEXT request: {}", e))
    }

    fn build_v2c_getbulk_request(
        oids: &[String],
        community: &str,
        request_id: i32,
        non_repeaters: i32,
        max_repetitions: i32,
    ) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: parse_oid(oid),
                value: v2::VarBindValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetBulkRequest(v2::GetBulkRequest(v2::BulkPdu {
            request_id,
            non_repeaters: non_repeaters as u32,
            max_repetitions: max_repetitions as u32,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: Integer::Primitive(1),
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v2c GETBULK request: {}", e))
    }

    fn build_v2c_set_request(variables: &[Value], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = variables.iter().map(|var| {
            let oid = var.get("oid").and_then(|v| v.as_str()).unwrap_or("1.3.6.1.2.1.1.1.0");
            let value_type = var.get("type").and_then(|v| v.as_str()).unwrap_or("string");
            let value = var.get("value").unwrap_or(&serde_json::json!(null));

            let data = match value_type {
                "integer" => {
                    let n = value.as_i64().unwrap_or(0);
                    v2::VarBindValue::Value(V2ObjectSyntax::Simple(V2SimpleSyntax::Integer(Integer::Primitive(n as isize))))
                }
                "string" => {
                    let s = value.as_str().unwrap_or("");
                    v2::VarBindValue::Value(V2ObjectSyntax::Simple(V2SimpleSyntax::String(s.as_bytes().to_vec().into())))
                }
                _ => v2::VarBindValue::Unspecified,
            };

            v2::VarBind {
                name: parse_oid(oid),
                value: data,
            }
        }).collect();

        let pdu = v2::Pdus::SetRequest(v2::SetRequest(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: Integer::Primitive(1),
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v2c SET request: {}", e))
    }

    fn build_v1_get_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = oids.iter().map(|oid| {
            v1::VarBind {
                name: parse_oid(oid),
                value: V1ObjectSyntax::Simple(V1SimpleSyntax::Empty),
            }
        }).collect();

        let pdu = v1::Pdus::GetRequest(v1::GetRequest(v1::Pdu {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        }));

        let message = v1::Message {
            version: Integer::Primitive(0),  // v1 uses version 0
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v1 GET request: {}", e))
    }

    fn build_v1_getnext_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = oids.iter().map(|oid| {
            v1::VarBind {
                name: parse_oid(oid),
                value: V1ObjectSyntax::Simple(V1SimpleSyntax::Empty),
            }
        }).collect();

        let pdu = v1::Pdus::GetNextRequest(v1::GetNextRequest(v1::Pdu {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        }));

        let message = v1::Message {
            version: Integer::Primitive(0),
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v1 GETNEXT request: {}", e))
    }

    fn build_v1_set_request(variables: &[Value], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = variables.iter().map(|var| {
            let oid = var.get("oid").and_then(|v| v.as_str()).unwrap_or("1.3.6.1.2.1.1.1.0");
            let value_type = var.get("type").and_then(|v| v.as_str()).unwrap_or("string");
            let value = var.get("value").unwrap_or(&serde_json::json!(null));

            let value_obj = match value_type {
                "integer" => {
                    let n = value.as_i64().unwrap_or(0);
                    V1ObjectSyntax::Simple(V1SimpleSyntax::Number(Integer::Primitive(n as isize)))
                }
                "string" => {
                    let s = value.as_str().unwrap_or("");
                    V1ObjectSyntax::Simple(V1SimpleSyntax::String(s.as_bytes().to_vec().into()))
                }
                _ => V1ObjectSyntax::Simple(V1SimpleSyntax::Empty),
            };

            v1::VarBind {
                name: parse_oid(oid),
                value: value_obj,
            }
        }).collect();

        let pdu = v1::Pdus::SetRequest(v1::SetRequest(v1::Pdu {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        }));

        let message = v1::Message {
            version: Integer::Primitive(0),
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).map_err(|e| anyhow::anyhow!("Failed to encode SNMP v1 SET request: {}", e))
    }
}
