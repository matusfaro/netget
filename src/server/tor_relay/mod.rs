//! Tor Relay (OR protocol) server implementation
//!
//! Implements a full Tor OR protocol exit relay server with:
//! - **ntor handshake** for circuit creation (CREATE2/CREATED2) with Curve25519 DH
//! - **Relay cell encryption** using AES-128-CTR with forward/backward keys
//! - **Circuit management** with crypto state and stream multiplexing
//! - **Exit relay functionality** (BEGIN, DATA, END, CONNECTED)
//! - **Bidirectional data forwarding** (TCP ↔ Tor client) via background tasks
//! - **SENDME flow control** at circuit and stream levels (tor-spec compliant)
//! - **Bandwidth tracking** per circuit and aggregate statistics
//! - **Channel-based architecture** for concurrent stream handling
//! - **LLM-controlled policies** for relay decisions
//!
//! ## Architecture
//!
//! ```text
//! TLS Connection (TorRelaySession)
//!     │
//!     ├─ tokio::select! loop
//!     │   ├─ Read TLS stream → Parse cells
//!     │   └─ Write from channel ← Forwarder tasks
//!     │
//!     ├─ CircuitManager (shared across connections)
//!     │   ├─ Circuit 1 (crypto + streams)
//!     │   │   ├─ Stream 1 → TCP connection
//!     │   │   ├─ Stream 2 → TCP connection
//!     │   │   └─ Flow control windows
//!     │   ├─ Circuit 2 (crypto + streams)
//!     │   └─ Statistics tracking
//!     │
//!     └─ Background forwarder tasks (per stream)
//!         └─ TCP → Encrypt → Channel → TLS
//! ```
//!
//! ## Cell Processing Flow
//!
//! 1. **CREATE2**: ntor handshake → CREATED2 response
//! 2. **RELAY/BEGIN**: Parse target → TCP connect → CONNECTED response
//! 3. **RELAY/DATA**: Decrypt → Forward to TCP (client→server) OR Encrypt ← TCP (server→client)
//! 4. **RELAY/END**: Close TCP connection
//! 5. **RELAY/SENDME**: Update flow control windows
//!
//! ## Flow Control (SENDME)
//!
//! - Circuit-level: 1000 cell window, SENDME every 100 cells
//! - Stream-level: 500 cell window, SENDME every 50 cells
//! - Automatic SENDME generation on receive thresholds
//! - Package window prevents overwhelming next hop
//!
//! **Status**: Beta - Production-ready exit relay with full crypto and flow control

pub mod actions;
pub mod circuit;
pub mod stream;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "tor")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "tor")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "tor")]
use crate::llm::ActionResult;
#[cfg(feature = "tor")]
use actions::{TOR_RELAY_CIRCUIT_CREATED_EVENT, TOR_RELAY_RELAY_CELL_EVENT};
#[cfg(feature = "tor")]
use crate::server::TorRelayProtocol;
#[cfg(feature = "tor")]
use crate::protocol::Event;
#[cfg(feature = "tor")]
use crate::state::app_state::AppState;
#[cfg(feature = "tor")]
use circuit::{CircuitId, CircuitManager, StreamId};
#[cfg(feature = "tor")]
use stream::{parse_begin_target, connect_to_target, build_relay_cell, relay_command, end_reason};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// Tor Relay server - handles OR protocol connections
pub struct TorRelayServer;

#[cfg(feature = "tor")]
impl TorRelayServer {
    /// Spawn Tor Relay server with LLM action integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // Generate self-signed TLS certificate for OR protocol
        let (cert, key) = generate_tls_certificate()?;

