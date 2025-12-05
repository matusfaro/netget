//! TFTP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::TftpProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_trace};
use actions::{TFTP_ACK_RECEIVED_EVENT, TFTP_DATA_BLOCK_EVENT, TFTP_READ_REQUEST_EVENT, TFTP_WRITE_REQUEST_EVENT};

/// Transfer ID (client address + transaction ID port)
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct TransferId {
    client_addr: SocketAddr,
}

impl TransferId {
    fn new(client_addr: SocketAddr) -> Self {
        Self { client_addr }
    }
}

/// TFTP transfer operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TftpOperation {
    Read,  // Server sends DATA, receives ACK
    Write, // Server receives DATA, sends ACK
}

/// Transfer state
#[allow(dead_code)]
struct TftpTransfer {
    transfer_id: TransferId,
    operation: TftpOperation,
    filename: String,
    mode: String,
    current_block: u16,
    connection_id: ConnectionId,
    server_socket: Arc<UdpSocket>, // Unique socket for this transfer (TID)
}

/// TFTP server that forwards requests to LLM
pub struct TftpServer;

impl TftpServer {
    /// Spawn TFTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // Main listening socket (port 69)
        let main_socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = main_socket.local_addr()?;
        info!("TFTP server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] TFTP server listening on {}", local_addr));

        let protocol = Arc::new(TftpProtocol::new());

        // Active transfers (keyed by TransferId)
        let transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Spawn main request listener (handles RRQ and WRQ)
        let main_socket_clone = main_socket.clone();
        let llm_clone = llm_client.clone();
        let state_clone = app_state.clone();
        let status_clone = status_tx.clone();
        let protocol_clone = protocol.clone();
        let transfers_clone = transfers.clone();

        tokio::spawn(async move {
            Self::handle_main_requests(
                main_socket_clone,
                llm_clone,
                state_clone,
                status_clone,
                server_id,
                protocol_clone,
                transfers_clone,
            )
            .await;
        });

        Ok(local_addr)
    }

    /// Handle incoming requests on main socket (RRQ and WRQ)
    async fn handle_main_requests(
        main_socket: Arc<UdpSocket>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<TftpProtocol>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) {
        let mut buffer = vec![0u8; 516]; // Max TFTP packet (4 bytes header + 512 data)

        loop {
            match main_socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    let data = buffer[..n].to_vec();

                    console_debug!(
                        status_tx,
                        "TFTP received {} bytes from {}",
                        n,
                        peer_addr
                    );
                    console_trace!(status_tx, "TFTP packet (hex): {}", hex::encode(&data));

                    // Parse TFTP opcode
                    if data.len() < 2 {
                        console_debug!(status_tx, "TFTP packet too short, ignoring");
                        continue;
                    }

                    let opcode = u16::from_be_bytes([data[0], data[1]]);

                    match opcode {
                        1 => {
                            // RRQ (Read Request)
                            let llm_clone = llm_client.clone();
                            let state_clone = app_state.clone();
                            let status_clone = status_tx.clone();
                            let protocol_clone = protocol.clone();
                            let transfers_clone = transfers.clone();

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_read_request(
                                    data,
                                    peer_addr,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    server_id,
                                    protocol_clone,
                                    transfers_clone,
                                )
                                .await
                                {
                                    error!("TFTP RRQ handling error: {}", e);
                                }
                            });
                        }
                        2 => {
                            // WRQ (Write Request)
                            let llm_clone = llm_client.clone();
                            let state_clone = app_state.clone();
                            let status_clone = status_tx.clone();
                            let protocol_clone = protocol.clone();
                            let transfers_clone = transfers.clone();

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_write_request(
                                    data,
                                    peer_addr,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    server_id,
                                    protocol_clone,
                                    transfers_clone,
                                )
                                .await
                                {
                                    error!("TFTP WRQ handling error: {}", e);
                                }
                            });
                        }
                        _ => {
                            console_debug!(
                                status_tx,
                                "TFTP unexpected opcode {} on main socket, ignoring",
                                opcode
                            );
                        }
                    }
                }
                Err(e) => {
                    error!("TFTP main socket error: {}", e);
                    let _ = status_tx.send(format!("[ERROR] TFTP main socket error: {}", e));
                }
            }
        }
    }

    /// Handle RRQ (Read Request)
    async fn handle_read_request(
        data: Vec<u8>,
        peer_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<TftpProtocol>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) -> Result<()> {
        // Parse RRQ: opcode(2) filename(string) 0 mode(string) 0
        let (filename, mode) = Self::parse_request(&data[2..])?;

        console_debug!(
            status_tx,
            "TFTP RRQ: filename='{}' mode='{}' from {}",
            filename,
            mode,
            peer_addr
        );

        // Create unique socket for this transfer (transaction ID)
        let transfer_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let tid_addr = transfer_socket.local_addr()?;

        console_debug!(
            status_tx,
            "TFTP RRQ assigned TID {}",
            tid_addr.port()
        );

        // Create connection entry
        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
        use crate::state::server::{
            ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo,
        };
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr: tid_addr,
            bytes_sent: 0,
            bytes_received: data.len() as u64,
            packets_sent: 0,
            packets_received: 1,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                "operation": "read",
                "filename": filename.clone(),
                "current_block": 0,
                "total_bytes": 0,
            })),
        };
        app_state
            .add_connection_to_server(server_id, conn_state)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Create transfer state
        let transfer_id = TransferId::new(peer_addr);
        let transfer = TftpTransfer {
            transfer_id,
            operation: TftpOperation::Read,
            filename: filename.clone(),
            mode: mode.clone(),
            current_block: 0,
            connection_id,
            server_socket: transfer_socket.clone(),
        };

        transfers.lock().await.insert(transfer_id, transfer);

        // Call LLM with read request event
        let event = Event::new(
            &TFTP_READ_REQUEST_EVENT,
            serde_json::json!({
                "filename": filename,
                "mode": mode,
                "client_addr": peer_addr.to_string(),
            }),
        );

        debug!("TFTP calling LLM for RRQ: {}", filename);
        let _ = status_tx.send(format!("[DEBUG] TFTP calling LLM for RRQ: {}", filename));

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
                // Display messages
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = status_tx.send(format!("[INFO] {}", message));
                }

                debug!(
                    "TFTP parsed {} actions for RRQ",
                    execution_result.raw_actions.len()
                );

                // Process protocol results (DATA or ERROR packets)
                for protocol_result in execution_result.protocol_results {
                    match protocol_result {
                        crate::llm::actions::protocol_trait::ActionResult::Output(packet) => {
                            // Send packet from transfer socket to client
                            if let Err(e) =
                                transfer_socket.send_to(&packet, peer_addr).await
                            {
                                error!("TFTP failed to send DATA: {}", e);
                                continue;
                            }

                            // Update bytes sent
                            app_state.update_connection_stats(server_id, connection_id, None, Some(packet.len() as u64), None, Some(1)).await;

                            console_trace!(
                                status_tx,
                                "TFTP sent DATA packet (hex): {}",
                                hex::encode(&packet)
                            );

                            // Check if this is DATA packet
                            if packet.len() >= 4 {
                                let opcode = u16::from_be_bytes([packet[0], packet[1]]);
                                if opcode == 3 {
                                    // DATA
                                    let block_num =
                                        u16::from_be_bytes([packet[2], packet[3]]);
                                    let data_len = packet.len() - 4;

                                    // Update transfer state
                                    if let Some(transfer) =
                                        transfers.lock().await.get_mut(&transfer_id)
                                    {
                                        transfer.current_block = block_num;
                                    }

                                    // Update connection info
                                    app_state
                                        .update_tftp_connection_block(
                                            server_id,
                                            connection_id,
                                            block_num,
                                            data_len,
                                        )
                                        .await;

                                    console_debug!(
                                        status_tx,
                                        "TFTP sent DATA block {} ({} bytes)",
                                        block_num,
                                        data_len
                                    );

                                    // If final block (< 512 bytes data), spawn ACK listener
                                    if data_len < 512 {
                                        console_debug!(
                                            status_tx,
                                            "TFTP final block sent, waiting for final ACK"
                                        );

                                        // Spawn listener for final ACK
                                        let socket_clone = transfer_socket.clone();
                                        let status_clone = status_tx.clone();
                                        let state_clone = app_state.clone();
                                        let transfers_clone = transfers.clone();

                                        tokio::spawn(async move {
                                            Self::wait_for_final_ack(
                                                socket_clone,
                                                peer_addr,
                                                transfer_id,
                                                block_num,
                                                server_id,
                                                connection_id,
                                                state_clone,
                                                status_clone,
                                                transfers_clone,
                                            )
                                            .await;
                                        });
                                    } else {
                                        // Spawn listener for ACK and continue transfer
                                        let socket_clone = transfer_socket.clone();
                                        let llm_clone = llm_client.clone();
                                        let state_clone = app_state.clone();
                                        let status_clone = status_tx.clone();
                                        let protocol_clone = protocol.clone();
                                        let transfers_clone = transfers.clone();

                                        tokio::spawn(async move {
                                            Self::continue_read_transfer(
                                                socket_clone,
                                                peer_addr,
                                                transfer_id,
                                                block_num,
                                                llm_clone,
                                                state_clone,
                                                status_clone,
                                                server_id,
                                                connection_id,
                                                protocol_clone,
                                                transfers_clone,
                                            )
                                            .await;
                                        });
                                    }
                                } else if opcode == 5 {
                                    // ERROR - transfer terminated
                                    console_debug!(status_tx, "TFTP sent ERROR, transfer terminated");
                                    transfers.lock().await.remove(&transfer_id);
                                    app_state
                                        .close_connection_on_server(server_id, connection_id)
                                        .await;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("TFTP LLM error for RRQ: {}", e);
                let _ = status_tx.send(format!("[ERROR] TFTP LLM error: {}", e));

                // Send ERROR packet
                let error_packet = Self::build_error_packet(0, "Internal error");
                let _ = transfer_socket.send_to(&error_packet, peer_addr).await;

                transfers.lock().await.remove(&transfer_id);
                app_state
                    .close_connection_on_server(server_id, connection_id)
                    .await;
            }
        }

        Ok(())
    }

    /// Continue read transfer by waiting for ACK and calling LLM for next block
    fn continue_read_transfer(
        socket: Arc<UdpSocket>,
        peer_addr: SocketAddr,
        transfer_id: TransferId,
        expected_block: u16,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        connection_id: ConnectionId,
        protocol: Arc<TftpProtocol>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
        let mut buffer = vec![0u8; 516];

        // Wait for ACK with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.recv_from(&mut buffer),
        )
        .await
        {
            Ok(Ok((n, _))) => {
                let data = &buffer[..n];

                if data.len() >= 4 {
                    let opcode = u16::from_be_bytes([data[0], data[1]]);
                    if opcode == 4 {
                        // ACK
                        let ack_block = u16::from_be_bytes([data[2], data[3]]);

                        console_debug!(status_tx, "TFTP received ACK for block {}", ack_block);

                        if ack_block == expected_block {
                            // Update connection
                            app_state.update_connection_stats(server_id, connection_id, Some(n as u64,
                                ), None, Some(1), None).await;

                            // Call LLM for next block
                            let event = Event::new(
                                &TFTP_ACK_RECEIVED_EVENT,
                                serde_json::json!({
                                    "block_number": ack_block,
                                }),
                            );

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
                                    // Process DATA packets from LLM
                                    for protocol_result in execution_result.protocol_results {
                                        if let crate::llm::actions::protocol_trait::ActionResult::Output(packet) = protocol_result {
                                            let _ = socket.send_to(&packet, peer_addr).await;

                                            app_state.update_connection_stats(server_id, connection_id, None, Some(packet.len() as u64), None, Some(1)).await;

                                            // Check if final block
                                            if packet.len() >= 4 && packet.len() - 4 < 512 {
                                                console_debug!(status_tx, "TFTP final block sent");

                                                // Wait for final ACK
                                                let socket_clone = socket.clone();
                                                let status_clone = status_tx.clone();
                                                let state_clone = app_state.clone();
                                                let transfers_clone = transfers.clone();
                                                let block_num = u16::from_be_bytes([packet[2], packet[3]]);

                                                tokio::spawn(async move {
                                                    Self::wait_for_final_ack(
                                                        socket_clone,
                                                        peer_addr,
                                                        transfer_id,
                                                        block_num,
                                                        server_id,
                                                        connection_id,
                                                        state_clone,
                                                        status_clone,
                                                        transfers_clone,
                                                    )
                                                    .await;
                                                });
                                            } else {
                                                // Continue transfer
                                                let block_num = u16::from_be_bytes([packet[2], packet[3]]);
                                                let socket_clone = socket.clone();
                                                let llm_clone = llm_client.clone();
                                                let state_clone = app_state.clone();
                                                let status_clone = status_tx.clone();
                                                let protocol_clone = protocol.clone();
                                                let transfers_clone = transfers.clone();

                                                tokio::spawn(async move {
                                                    Self::continue_read_transfer(
                                                        socket_clone,
                                                        peer_addr,
                                                        transfer_id,
                                                        block_num,
                                                        llm_clone,
                                                        state_clone,
                                                        status_clone,
                                                        server_id,
                                                        connection_id,
                                                        protocol_clone,
                                                        transfers_clone,
                                                    )
                                                    .await;
                                                });
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("TFTP LLM error during transfer: {}", e);
                                    transfers.lock().await.remove(&transfer_id);
                                    app_state.close_connection_on_server(server_id, connection_id).await;
                                }
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                error!("TFTP socket error waiting for ACK: {}", e);
                transfers.lock().await.remove(&transfer_id);
                app_state.close_connection_on_server(server_id, connection_id).await;
            }
            Err(_) => {
                console_debug!(status_tx, "TFTP timeout waiting for ACK block {}", expected_block);
                transfers.lock().await.remove(&transfer_id);
                app_state.close_connection_on_server(server_id, connection_id).await;
            }
        }
        })
    }

    /// Wait for final ACK and close transfer
    async fn wait_for_final_ack(
        socket: Arc<UdpSocket>,
        _peer_addr: SocketAddr,
        transfer_id: TransferId,
        expected_block: u16,
        server_id: crate::state::ServerId,
        connection_id: ConnectionId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) {
        let mut buffer = vec![0u8; 516];

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.recv_from(&mut buffer),
        )
        .await
        {
            Ok(Ok((n, _))) => {
                let data = &buffer[..n];

                if data.len() >= 4 {
                    let opcode = u16::from_be_bytes([data[0], data[1]]);
                    if opcode == 4 {
                        let ack_block = u16::from_be_bytes([data[2], data[3]]);

                        if ack_block == expected_block {
                            console_debug!(
                                status_tx,
                                "TFTP transfer complete (final ACK received for block {})",
                                ack_block
                            );

                            app_state.update_connection_stats(server_id, connection_id, Some(n as u64), None, Some(1), None).await;

                            transfers.lock().await.remove(&transfer_id);
                            app_state
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                        }
                    }
                }
            }
            _ => {
                console_debug!(
                    status_tx,
                    "TFTP timeout waiting for final ACK (transfer may have completed)"
                );
                transfers.lock().await.remove(&transfer_id);
                app_state
                    .close_connection_on_server(server_id, connection_id)
                    .await;
            }
        }
    }

    /// Handle WRQ (Write Request)
    async fn handle_write_request(
        data: Vec<u8>,
        peer_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<TftpProtocol>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) -> Result<()> {
        // Parse WRQ
        let (filename, mode) = Self::parse_request(&data[2..])?;

        console_debug!(
            status_tx,
            "TFTP WRQ: filename='{}' mode='{}' from {}",
            filename,
            mode,
            peer_addr
        );

        // Create unique socket for this transfer
        let transfer_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let tid_addr = transfer_socket.local_addr()?;

        console_debug!(status_tx, "TFTP WRQ assigned TID {}", tid_addr.port());

        // Create connection entry
        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
        use crate::state::server::{
            ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo,
        };
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr: tid_addr,
            bytes_sent: 0,
            bytes_received: data.len() as u64,
            packets_sent: 0,
            packets_received: 1,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                "operation": "write",
                "filename": filename.clone(),
                "current_block": 0,
                "total_bytes": 0,
            })),
        };
        app_state
            .add_connection_to_server(server_id, conn_state)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Create transfer state
        let transfer_id = TransferId::new(peer_addr);
        let transfer = TftpTransfer {
            transfer_id,
            operation: TftpOperation::Write,
            filename: filename.clone(),
            mode: mode.clone(),
            current_block: 0,
            connection_id,
            server_socket: transfer_socket.clone(),
        };

        transfers.lock().await.insert(transfer_id, transfer);

        // Call LLM with write request event
        let event = Event::new(
            &TFTP_WRITE_REQUEST_EVENT,
            serde_json::json!({
                "filename": filename,
                "mode": mode,
                "client_addr": peer_addr.to_string(),
            }),
        );

        debug!("TFTP calling LLM for WRQ: {}", filename);
        let _ = status_tx.send(format!("[DEBUG] TFTP calling LLM for WRQ: {}", filename));

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
                // Display messages
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = status_tx.send(format!("[INFO] {}", message));
                }

                // Process protocol results (should be ACK block 0 or ERROR)
                for protocol_result in execution_result.protocol_results {
                    match protocol_result {
                        crate::llm::actions::protocol_trait::ActionResult::Output(packet) => {
                            // Send ACK block 0 or ERROR
                            if let Err(e) = transfer_socket.send_to(&packet, peer_addr).await {
                                error!("TFTP failed to send ACK: {}", e);
                                continue;
                            }

                            app_state.update_connection_stats(server_id, connection_id, None, Some(packet.len() as u64), None, Some(1)).await;

                            console_trace!(
                                status_tx,
                                "TFTP sent packet (hex): {}",
                                hex::encode(&packet)
                            );

                            // Check if ACK (opcode 4)
                            if packet.len() >= 4 {
                                let opcode = u16::from_be_bytes([packet[0], packet[1]]);
                                if opcode == 4 {
                                    console_debug!(status_tx, "TFTP sent ACK block 0, ready to receive");

                                    // Spawn listener for incoming DATA blocks
                                    let socket_clone = transfer_socket.clone();
                                    let llm_clone = llm_client.clone();
                                    let state_clone = app_state.clone();
                                    let status_clone = status_tx.clone();
                                    let protocol_clone = protocol.clone();
                                    let transfers_clone = transfers.clone();

                                    tokio::spawn(async move {
                                        Self::receive_write_data(
                                            socket_clone,
                                            peer_addr,
                                            transfer_id,
                                            llm_clone,
                                            state_clone,
                                            status_clone,
                                            server_id,
                                            connection_id,
                                            protocol_clone,
                                            transfers_clone,
                                        )
                                        .await;
                                    });
                                } else if opcode == 5 {
                                    console_debug!(status_tx, "TFTP sent ERROR, transfer denied");
                                    transfers.lock().await.remove(&transfer_id);
                                    app_state
                                        .close_connection_on_server(server_id, connection_id)
                                        .await;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("TFTP LLM error for WRQ: {}", e);
                let _ = status_tx.send(format!("[ERROR] TFTP LLM error: {}", e));

                let error_packet = Self::build_error_packet(0, "Internal error");
                let _ = transfer_socket.send_to(&error_packet, peer_addr).await;

                transfers.lock().await.remove(&transfer_id);
                app_state
                    .close_connection_on_server(server_id, connection_id)
                    .await;
            }
        }

        Ok(())
    }

    /// Receive DATA blocks for write transfer
    async fn receive_write_data(
        socket: Arc<UdpSocket>,
        peer_addr: SocketAddr,
        transfer_id: TransferId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        connection_id: ConnectionId,
        protocol: Arc<TftpProtocol>,
        transfers: Arc<Mutex<HashMap<TransferId, TftpTransfer>>>,
    ) {
        let mut buffer = vec![0u8; 516];

        loop {
            match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                socket.recv_from(&mut buffer),
            )
            .await
            {
                Ok(Ok((n, _))) => {
                    let data = buffer[..n].to_vec();

                    if data.len() < 4 {
                        console_debug!(status_tx, "TFTP received short packet, ignoring");
                        continue;
                    }

                    let opcode = u16::from_be_bytes([data[0], data[1]]);
                    if opcode != 3 {
                        console_debug!(
                            status_tx,
                            "TFTP expected DATA (opcode 3), got opcode {}",
                            opcode
                        );
                        continue;
                    }

                    let block_num = u16::from_be_bytes([data[2], data[3]]);
                    let block_data = &data[4..];

                    console_debug!(
                        status_tx,
                        "TFTP received DATA block {} ({} bytes)",
                        block_num,
                        block_data.len()
                    );

                    app_state.update_connection_stats(server_id, connection_id, Some(n as u64), None, Some(1), None).await;

                    // Update transfer state
                    if let Some(transfer) = transfers.lock().await.get_mut(&transfer_id) {
                        transfer.current_block = block_num;
                    }

                    app_state
                        .update_tftp_connection_block(
                            server_id,
                            connection_id,
                            block_num,
                            block_data.len(),
                        )
                        .await;

                    // Call LLM with data block event
                    let is_final = block_data.len() < 512;
                    let event = Event::new(
                        &TFTP_DATA_BLOCK_EVENT,
                        serde_json::json!({
                            "block_number": block_num,
                            "data_hex": hex::encode(block_data),
                            "data_length": block_data.len(),
                            "is_final": is_final,
                        }),
                    );

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
                            // Process ACK packets
                            for protocol_result in execution_result.protocol_results {
                                if let crate::llm::actions::protocol_trait::ActionResult::Output(
                                    packet,
                                ) = protocol_result
                                {
                                    let _ = socket.send_to(&packet, peer_addr).await;

                                    app_state.update_connection_stats(server_id, connection_id, None, Some(packet.len() as u64), None, Some(1)).await;

                                    console_debug!(
                                        status_tx,
                                        "TFTP sent ACK for block {}",
                                        block_num
                                    );
                                }
                            }

                            // If final block, close transfer
                            if is_final {
                                console_debug!(
                                    status_tx,
                                    "TFTP write transfer complete (final block {} received)",
                                    block_num
                                );
                                transfers.lock().await.remove(&transfer_id);
                                app_state
                                    .close_connection_on_server(server_id, connection_id)
                                    .await;
                                break;
                            }
                        }
                        Err(e) => {
                            error!("TFTP LLM error during write: {}", e);
                            transfers.lock().await.remove(&transfer_id);
                            app_state
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            break;
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("TFTP socket error: {}", e);
                    transfers.lock().await.remove(&transfer_id);
                    app_state
                        .close_connection_on_server(server_id, connection_id)
                        .await;
                    break;
                }
                Err(_) => {
                    console_debug!(status_tx, "TFTP timeout waiting for DATA");
                    transfers.lock().await.remove(&transfer_id);
                    app_state
                        .close_connection_on_server(server_id, connection_id)
                        .await;
                    break;
                }
            }
        }
    }

    /// Parse RRQ/WRQ request packet
    fn parse_request(data: &[u8]) -> Result<(String, String)> {
        // Format: filename\0mode\0
        let null_positions: Vec<usize> = data
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| if b == 0 { Some(i) } else { None })
            .collect();

        if null_positions.len() < 2 {
            return Err(anyhow::anyhow!("Invalid TFTP request format"));
        }

        let filename = String::from_utf8_lossy(&data[..null_positions[0]]).to_string();
        let mode = String::from_utf8_lossy(&data[null_positions[0] + 1..null_positions[1]])
            .to_string();

        Ok((filename, mode))
    }

    /// Build TFTP ERROR packet
    fn build_error_packet(error_code: u16, message: &str) -> Vec<u8> {
        let mut packet = Vec::with_capacity(4 + message.len() + 1);
        packet.extend_from_slice(&5u16.to_be_bytes()); // Opcode ERROR
        packet.extend_from_slice(&error_code.to_be_bytes());
        packet.extend_from_slice(message.as_bytes());
        packet.push(0); // Null terminator
        packet
    }
}
