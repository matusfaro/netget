//! Bitcoin P2P protocol server implementation
pub mod actions;

use anyhow::{Context, Result};
use bitcoin::consensus::{Decodable, Encodable};
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::Magic;
use std::collections::HashMap;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use super::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::protocol::Event;
use crate::server::BitcoinProtocol;
use crate::state::app_state::AppState;
use actions::{BITCOIN_CONNECTION_OPENED_EVENT, BITCOIN_MESSAGE_RECEIVED_EVENT};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for Bitcoin protocol
struct ConnectionData {
    state: ConnectionState,
    queued_data: Vec<u8>,
    write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
    handshake_complete: bool,
}

/// Bitcoin P2P protocol server
pub struct BitcoinServer;

impl BitcoinServer {
    /// Spawn the Bitcoin P2P server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        network: String,
    ) -> Result<SocketAddr> {
        // Parse network magic
        let magic = match network.to_lowercase().as_str() {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => {
                info!("Unknown network '{}', defaulting to mainnet", network);
                let _ = status_tx.send(format!("[INFO] Unknown network '{}', defaulting to mainnet", network));
                Magic::BITCOIN
            }
        };

        // Create and bind TCP server
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Bitcoin P2P server listening on {} (network: {:?})", local_addr, magic);
        let _ = status_tx.send(format!("[INFO] Bitcoin P2P server listening on {} (network: {:?})", local_addr, magic));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(BitcoinProtocol::new());

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Accepted Bitcoin P2P connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] Accepted Bitcoin P2P connection {} from {}", connection_id, remote_addr));

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Bitcoin {
                                handshake_complete: false,
                                last_message_type: None,
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Handle connection opened event
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let write_half_for_conn = write_half_arc.clone();
                        let protocol_clone = protocol.clone();
                        let magic_clone = magic;
                        tokio::spawn(async move {
                            Self::handle_connection_opened(
                                connection_id,
                                server_id,
                                llm_client_clone,
                                app_state_clone,
                                status_tx_clone,
                                connections_clone,
                                write_half_for_conn,
                                protocol_clone,
                                magic_clone,
                            )
                            .await;
                        });

                        // Spawn reader task
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let protocol_clone = protocol.clone();
                        let magic_clone = magic;
                        tokio::spawn(async move {
                            let mut buffer = vec![0u8; 8192];
                            let mut read_half = read_half;

                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => {
                                        // Connection closed
                                        connections_clone.lock().await.remove(&connection_id);
                                        app_state_clone
                                            .close_connection_on_server(server_id, connection_id)
                                            .await;
                                        info!("Bitcoin connection {} closed", connection_id);
                                        let _ = status_tx_clone
                                            .send(format!("✗ Bitcoin connection {} closed", connection_id));
                                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                                        break;
                                    }
                                    Ok(n) => {
                                        let data = &buffer[..n];

                                        // DEBUG: Log binary data summary
                                        debug!("Bitcoin P2P received {} bytes on {}", n, connection_id);
                                        let _ = status_tx_clone.send(format!(
                                            "[DEBUG] Bitcoin P2P received {} bytes on {}",
                                            n, connection_id
                                        ));

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(data);
                                        trace!("Bitcoin P2P data (hex): {}", hex_str);
                                        let _ = status_tx_clone
                                            .send(format!("[TRACE] Bitcoin P2P data (hex): {}", hex_str));

                                        // Handle data in separate task
                                        let llm_clone = llm_client_clone.clone();
                                        let state_clone = app_state_clone.clone();
                                        let status_clone = status_tx_clone.clone();
                                        let conns_clone = connections_clone.clone();
                                        let protocol_clone = protocol_clone.clone();
                                        let data_vec = data.to_vec();
                                        tokio::spawn(async move {
                                            Self::handle_data_with_actions(
                                                connection_id,
                                                server_id,
                                                data_vec,
                                                llm_clone,
                                                state_clone,
                                                status_clone,
                                                conns_clone,
                                                protocol_clone,
                                                magic_clone,
                                            )
                                            .await;
                                        });
                                    }
                                    Err(e) => {
                                        error!("Read error on Bitcoin connection {}: {}", connection_id, e);
                                        let _ = status_tx_clone.send(format!(
                                            "[ERROR] Read error on Bitcoin connection {}: {}",
                                            connection_id, e
                                        ));
                                        connections_clone.lock().await.remove(&connection_id);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error on Bitcoin server: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Accept error on Bitcoin server: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle new connection opened event
    async fn handle_connection_opened(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        protocol: Arc<BitcoinProtocol>,
        magic: Magic,
    ) {
        // Add connection to tracking
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                queued_data: Vec::new(),
                write_half: write_half.clone(),
                handshake_complete: false,
            },
        );

        // Create connection opened event
        let event = Event::new(&BITCOIN_CONNECTION_OPENED_EVENT, serde_json::json!({}));

        // Call LLM to decide what to do (wait or send version message)
        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                debug!("LLM Bitcoin connection opened response received");

                // Display messages
                for msg in execution_result.messages {
                    let _ = status_tx.send(msg);
                }

                // Handle protocol results
                for protocol_result in execution_result.protocol_results {
                    match protocol_result {
                        ActionResult::Output(output_data) => {
                            if let Err(e) = Self::send_bitcoin_message(
                                &write_half,
                                &output_data,
                                connection_id,
                                &status_tx,
                                magic,
                            )
                            .await
                            {
                                error!("Failed to send Bitcoin message: {}", e);
                                let _ = status_tx
                                    .send(format!("✗ Failed to send Bitcoin message: {}", e));
                            }
                        }
                        ActionResult::CloseConnection => {
                            connections.lock().await.remove(&connection_id);
                            info!("Closed Bitcoin connection {} after connection opened", connection_id);
                            let _ = status_tx.send(format!(
                                "✗ Closed Bitcoin connection {} after connection opened",
                                connection_id
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("LLM error on Bitcoin connection opened: {}", e);
                let _ = status_tx.send(format!("✗ LLM error: {}", e));
            }
        }
    }

    /// Handle data received on a connection with LLM actions
    async fn handle_data_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Vec<u8>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<BitcoinProtocol>,
        magic: Magic,
    ) {
        // Check connection state
        let current_state = {
            let conns = connections.lock().await;
            if let Some(conn_data) = conns.get(&connection_id) {
                conn_data.state.clone()
            } else {
                return; // Connection not found
            }
        };

        // If processing, queue the data
        if current_state == ConnectionState::Processing {
            connections
                .lock()
                .await
                .entry(connection_id)
                .and_modify(|conn| {
                    conn.queued_data.extend_from_slice(&data);
                });
            debug!("Queued {} bytes for Bitcoin connection {}", data.len(), connection_id);
            let _ = status_tx.send(format!("⏸ Queued {} bytes for {}", data.len(), connection_id));
            return;
        }

        // Merge any queued data with new data
        let all_data = {
            let mut conns = connections.lock().await;
            let conn_data = conns.get_mut(&connection_id).unwrap();
            conn_data.state = ConnectionState::Processing;
            let mut merged = conn_data.queued_data.clone();
            merged.extend_from_slice(&data);
            conn_data.queued_data.clear();
            merged
        };

        loop {
            // Try to parse Bitcoin message
            let parsed_message = Self::try_parse_bitcoin_message(&all_data, magic);

            match parsed_message {
                Ok(Some((message, _remaining))) => {
                    // Successfully parsed a message
                    let payload = message.payload();
                    let message_type = Self::get_message_type_name(payload);
                    info!("Parsed Bitcoin message: {} from {}", message_type, connection_id);
                    let _ = status_tx
                        .send(format!("→ Received Bitcoin message: {}", message_type));

                    // Update connection info with message type
                    app_state
                        .update_bitcoin_connection_info(
                            server_id,
                            connection_id,
                            message_type.clone(),
                        )
                        .await;

                    // Get write_half for context
                    let write_half = {
                        let conns = connections.lock().await;
                        conns.get(&connection_id).map(|c| c.write_half.clone())
                    };

                    let Some(write_half) = write_half else {
                        return; // Connection not found
                    };

                    // Serialize message for LLM
                    let message_json = Self::serialize_message_to_json(payload);

                    // Create data received event
                    let event = Event::new(
                        &BITCOIN_MESSAGE_RECEIVED_EVENT,
                        serde_json::json!({
                            "message_type": message_type,
                            "message": message_json,
                        }),
                    );

                    // Call LLM
                    match call_llm(
                        &llm_client,
                        &app_state,
                        server_id,
                        Some(connection_id),
                        &event,
                        protocol.as_ref(),
                    )
                    .await
                    {
                        Ok(execution_result) => {
                            debug!("LLM Bitcoin response received");

                            // Display messages
                            for msg in execution_result.messages {
                                let _ = status_tx.send(msg);
                            }

                            // Handle protocol results
                            let mut should_close = false;

                            for protocol_result in execution_result.protocol_results {
                                match protocol_result {
                                    ActionResult::Output(output_data) => {
                                        if let Err(e) = Self::send_bitcoin_message(
                                            &write_half,
                                            &output_data,
                                            connection_id,
                                            &status_tx,
                                            magic,
                                        )
                                        .await
                                        {
                                            error!("Failed to send Bitcoin response: {}", e);
                                            let _ = status_tx.send(format!(
                                                "✗ Failed to send Bitcoin response: {}",
                                                e
                                            ));
                                        }
                                    }
                                    ActionResult::CloseConnection => {
                                        should_close = true;
                                    }
                                    _ => {}
                                }
                            }

                            // Handle close_connection
                            if should_close {
                                connections.lock().await.remove(&connection_id);
                                info!("Closed Bitcoin connection {}", connection_id);
                                let _ =
                                    status_tx.send(format!("✗ Closed connection {}", connection_id));
                                return;
                            }

                            // Check for queued data
                            let has_queued = {
                                let conns = connections.lock().await;
                                conns
                                    .get(&connection_id)
                                    .map(|c| !c.queued_data.is_empty())
                                    .unwrap_or(false)
                            };

                            if has_queued {
                                debug!("Processing queued data for Bitcoin connection {}", connection_id);
                                let _ = status_tx.send(format!(
                                    "▶ Processing queued data for {}",
                                    connection_id
                                ));
                                // Loop continues to process queued data
                            } else {
                                // Go to Idle state
                                connections
                                    .lock()
                                    .await
                                    .entry(connection_id)
                                    .and_modify(|conn| conn.state = ConnectionState::Idle);
                                return;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for Bitcoin data: {}", e);
                            let _ = status_tx.send(format!("✗ LLM error: {}", e));
                            connections
                                .lock()
                                .await
                                .entry(connection_id)
                                .and_modify(|conn| conn.state = ConnectionState::Idle);
                            return;
                        }
                    }
                }
                Ok(None) => {
                    // Need more data to complete message
                    debug!("Incomplete Bitcoin message, waiting for more data on {}", connection_id);
                    let _ =
                        status_tx.send(format!("⏳ Waiting for more data on {}", connection_id));
                    connections
                        .lock()
                        .await
                        .entry(connection_id)
                        .and_modify(|conn| {
                            conn.state = ConnectionState::Accumulating;
                            conn.queued_data = all_data.clone();
                        });
                    return;
                }
                Err(e) => {
                    // Parse error
                    error!("Failed to parse Bitcoin message on {}: {}", connection_id, e);
                    let _ = status_tx.send(format!(
                        "✗ Failed to parse Bitcoin message on {}: {}",
                        connection_id, e
                    ));
                    connections
                        .lock()
                        .await
                        .entry(connection_id)
                        .and_modify(|conn| conn.state = ConnectionState::Idle);
                    return;
                }
            }
        }
    }

    /// Try to parse a Bitcoin P2P message from raw bytes
    fn try_parse_bitcoin_message(
        data: &[u8],
        magic: Magic,
    ) -> Result<Option<(RawNetworkMessage, Vec<u8>)>> {
        if data.is_empty() {
            return Ok(None);
        }

        // Try to decode the message
        let mut cursor = Cursor::new(data);
        match RawNetworkMessage::consensus_decode(&mut cursor) {
            Ok(message) => {
                // Check magic bytes match
                if *message.magic() != magic {
                    return Err(anyhow::anyhow!(
                        "Magic bytes mismatch: expected {:?}, got {:?}",
                        magic,
                        *message.magic()
                    ));
                }

                // Calculate remaining bytes
                let position = cursor.position() as usize;
                let remaining = data[position..].to_vec();

                Ok(Some((message, remaining)))
            }
            Err(_) => {
                // Not enough data yet or parse error
                Ok(None)
            }
        }
    }

    /// Get message type name from NetworkMessage
    fn get_message_type_name(payload: &NetworkMessage) -> String {
        match payload {
            NetworkMessage::Version(_) => "version".to_string(),
            NetworkMessage::Verack => "verack".to_string(),
            NetworkMessage::Addr(_) => "addr".to_string(),
            NetworkMessage::Inv(_) => "inv".to_string(),
            NetworkMessage::GetData(_) => "getdata".to_string(),
            NetworkMessage::NotFound(_) => "notfound".to_string(),
            NetworkMessage::GetBlocks(_) => "getblocks".to_string(),
            NetworkMessage::GetHeaders(_) => "getheaders".to_string(),
            NetworkMessage::MemPool => "mempool".to_string(),
            NetworkMessage::Tx(_) => "tx".to_string(),
            NetworkMessage::Block(_) => "block".to_string(),
            NetworkMessage::Headers(_) => "headers".to_string(),
            NetworkMessage::SendHeaders => "sendheaders".to_string(),
            NetworkMessage::GetAddr => "getaddr".to_string(),
            NetworkMessage::Ping(_) => "ping".to_string(),
            NetworkMessage::Pong(_) => "pong".to_string(),
            NetworkMessage::MerkleBlock(_) => "merkleblock".to_string(),
            NetworkMessage::FilterLoad(_) => "filterload".to_string(),
            NetworkMessage::FilterAdd(_) => "filteradd".to_string(),
            NetworkMessage::FilterClear => "filterclear".to_string(),
            NetworkMessage::GetCFilters(_) => "getcfilters".to_string(),
            NetworkMessage::CFilter(_) => "cfilter".to_string(),
            NetworkMessage::GetCFHeaders(_) => "getcfheaders".to_string(),
            NetworkMessage::CFHeaders(_) => "cfheaders".to_string(),
            NetworkMessage::GetCFCheckpt(_) => "getcfcheckpt".to_string(),
            NetworkMessage::CFCheckpt(_) => "cfcheckpt".to_string(),
            NetworkMessage::SendCmpct(_) => "sendcmpct".to_string(),
            NetworkMessage::CmpctBlock(_) => "cmpctblock".to_string(),
            NetworkMessage::GetBlockTxn(_) => "getblocktxn".to_string(),
            NetworkMessage::BlockTxn(_) => "blocktxn".to_string(),
            NetworkMessage::Alert(_) => "alert".to_string(),
            NetworkMessage::Reject(_) => "reject".to_string(),
            NetworkMessage::FeeFilter(_) => "feefilter".to_string(),
            NetworkMessage::WtxidRelay => "wtxidrelay".to_string(),
            NetworkMessage::AddrV2(_) => "addrv2".to_string(),
            NetworkMessage::SendAddrV2 => "sendaddrv2".to_string(),
            NetworkMessage::Unknown { command, .. } => format!("unknown({})", command),
        }
    }

    /// Serialize NetworkMessage to JSON for LLM
    fn serialize_message_to_json(payload: &NetworkMessage) -> serde_json::Value {
        match payload {
            NetworkMessage::Version(v) => serde_json::json!({
                "version": v.version,
                "services": v.services.to_u64(),
                "timestamp": v.timestamp,
                "receiver": v.receiver.socket_addr().ok().map(|a| a.to_string()),
                "sender": v.sender.socket_addr().ok().map(|a| a.to_string()),
                "nonce": v.nonce,
                "user_agent": v.user_agent,
                "start_height": v.start_height,
                "relay": v.relay,
            }),
            NetworkMessage::Ping(nonce) => serde_json::json!({ "nonce": nonce }),
            NetworkMessage::Pong(nonce) => serde_json::json!({ "nonce": nonce }),
            NetworkMessage::Addr(addrs) => {
                let addr_strings: Vec<Option<String>> = addrs.iter()
                    .map(|(_, addr)| addr.socket_addr().ok().map(|a| a.to_string()))
                    .collect();
                serde_json::json!({ "count": addrs.len(), "addresses": addr_strings })
            }
            NetworkMessage::GetAddr => serde_json::json!({}),
            NetworkMessage::Verack => serde_json::json!({}),
            NetworkMessage::Inv(inv) => {
                serde_json::json!({ "count": inv.len(), "inventory": inv.iter().map(|i| format!("{:?}", i)).collect::<Vec<_>>() })
            }
            // For other message types, provide basic info
            _ => serde_json::json!({ "type": Self::get_message_type_name(payload) }),
        }
    }

    /// Send a Bitcoin message (raw bytes that will be wrapped in Bitcoin message format)
    async fn send_bitcoin_message(
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        data: &[u8],
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
        _magic: Magic,
    ) -> Result<()> {
        let mut write = write_half.lock().await;
        write
            .write_all(data)
            .await
            .context("Failed to write Bitcoin message")?;

        // DEBUG: Log binary data summary
        debug!("Bitcoin P2P sent {} bytes to {}", data.len(), connection_id);
        let _ = status_tx.send(format!(
            "[DEBUG] Bitcoin P2P sent {} bytes to {}",
            data.len(),
            connection_id
        ));

        // TRACE: Log full hex payload
        let hex_str = hex::encode(data);
        trace!("Bitcoin P2P sent (hex): {}", hex_str);
        let _ = status_tx.send(format!("[TRACE] Bitcoin P2P sent (hex): {}", hex_str));

        let _ = status_tx.send(format!("→ Sent Bitcoin message to {}", connection_id));

        Ok(())
    }
}