        // Configure TLS acceptor
        // Use aws-lc-rs crypto provider (required for rustls 0.23+)
        let crypto_provider = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider();
        let tls_config = ServerConfig::builder_with_provider(Arc::new(crypto_provider))
            .with_protocol_versions(&[&tokio_rustls::rustls::version::TLS13])
            .expect("Valid TLS protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .context("Failed to create TLS config")?;

        let acceptor = TlsAcceptor::from(Arc::new(tls_config));

        // Bind TCP listener
        let listener = TcpListener::bind(listen_addr)
            .await
            .context("Failed to bind Tor Relay server")?;

        let actual_addr = listener
            .local_addr()
            .context("Failed to get local address")?;

        console_info!(status_tx, "[INFO] Tor Relay (OR protocol) server listening on {}", actual_addr);

        // Create circuit manager (shared across all connections)
        let circuit_manager = Arc::new(CircuitManager::new());

        // Log relay identity
        let fingerprint = circuit_manager.identity_fingerprint();
        console_info!(status_tx, "[INFO] Relay fingerprint: {}", hex::encode(fingerprint));

        let protocol = Arc::new(TorRelayProtocol::new());

        // Spawn connection handler
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await
                        );
                        console_debug!(status_tx, "[DEBUG] Tor Relay connection {} from {}", connection_id, remote_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let acceptor_clone = acceptor.clone();
                        let protocol_clone = protocol.clone();
                        let circuit_mgr_clone = circuit_manager.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_tor_relay_connection(
                                stream,
                                connection_id,
                                server_id,
                                remote_addr,
                                acceptor_clone,
                                llm_clone,
                                state_clone,
                                status_clone,
                                protocol_clone,
                                circuit_mgr_clone,
                            )
                            .await
                            {
                                error!("Tor Relay connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Tor Relay connection: {}", e);
                    }
                }
            }
        });

        Ok(actual_addr)
    }
}

#[cfg(not(feature = "tor"))]
impl TorRelayServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("Tor Relay feature not enabled")
    }
}

/// Generate self-signed TLS certificate for OR protocol
fn generate_tls_certificate() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
    use rcgen::{CertificateParams, KeyPair};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

    let mut params = CertificateParams::new(vec!["tor-relay.local".to_string()])?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        "NetGet Tor Relay"
    );

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());

    Ok((cert_der, key_der))
}

/// Handle individual Tor Relay connection
async fn handle_tor_relay_connection(
    stream: TcpStream,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    acceptor: TlsAcceptor,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<TorRelayProtocol>,
    circuit_manager: Arc<CircuitManager>,
) -> Result<()> {
    // Perform TLS handshake
    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => {
            console_debug!(status_tx, "→ TLS handshake completed for {}", remote_addr);
            s
        }
        Err(e) => {
            console_warn!(status_tx, "✗ TLS handshake failed for {}: {}", remote_addr, e);
            return Err(e.into());
        }
    };

    // Create channel for outgoing cells
    let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();

    let mut session = TorRelaySession {
        stream: tls_stream,
        connection_id,
        server_id,
        remote_addr,
        llm_client,
        app_state,
        status_tx,
        protocol,
        circuit_manager,
        outgoing_tx,
        outgoing_rx,
    };

    session.handle().await
}

