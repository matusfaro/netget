//! DoT (DNS over TLS) client implementation
pub mod actions;

pub use actions::DotClientProtocol;

use anyhow::{Context, Result};
use hickory_proto::op::{Message as DnsMessage, MessageType, OpCode, Query};
use hickory_proto::rr::{Name, RecordType};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use tracing::{debug, error, info, trace};

use crate::client::dot::actions::{DOT_CLIENT_CONNECTED_EVENT, DOT_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client as ClientTrait, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_responses: Vec<Vec<u8>>,
    memory: String,
}

/// DoT client that makes DNS queries over TLS
pub struct DotClient;

impl DotClient {
    /// Connect to a DoT server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("DoT client {} connecting to {}", client_id, remote_addr);
        let _ = status_tx.send(format!("[CLIENT] DoT client {} connecting to {}", client_id, remote_addr));

        // Parse remote address
        let remote_socket_addr: SocketAddr = remote_addr.parse()
            .context("Invalid remote address format")?;

        // Extract hostname for SNI (or use IP as fallback)
        let server_name = remote_addr.split(':').next().unwrap_or("dns.server");

        // Create TLS config with root certificates
        let root_store = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));

        // Connect TCP stream
        let tcp_stream = TcpStream::connect(&remote_socket_addr)
            .await
            .context("Failed to connect to DoT server")?;

        let local_addr = tcp_stream.local_addr()?;

        // Perform TLS handshake
        let server_name = match ServerName::try_from(server_name.to_string()) {
            Ok(name) => name,
            Err(_) => {
                debug!("Failed to parse server name, using IP");
                ServerName::try_from(remote_socket_addr.ip().to_string())
                    .map_err(|e| anyhow::anyhow!("Invalid server name: {}", e))?
            }
        };

        let tls_stream = connector.connect(server_name, tcp_stream)
            .await
            .context("TLS handshake failed")?;

        info!("DoT client {} connected to {}", client_id, remote_addr);
        let _ = status_tx.send(format!("[CLIENT] DoT client {} connected", client_id));

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream for bidirectional communication
        let (read_half, write_half) = tokio::io::split(tls_stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_responses: Vec::new(),
            memory: String::new(),
        }));

        // Spawn read loop for handling responses
        let write_for_read = write_half_arc.clone();
        let client_data_for_read = client_data.clone();
        let read_app_state = app_state.clone();
        let read_llm_client = llm_client.clone();
        let read_status_tx = status_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                read_half,
                write_for_read,
                client_id,
                read_app_state,
                read_llm_client,
                read_status_tx,
                client_data_for_read,
            ).await {
                error!("DoT client {} read loop error: {}", client_id, e);
            }
        });

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(DotClientProtocol::new());
            let event = Event::new(
                &DOT_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute initial actions
                    for action_json in actions {
                        if let Err(e) = Self::execute_client_action(
                            client_id,
                            action_json,
                            &write_half_arc,
                            &status_tx,
                            &app_state,
                        ).await {
                            error!("DoT client {} action execution failed: {}", client_id, e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for DoT client {}: {}", client_id, e);
                }
            }
        }

        Ok(local_addr)
    }

    /// Read loop for handling DNS responses
    async fn read_loop(
        mut read_half: ReadHalf<TlsStream<TcpStream>>,
        write_half: Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
        client_id: ClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
        client_data: Arc<Mutex<ClientData>>,
    ) -> Result<()> {

        loop {
            // Check client status
            if app_state.get_client(client_id).await.is_none() {
                info!("DoT client {} stopped", client_id);
                break;
            }

            // Read length-prefixed DNS message (2-byte big-endian length)
            let mut len_buf = [0u8; 2];
            match read_half.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("DoT client {} connection closed by server", client_id);
                    let _ = status_tx.send(format!("[CLIENT] DoT client {} disconnected", client_id));
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    break;
                }
                Err(e) => {
                    error!("DoT client {} failed to read length prefix: {}", client_id, e);
                    break;
                }
            }

            let dns_len = u16::from_be_bytes(len_buf) as usize;

            if dns_len == 0 || dns_len > 65535 {
                error!("DoT client {} invalid DNS message length: {}", client_id, dns_len);
                break;
            }

            // Read DNS message
            let mut dns_buf = vec![0u8; dns_len];
            if let Err(e) = read_half.read_exact(&mut dns_buf).await {
                error!("DoT client {} failed to read DNS message: {}", client_id, e);
                break;
            }

            debug!("DoT client {} received {} bytes", client_id, dns_len);
            trace!("DoT response hex: {}", hex::encode(&dns_buf));

            // Parse DNS response
            let dns_message = match DnsMessage::from_vec(&dns_buf) {
                Ok(msg) => msg,
                Err(e) => {
                    error!("DoT client {} failed to parse DNS message: {}", client_id, e);
                    continue;
                }
            };

            // Extract response information
            let query_id = dns_message.id();
            let response_code = format!("{:?}", dns_message.response_code());

            let answers: Vec<serde_json::Value> = dns_message.answers()
                .iter()
                .map(|record| {
                    let data_str = match record.data() {
                        Some(data) => format!("{}", data),
                        None => "NULL".to_string(),
                    };
                    serde_json::json!({
                        "name": record.name().to_utf8(),
                        "type": format!("{:?}", record.record_type()),
                        "ttl": record.ttl(),
                        "data": data_str,
                    })
                })
                .collect();

            let authorities: Vec<serde_json::Value> = dns_message.name_servers()
                .iter()
                .map(|record| {
                    let data_str = match record.data() {
                        Some(data) => format!("{}", data),
                        None => "NULL".to_string(),
                    };
                    serde_json::json!({
                        "name": record.name().to_utf8(),
                        "type": format!("{:?}", record.record_type()),
                        "ttl": record.ttl(),
                        "data": data_str,
                    })
                })
                .collect();

            let additionals: Vec<serde_json::Value> = dns_message.additionals()
                .iter()
                .map(|record| {
                    let data_str = match record.data() {
                        Some(data) => format!("{}", data),
                        None => "NULL".to_string(),
                    };
                    serde_json::json!({
                        "name": record.name().to_utf8(),
                        "type": format!("{:?}", record.record_type()),
                        "ttl": record.ttl(),
                        "data": data_str,
                    })
                })
                .collect();

            info!("DoT client {} received response: ID={}, Code={}, Answers={}",
                  client_id, query_id, response_code, answers.len());

            // Check state machine
            let mut client_data_lock = client_data.lock().await;

            match client_data_lock.state {
                ConnectionState::Processing => {
                    // Queue this data for later
                    debug!("DoT client {} queuing response (LLM processing)", client_id);
                    client_data_lock.state = ConnectionState::Accumulating;
                    client_data_lock.queued_responses.push(dns_buf);
                    continue;
                }
                ConnectionState::Accumulating => {
                    // Already accumulating, just continue queuing
                    debug!("DoT client {} already accumulating", client_id);
                    client_data_lock.queued_responses.push(dns_buf);
                    continue;
                }
                ConnectionState::Idle => {
                    // Normal processing
                    client_data_lock.state = ConnectionState::Processing;
                    drop(client_data_lock);
                }
            }

            // Call LLM with response
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(DotClientProtocol::new());
                let event = Event::new(
                    &DOT_CLIENT_RESPONSE_RECEIVED_EVENT,
                    serde_json::json!({
                        "query_id": query_id,
                        "response_code": response_code,
                        "answers": answers,
                        "authorities": authorities,
                        "additionals": additionals,
                    }),
                );

                match call_llm_for_client(
                    &llm_client,
                    &app_state,
                    client_id.to_string(),
                    &instruction,
                    &client_data.lock().await.memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            client_data.lock().await.memory = mem;
                        }

                        // Execute actions
                        for action_json in actions {
                            if let Err(e) = Self::execute_client_action(
                                client_id,
                                action_json,
                                &write_half,
                                &status_tx,
                                &app_state,
                            ).await {
                                error!("DoT client {} action execution failed: {}", client_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for DoT client {}: {}", client_id, e);
                    }
                }
            }

            // Set back to Idle and process queued responses
            let mut client_data_lock = client_data.lock().await;
            if !client_data_lock.queued_responses.is_empty() {
                client_data_lock.queued_responses.clear();
            }
            client_data_lock.state = ConnectionState::Idle;
        }

        Ok(())
    }

    /// Execute a client action
    async fn execute_client_action(
        client_id: ClientId,
        action_json: serde_json::Value,
        write_half: &Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
        status_tx: &mpsc::UnboundedSender<String>,
        app_state: &Arc<AppState>,
    ) -> Result<()> {
        let protocol = DotClientProtocol::new();
        let action_result = protocol.execute_action(action_json)?;

        match action_result {
            ClientActionResult::Custom { name, data } if name == "dns_query" => {
                // Send DNS query
                Self::send_dns_query(
                    client_id,
                    data.get("domain").and_then(|v| v.as_str()).context("Missing domain")?.to_string(),
                    data.get("query_type").and_then(|v| v.as_str()).context("Missing query_type")?.to_string(),
                    data.get("recursive").and_then(|v| v.as_bool()).unwrap_or(true),
                    write_half,
                    status_tx,
                ).await?;
            }
            ClientActionResult::Disconnect => {
                info!("DoT client {} disconnecting", client_id);
                // Remove client from app state, which will cause the read loop to exit
                app_state.remove_client(client_id).await;
                let _ = status_tx.send(format!("[CLIENT] DoT client {} disconnected", client_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            ClientActionResult::WaitForMore => {
                debug!("DoT client {} waiting for more data", client_id);
            }
            _ => {}
        }

        Ok(())
    }

    /// Send a DNS query over TLS
    async fn send_dns_query(
        client_id: ClientId,
        domain: String,
        query_type: String,
        recursive: bool,
        write_half: &Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("DoT client {} querying {} {}", client_id, domain, query_type);
        let _ = status_tx.send(format!("[CLIENT] DoT query: {} {}", domain, query_type));

        // Parse record type
        let record_type = RecordType::from_str(&query_type)
            .context(format!("Invalid query type: {}", query_type))?;

        // Parse domain name
        let name = Name::from_str(&domain)
            .context(format!("Invalid domain name: {}", domain))?;

        // Create DNS query message
        let mut message = DnsMessage::new();
        message.set_id(rand::random());
        message.set_message_type(MessageType::Query);
        message.set_op_code(OpCode::Query);
        message.set_recursion_desired(recursive);

        let query = Query::query(name, record_type);
        message.add_query(query);

        // Encode message
        let dns_bytes = message.to_vec()
            .context("Failed to encode DNS message")?;

        trace!("DoT query hex: {}", hex::encode(&dns_bytes));

        // Length-prefix the message
        let len = dns_bytes.len() as u16;
        let mut prefixed_message = len.to_be_bytes().to_vec();
        prefixed_message.extend_from_slice(&dns_bytes);

        // Send via TLS stream
        write_half.lock().await.write_all(&prefixed_message).await
            .context("Failed to send DNS query over TLS")?;

        debug!("DoT client {} sent DNS query ({} bytes)", client_id, prefixed_message.len());

        Ok(())
    }
}