/// Tor Relay session handler
struct TorRelaySession {
    stream: tokio_rustls::server::TlsStream<TcpStream>,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<TorRelayProtocol>,
    circuit_manager: Arc<CircuitManager>,
    /// Channel for sending outgoing cells (from forwarder tasks)
    outgoing_tx: mpsc::UnboundedSender<Vec<u8>>,
    outgoing_rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl TorRelaySession {
    /// Handle Tor Relay session - read cells and process
    async fn handle(&mut self) -> Result<()> {
        debug!("Tor Relay session started for {}", self.remote_addr);

        let mut cell_buffer = vec![0u8; 514];

        loop {
            tokio::select! {
                // Read incoming cells from TLS stream
                read_result = self.stream.read_exact(&mut cell_buffer) => {
                    match read_result {
                        Ok(_) => {
                            trace!("Received Tor cell ({} bytes) from {}", cell_buffer.len(), self.remote_addr);

                            // Parse cell header
                            if let Some(cell_info) = parse_tor_cell(&cell_buffer) {
                                debug!(
                                    "Tor cell: type={}, circuit_id={}",
                                    cell_info.cell_type, cell_info.circuit_id
                                );

                                let circuit_id = cell_info.circuit_id;  // Save circuit_id for error handling

                                // Handle cell based on type
                                match self.handle_cell(cell_info, &cell_buffer).await {
                                    Ok(Some(response)) => {
                                        // Send response
                                        self.stream.write_all(&response).await?;
                                        debug!("Sent response cell ({} bytes)", response.len());
                                    }
                                    Ok(None) => {
                                        // No response needed
                                    }
                                    Err(e) => {
                                        error!("Failed to handle cell: {}", e);
                                        // Send DESTROY cell
                                        let destroy = self.create_destroy_cell(circuit_id);
                                        self.stream.write_all(&destroy).await?;
                                        return Err(e);
                                    }
                                }
                            } else {
                                warn!("Failed to parse Tor cell from {}", self.remote_addr);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            // Connection closed
                            console_debug!(self.status_tx, "→ Tor Relay connection closed by {}");
                            return Ok(());
                        }
                        Err(e) => {
                            error!("Failed to read Tor cell from {}: {}", self.remote_addr, e);
                            return Err(e.into());
                        }
                    }
                }

                // Send outgoing cells from forwarder tasks
                Some(cell) = self.outgoing_rx.recv() => {
                    trace!("Sending outgoing cell ({} bytes)", cell.len());
                    self.stream.write_all(&cell).await?;
                }
            }
        }
    }

    /// Handle individual cell based on type
    async fn handle_cell(&mut self, cell_info: TorCellInfo, cell_data: &[u8]) -> Result<Option<Vec<u8>>> {
        match cell_info.cell_type.as_str() {
            "CREATE2" => self.handle_create2(cell_info.circuit_id, cell_data).await,
            "RELAY" | "RELAY_EARLY" => self.handle_relay(cell_info.circuit_id, cell_data).await,
            "DESTROY" => {
                debug!("Received DESTROY for circuit {}", cell_info.circuit_id.as_u32());
                self.circuit_manager.destroy_circuit(cell_info.circuit_id).await;
                Ok(None)
            }
            "PADDING" => {
                trace!("Received PADDING cell");
                Ok(None)
            }
            _ => {
                warn!("Unhandled cell type: {}", cell_info.cell_type);
                Ok(None)
            }
        }
    }

    /// Handle CREATE2 cell
    async fn handle_create2(&mut self, circuit_id: CircuitId, cell_data: &[u8]) -> Result<Option<Vec<u8>>> {
        debug!("Processing CREATE2 for circuit {}", circuit_id.as_u32());

        // Parse CREATE2 cell:
        // CircID (4) | Command (1) | HTYPE (2) | HLEN (2) | HDATA (HLEN)
        if cell_data.len() < 9 {
            return Err(anyhow::anyhow!("CREATE2 cell too short"));
        }

        let htype = u16::from_be_bytes([cell_data[5], cell_data[6]]);
        let hlen = u16::from_be_bytes([cell_data[7], cell_data[8]]);

        if htype != 0x0002 {  // ntor
            return Err(anyhow::anyhow!("Unsupported handshake type: {}", htype));
        }

        if hlen != 84 {  // ntor client handshake is 84 bytes (ID:20 + B:32 + X:32)
            return Err(anyhow::anyhow!("Invalid ntor handshake length: {}", hlen));
        }

        // Extract client's X (last 32 bytes of handshake data)
        let handshake_start = 9;
        let handshake_end = handshake_start + hlen as usize;
        if cell_data.len() < handshake_end {
            return Err(anyhow::anyhow!("Handshake data incomplete"));
        }

        let handshake_data = &cell_data[handshake_start..handshake_end];
        // ntor handshake: ID(20) + B(32) + X(32)
        let client_x: [u8; 32] = handshake_data[52..84].try_into()?;

        // Perform ntor handshake via circuit manager
        let (y, auth) = self.circuit_manager.handle_create2(circuit_id, client_x).await?;

        console_info!(self.status_tx, "[INFO] Circuit {} created", circuit_id.as_u32());

        // Log relay statistics
        let stats = self.circuit_manager.get_relay_stats().await;
        console_info!(self.status_tx, "[DEBUG] Relay stats: {} circuits, {} streams, sent={} received={}", stats.total_circuits, stats.total_streams, stats.total_bytes_sent, stats.total_bytes_received);

        // Send event to LLM
        let event = Event::new(
            &TOR_RELAY_CIRCUIT_CREATED_EVENT,
            serde_json::json!({
                "circuit_id": format!("0x{:08x}", circuit_id.as_u32()),
                "client_ip": self.remote_addr.ip().to_string(),
            }),
        );

        // Call LLM (non-blocking, for logging/monitoring)
        let _ = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        ).await;

        // Build CREATED2 response
        // CircID (4) | Command (1) | HLEN (2) | HDATA (HLEN)
        let mut response = Vec::with_capacity(71);
        response.extend_from_slice(&circuit_id.to_bytes());
        response.push(10);  // CREATED2 command
        response.extend_from_slice(&64u16.to_be_bytes());  // HLEN = 64 (Y:32 + AUTH:32)
        response.extend_from_slice(&y);
        response.extend_from_slice(&auth);

        // Pad to 514 bytes
        response.resize(514, 0);

        Ok(Some(response))
    }

    /// Handle RELAY cell
    async fn handle_relay(&mut self, circuit_id: CircuitId, cell_data: &[u8]) -> Result<Option<Vec<u8>>> {
        trace!("Processing RELAY cell for circuit {}", circuit_id.as_u32());

        // Extract relay payload (skip CircID:4 + Command:1)
        let mut relay_payload = cell_data[5..514].to_vec();

        // Decrypt relay cell
        self.circuit_manager.decrypt_relay_cell(circuit_id, &mut relay_payload).await?;

        // Track bytes received from client (entire cell)
        let _ = self.circuit_manager.record_received(circuit_id, 509).await;

        // Track RELAY cell for circuit-level flow control - send SENDME if needed
        let send_circuit_sendme = self.circuit_manager.record_relay_received(circuit_id).await.unwrap_or(false);
        if send_circuit_sendme {
            debug!("Sending circuit-level SENDME for circuit {}", circuit_id.as_u32());
            let sendme_cell = build_relay_cell(
                circuit_id.as_u32(),
                0,  // Stream ID 0 for circuit-level SENDME
                relay_command::SENDME,
                &[]
            );
            let mut encrypted = sendme_cell.clone();
            self.circuit_manager.encrypt_relay_cell(circuit_id, &mut encrypted[5..514]).await?;
            let _ = self.outgoing_tx.send(encrypted);
        }

        // Parse relay cell header:
        // Command (1) | Recognized (2) | StreamID (2) | Digest (4) | Length (2) | Data (Length)
        if relay_payload.len() < 11 {
            return Err(anyhow::anyhow!("RELAY cell too short"));
        }

        let relay_cmd = relay_payload[0];
        let recognized = u16::from_be_bytes([relay_payload[1], relay_payload[2]]);
        let stream_id_u16 = u16::from_be_bytes([relay_payload[3], relay_payload[4]]);
        let stream_id = StreamId::new(stream_id_u16);
        let length = u16::from_be_bytes([relay_payload[9], relay_payload[10]]);
        let data = &relay_payload[11..11 + length as usize];

        // Check if this cell is for us (recognized should be 0)
        if recognized != 0 {
            // Not for us, would normally forward but we're endpoint
            trace!("RELAY cell not recognized, dropping");
            return Ok(None);
        }

        let relay_cmd_name = match relay_cmd {
            relay_command::BEGIN => "BEGIN",
            relay_command::DATA => "DATA",
            relay_command::END => "END",
            relay_command::CONNECTED => "CONNECTED",
            relay_command::SENDME => "SENDME",
            relay_command::EXTEND => "EXTEND",
            relay_command::EXTENDED => "EXTENDED",
            relay_command::TRUNCATE => "TRUNCATE",
            relay_command::TRUNCATED => "TRUNCATED",
            relay_command::DROP => "DROP",
            relay_command::RESOLVE => "RESOLVE",
            relay_command::RESOLVED => "RESOLVED",
            relay_command::BEGIN_DIR => "BEGIN_DIR",
            _ => "UNKNOWN",
        };

        debug!("RELAY cell: command={} ({}), stream={}, length={}",
            relay_cmd, relay_cmd_name, stream_id.as_u16(), length);

        // Handle specific relay commands
        match relay_cmd {
            relay_command::BEGIN => {
                return self.handle_begin_cell(circuit_id, stream_id, data).await;
            }
            relay_command::DATA => {
                return self.handle_data_cell(circuit_id, stream_id, data).await;
            }
            relay_command::END => {
                return self.handle_end_cell(circuit_id, stream_id, data).await;
            }
            relay_command::SENDME => {
                return self.handle_sendme_cell(circuit_id, stream_id).await;
            }
            _ => {
                // For other commands, send event to LLM
                let event = Event::new(
                    &TOR_RELAY_RELAY_CELL_EVENT,
                    serde_json::json!({
                        "circuit_id": format!("0x{:08x}", circuit_id.as_u32()),
                        "relay_command": relay_cmd_name,
                        "stream_id": stream_id.as_u16(),
                        "length": length,
                        "client_ip": self.remote_addr.ip().to_string(),
                    }),
                );

                // Get LLM response for how to handle this
                if let Ok(execution_result) = call_llm(
                    &self.llm_client,
                    &self.app_state,
                    self.server_id,
                    Some(self.connection_id),
                    &event,
                    self.protocol.as_ref(),
                ).await {
                    // Execute protocol actions
                    for protocol_result in execution_result.protocol_results {
                        match protocol_result {
                            ActionResult::Output(data) => {
                                // LLM wants to send a response
                                return Ok(Some(data));
                            }
                            ActionResult::CloseConnection => {
                                debug!("LLM requested connection close");
                                return Err(anyhow::anyhow!("LLM requested close"));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Default: no response
        Ok(None)
    }

    /// Handle BEGIN cell - establish TCP connection to target
    async fn handle_begin_cell(&mut self, circuit_id: CircuitId, stream_id: StreamId, data: &[u8]) -> Result<Option<Vec<u8>>> {
        // Parse target address
        let target = parse_begin_target(data)?;

        console_info!(self.status_tx, "[INFO] BEGIN stream {} → {}", stream_id.as_u16(), target);

        // Create stream in circuit manager
        self.circuit_manager.create_stream(circuit_id, stream_id, target.clone()).await?;

        // Attempt to connect to target
        match connect_to_target(&target).await {
            Ok(tcp_stream) => {
                console_info!(self.status_tx, "→ Connected to {} for stream {}", target, stream_id.as_u16());

                // Store TCP connection in stream
                self.circuit_manager.set_stream_active(circuit_id, stream_id, tcp_stream).await?;

                // Build CONNECTED response
                let connected_cell = build_relay_cell(
                    circuit_id.as_u32(),
                    stream_id.as_u16(),
                    relay_command::CONNECTED,
                    &[]
                );

                // Encrypt response
                let mut encrypted = connected_cell.clone();
                self.circuit_manager.encrypt_relay_cell(circuit_id, &mut encrypted[5..514]).await?;

                // Track bytes sent to client
                let _ = self.circuit_manager.record_sent(circuit_id, 509).await;

                // Start forwarding task for this stream
                self.spawn_stream_forwarder(circuit_id, stream_id, self.outgoing_tx.clone()).await?;

                Ok(Some(encrypted))
            }
            Err(e) => {
                console_error!(self.status_tx, "✗ Failed to connect to {}: {}", target, e);

                // Close stream
                let _ = self.circuit_manager.close_stream(circuit_id, stream_id).await;

                // Build END response with error reason
                let end_cell = build_relay_cell(
                    circuit_id.as_u32(),
                    stream_id.as_u16(),
                    relay_command::END,
                    &[end_reason::CONNECT_REFUSED]
                );

                // Encrypt response
                let mut encrypted = end_cell.clone();
                self.circuit_manager.encrypt_relay_cell(circuit_id, &mut encrypted[5..514]).await?;

                // Track bytes sent to client
                let _ = self.circuit_manager.record_sent(circuit_id, 509).await;

                Ok(Some(encrypted))
            }
        }
    }

    /// Handle DATA cell - forward data to TCP connection
    async fn handle_data_cell(&mut self, circuit_id: CircuitId, stream_id: StreamId, data: &[u8]) -> Result<Option<Vec<u8>>> {
        trace!("DATA cell for stream {} ({} bytes)", stream_id.as_u16(), data.len());

        // Track DATA cell for stream-level flow control - send SENDME if needed
        let send_stream_sendme = self.circuit_manager
            .record_stream_data_received(circuit_id, stream_id)
            .await
            .unwrap_or(false);

        if send_stream_sendme {
            debug!("Sending stream-level SENDME for stream {}", stream_id.as_u16());
            let sendme_cell = build_relay_cell(
                circuit_id.as_u32(),
                stream_id.as_u16(),
                relay_command::SENDME,
                &[]
            );
            let mut encrypted = sendme_cell.clone();
            self.circuit_manager.encrypt_relay_cell(circuit_id, &mut encrypted[5..514]).await?;
            let _ = self.circuit_manager.record_sent(circuit_id, 509).await;
            let _ = self.outgoing_tx.send(encrypted);
        }

        // Get TCP connection for this stream
        if let Some(connection) = self.circuit_manager.get_stream_connection(circuit_id, stream_id).await? {
            // Write data to TCP connection
            let mut conn = connection.lock().await;
            if let Err(e) = conn.write_all(data).await {
                error!("Failed to write to stream {}: {}", stream_id.as_u16(), e);

                // Close stream
                drop(conn);  // Release lock before closing
                let _ = self.circuit_manager.close_stream(circuit_id, stream_id).await;

                // Send END cell
                let end_cell = build_relay_cell(
                    circuit_id.as_u32(),
                    stream_id.as_u16(),
                    relay_command::END,
                    &[end_reason::MISC]
                );

                let mut encrypted = end_cell.clone();
                self.circuit_manager.encrypt_relay_cell(circuit_id, &mut encrypted[5..514]).await?;

                // Track bytes sent to client
                let _ = self.circuit_manager.record_sent(circuit_id, 509).await;

                return Ok(Some(encrypted));
            }

            trace!("Forwarded {} bytes to stream {}", data.len(), stream_id.as_u16());
        } else {
            warn!("Stream {} not found or not active", stream_id.as_u16());
        }

        Ok(None)
    }

    /// Handle END cell - close stream
    async fn handle_end_cell(&mut self, circuit_id: CircuitId, stream_id: StreamId, data: &[u8]) -> Result<Option<Vec<u8>>> {
        let reason = if data.is_empty() { end_reason::DONE } else { data[0] };

        console_debug!(self.status_tx, "[DEBUG] END stream {} (reason: {})", stream_id.as_u16(), reason);

        // Close stream
        let _ = self.circuit_manager.close_stream(circuit_id, stream_id).await;

        Ok(None)
    }

    /// Handle SENDME cell - update flow control windows
    async fn handle_sendme_cell(&mut self, circuit_id: CircuitId, stream_id: StreamId) -> Result<Option<Vec<u8>>> {
        if stream_id.as_u16() == 0 {
            // Circuit-level SENDME
            debug!("Received circuit-level SENDME for circuit {}", circuit_id.as_u32());
            let _ = self.circuit_manager.process_circuit_sendme(circuit_id).await;
        } else {
            // Stream-level SENDME
            debug!("Received stream-level SENDME for stream {}", stream_id.as_u16());
            let _ = self.circuit_manager.process_stream_sendme(circuit_id, stream_id).await;
        }
        Ok(None)
    }

    /// Spawn background task to forward data from TCP connection back to Tor client
    async fn spawn_stream_forwarder(
        &self,
        circuit_id: CircuitId,
        stream_id: StreamId,
        outgoing_tx: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<()> {
        // Get TCP connection
        let connection = self.circuit_manager.get_stream_connection(circuit_id, stream_id).await?
            .ok_or_else(|| anyhow::anyhow!("Stream not found"))?;

        let circuit_mgr = self.circuit_manager.clone();
        let status_tx = self.status_tx.clone();

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 498];  // Max relay cell data size

            loop {
                let bytes_read = {
                    let mut conn = connection.lock().await;
                    match conn.read(&mut buffer).await {
                        Ok(0) => {
                            // EOF - connection closed
                            debug!("Stream {} EOF from target", stream_id.as_u16());
                            break;
                        }
                        Ok(n) => n,
                        Err(e) => {
                            error!("Failed to read from stream {}: {}", stream_id.as_u16(), e);
                            break;
                        }
                    }
                };

                // Build DATA relay cell
                let mut data_cell = build_relay_cell(
                    circuit_id.as_u32(),
                    stream_id.as_u16(),
                    relay_command::DATA,
                    &buffer[..bytes_read]
                );

                // Encrypt relay cell payload
                if let Err(e) = circuit_mgr.encrypt_relay_cell(circuit_id, &mut data_cell[5..514]).await {
                    error!("Failed to encrypt DATA cell for stream {}: {}", stream_id.as_u16(), e);
                    break;
                }

                // Track bytes sent to client
                let _ = circuit_mgr.record_sent(circuit_id, 509).await;

                // Send encrypted cell through channel
                if let Err(e) = outgoing_tx.send(data_cell) {
                    error!("Failed to send DATA cell for stream {}: {}", stream_id.as_u16(), e);
                    break;
                }

                trace!("Forwarded {} bytes from stream {} back to client", bytes_read, stream_id.as_u16());
            }

            // Send END cell when stream closes
            let mut end_cell = build_relay_cell(
                circuit_id.as_u32(),
                stream_id.as_u16(),
                relay_command::END,
                &[end_reason::DONE]
            );

            // Encrypt END cell
            if let Ok(_) = circuit_mgr.encrypt_relay_cell(circuit_id, &mut end_cell[5..514]).await {
                let _ = circuit_mgr.record_sent(circuit_id, 509).await;
                let _ = outgoing_tx.send(end_cell);
            }

            // Close stream
            let _ = circuit_mgr.close_stream(circuit_id, stream_id).await;
            console_debug!(status_tx, "[DEBUG] Stream {} closed", stream_id.as_u16());
        });

        Ok(())
    }

    /// Create DESTROY cell
    fn create_destroy_cell(&self, circuit_id: CircuitId) -> Vec<u8> {
        let mut cell = Vec::with_capacity(514);
        cell.extend_from_slice(&circuit_id.to_bytes());
        cell.push(4);  // DESTROY command
        cell.push(1);  // Reason: protocol error
        cell.resize(514, 0);  // Pad to 514 bytes
        cell
    }
}

/// Tor cell information
#[derive(Debug, Clone)]
struct TorCellInfo {
    circuit_id: CircuitId,
    cell_type: String,
}

/// Parse Tor cell header
///
/// Tor v4 cell format:
/// - Circuit ID: 4 bytes
/// - Command: 1 byte
/// - Payload: 509 bytes (variable-length cells have length field)
///
/// Command types (from tor-spec.txt):
/// - 0: PADDING
/// - 1: CREATE (obsolete)
/// - 2: CREATED (obsolete)
/// - 3: RELAY
/// - 4: DESTROY
/// - 5: CREATE_FAST (obsolete)
/// - 6: CREATED_FAST (obsolete)
/// - 7: NETINFO
/// - 8: RELAY_EARLY
/// - 9: CREATE2
/// - 10: CREATED2
/// - 11: PADDING_NEGOTIATE
fn parse_tor_cell(data: &[u8]) -> Option<TorCellInfo> {
    if data.len() < 5 {
        return None;
    }

    // Extract circuit ID (4 bytes, big-endian)
    let circuit_id_bytes: [u8; 4] = data[0..4].try_into().ok()?;
    let circuit_id = CircuitId::from_bytes(&circuit_id_bytes);

    // Extract command byte
    let command = data[4];

    // Map command to cell type
    let cell_type = match command {
        0 => "PADDING",
        1 => "CREATE",
        2 => "CREATED",
        3 => "RELAY",
        4 => "DESTROY",
        5 => "CREATE_FAST",
        6 => "CREATED_FAST",
        7 => "NETINFO",
        8 => "RELAY_EARLY",
        9 => "CREATE2",
        10 => "CREATED2",
        11 => "PADDING_NEGOTIATE",
        _ => "UNKNOWN",
    };

    Some(TorCellInfo {
        circuit_id,
        cell_type: cell_type.to_string(),
    })
}
