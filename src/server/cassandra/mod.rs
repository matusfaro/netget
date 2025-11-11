//! Cassandra/CQL server implementation using cassandra-protocol
//!
//! Phase 1 (Minimal Viable):
//! - STARTUP → READY (no auth)
//! - OPTIONS → SUPPORTED
//! - QUERY → RESULT (LLM-generated rows)
//! - Single-stream operation (sequential processing)
//! - Protocol v4 only
//! - Basic types: int, varchar, boolean

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use actions::*;
use anyhow::{Context, Result};
use bytes::{Buf, BytesMut};
use cassandra_protocol::compression::Compression;
use cassandra_protocol::frame::{Direction, Envelope, Flags, Opcode, Version};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Cassandra server implementation
pub struct CassandraServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

/// Connection state for a Cassandra client
struct CassandraConnectionState {
    ready: bool,
    protocol_version: u8,
    /// Prepared statements: statement_id -> (query_string, param_count)
    prepared_statements: HashMap<Vec<u8>, (String, usize)>,
    /// Authentication state
    authenticated: bool,
    /// Authenticated username (if any)
    username: Option<String>,
}

impl CassandraServer {
    /// Create a new Cassandra server
    pub fn new(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Self {
        Self {
            llm_client,
            app_state,
            _status_tx: status_tx,
            server_id,
        }
    }

    /// Spawn Cassandra server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let actual_addr = listener.local_addr()?;

        console_info!(status_tx, "[INFO] Cassandra server listening on {}", actual_addr);

        let server = Arc::new(CassandraServer::new(
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            Some(server_id),
        ));

        // Spawn the accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        console_debug!(status_tx, "[DEBUG] Cassandra connection from {}", addr);

                        let server_clone = server.clone();
                        let status_tx_clone = status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = server_clone.handle_connection(stream, addr, status_tx_clone).await {
                                error!("Cassandra connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Accept failed: {}", e);
                    }
                }
            }
        });

        Ok(actual_addr)
    }

    /// Handle a single Cassandra connection
    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        addr: SocketAddr,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let connection_id = ConnectionId::new(self.app_state.get_next_unified_id().await);

        // Track the connection
        if let Some(server_id) = self.server_id {
            let now = Instant::now();
            let conn_state = ConnectionState {
                id: connection_id,
                remote_addr: addr,
                local_addr: stream.local_addr().unwrap_or(addr),
                bytes_sent: 0,
                bytes_received: 0,
                packets_sent: 0,
                packets_received: 0,
                last_activity: now,
                status: ConnectionStatus::Active,
                status_changed_at: now,
                protocol_info: ProtocolConnectionInfo::empty(),
            };

            self.app_state.add_connection_to_server(server_id, conn_state).await;
        }

        let mut conn_state = CassandraConnectionState {
            ready: false,
            protocol_version: 4,
            prepared_statements: HashMap::new(),
            authenticated: false,
            username: None,
        };

        let mut buffer = BytesMut::with_capacity(4096);

        loop {
            // Read data from stream
            let n = match stream.read_buf(&mut buffer).await {
                Ok(0) => {
                    console_debug!(status_tx, "[DEBUG] Cassandra client {} disconnected", addr);
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    error!("Read error from {}: {}", addr, e);
                    break;
                }
            };

            trace!("Read {} bytes from Cassandra client {}", n, addr);

            // Try to parse and handle frames
            while buffer.remaining() >= 9 {
                // Check if we have a complete frame header (9 bytes)
                let frame_start = buffer.as_ref();
                if frame_start.len() < 9 {
                    break;
                }

                // Read frame length from header (bytes 5-8)
                let length = u32::from_be_bytes([
                    frame_start[5],
                    frame_start[6],
                    frame_start[7],
                    frame_start[8],
                ]) as usize;

                // Check if we have the complete frame
                if buffer.remaining() < 9 + length {
                    trace!("Waiting for complete frame: have {}, need {}", buffer.remaining(), 9 + length);
                    break;
                }

                // We have a complete frame, parse it
                let frame_bytes = buffer.split_to(9 + length);

                match self
                    .handle_frame(
                        &frame_bytes,
                        &mut conn_state,
                        &mut stream,
                        connection_id,
                        &status_tx,
                    )
                    .await
                {
                    Ok(should_continue) => {
                        if !should_continue {
                            debug!("Closing Cassandra connection to {}", addr);
                            break;
                        }
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Frame error: {}", e);
                        // Send error frame and close connection
                        break;
                    }
                }
            }
        }

        // Close connection
        if let Some(server_id) = self.server_id {
            self.app_state
                .close_connection_on_server(server_id, connection_id)
                .await;
        }

        Ok(())
    }

    /// Handle a single Cassandra frame
    async fn handle_frame(
        &self,
        frame_bytes: &[u8],
        conn_state: &mut CassandraConnectionState,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        // Parse frame using cassandra-protocol
        let parsed = Envelope::from_buffer(frame_bytes, Compression::None)
            .context("Failed to parse Cassandra frame")?;
        let frame = parsed.envelope;

        console_trace!(status_tx, "[TRACE] Cassandra ← {:?}", frame.opcode);

        match frame.opcode {
            Opcode::Startup => {
                self.handle_startup(frame, conn_state, stream, connection_id, status_tx)
                    .await
            }
            Opcode::Options => {
                self.handle_options(frame, stream, connection_id, status_tx)
                    .await
            }
            Opcode::Query => {
                self.handle_query(frame, stream, connection_id, status_tx)
                    .await
            }
            Opcode::Prepare => {
                self.handle_prepare(frame, conn_state, stream, connection_id, status_tx)
                    .await
            }
            Opcode::Execute => {
                self.handle_execute(frame, conn_state, stream, connection_id, status_tx)
                    .await
            }
            Opcode::AuthResponse => {
                self.handle_auth_response(frame, conn_state, stream, connection_id, status_tx)
                    .await
            }
            _ => {
                console_warn!(status_tx, "[WARN] Unsupported opcode: {:?}", frame.opcode);
                // Send error response
                self.send_error(frame.stream_id, 0x000A, "Unsupported operation", stream, status_tx)
                    .await?;
                Ok(true)
            }
        }
    }

    /// Handle STARTUP frame
    async fn handle_startup(
        &self,
        frame: Envelope,
        conn_state: &mut CassandraConnectionState,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        debug!("Handling STARTUP from connection {}", connection_id);

        // Parse startup options (CQL_VERSION, etc.)
        // For Phase 1, we just accept any version and send READY

        // Call LLM to decide response
        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_STARTUP_EVENT,
            data: json!({
                "protocol_version": conn_state.protocol_version,
                "options": {"CQL_VERSION": "3.0.0"}
            }),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_ready" => {
                            conn_state.ready = true;
                            self.send_ready(frame.stream_id, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                _ => {
                    warn!("Unexpected action result for STARTUP");
                }
            }
        }

        // If no action was executed, send a default READY
        self.send_ready(frame.stream_id, stream, status_tx).await?;
        Ok(true)
    }

    /// Handle OPTIONS frame
    async fn handle_options(
        &self,
        frame: Envelope,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        debug!("Handling OPTIONS from connection {}", connection_id);

        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_OPTIONS_EVENT,
            data: json!({}),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_supported" => {
                            let options = data.get("options")
                                .and_then(|v| v.as_object())
                                .cloned()
                                .unwrap_or_default();
                            self.send_supported(frame.stream_id, options, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                _ => {
                    warn!("Unexpected action result for OPTIONS");
                }
            }
        }

        // If no action was executed, send default SUPPORTED
        self.send_supported(frame.stream_id, serde_json::Map::new(), stream, status_tx).await?;
        Ok(true)
    }

    /// Handle QUERY frame
    async fn handle_query(
        &self,
        frame: Envelope,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        // Parse query from frame body
        let query_str = self.parse_query(&frame)?;

        console_debug!(status_tx, "[DEBUG] Cassandra ← Query: {}", query_str);

        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_QUERY_EVENT,
            data: json!({
                "query": query_str,
                "consistency": "ONE"
            }),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_result_rows" => {
                            let columns = data.get("columns")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            let rows = data.get("rows")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            self.send_result_rows(frame.stream_id, columns, rows, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                ActionResult::CloseConnection => {
                    return Ok(false);
                }
                _ => {
                    warn!("Unexpected action result for QUERY");
                }
            }
        }

        // Default: send empty result
        self.send_result_rows(frame.stream_id, vec![], vec![], stream, status_tx).await?;
        Ok(true)
    }

    /// Parse query string from QUERY frame
    fn parse_query(&self, frame: &Envelope) -> Result<String> {
        // Frame body contains:
        // - query (long string)
        // - query parameters

        // For Phase 1, we do simple parsing
        // The body starts with a [long string] for the query
        let body = &frame.body;
        if body.len() < 4 {
            return Err(anyhow::anyhow!("Query frame too short"));
        }

        // Read long string length (4 bytes, big-endian)
        let query_len = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;

        if body.len() < 4 + query_len {
            return Err(anyhow::anyhow!("Query frame truncated"));
        }

        let query_bytes = &body[4..4 + query_len];
        let query_str = String::from_utf8_lossy(query_bytes).to_string();

        Ok(query_str)
    }

    /// Send READY response
    async fn send_ready(
        &self,
        stream_id: i16,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Ready,
            body: vec![],
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → READY");

        Ok(())
    }

    /// Send SUPPORTED response
    async fn send_supported(
        &self,
        stream_id: i16,
        options: serde_json::Map<String, serde_json::Value>,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Build SUPPORTED body: string multimap
        // For Phase 1, send minimal options
        let mut body = Vec::new();

        // Number of options (2 bytes)
        body.extend_from_slice(&(options.len() as u16).to_be_bytes());

        for (key, value) in options.iter() {
            // Key (string)
            let key_bytes = key.as_bytes();
            body.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
            body.extend_from_slice(key_bytes);

            // Value list (string list)
            if let Some(arr) = value.as_array() {
                body.extend_from_slice(&(arr.len() as u16).to_be_bytes());
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let s_bytes = s.as_bytes();
                        body.extend_from_slice(&(s_bytes.len() as u16).to_be_bytes());
                        body.extend_from_slice(s_bytes);
                    }
                }
            } else {
                // Empty list
                body.extend_from_slice(&0u16.to_be_bytes());
            }
        }

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Supported,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → SUPPORTED");

        Ok(())
    }

    /// Send RESULT with rows
    async fn send_result_rows(
        &self,
        stream_id: i16,
        columns: Vec<serde_json::Value>,
        rows: Vec<serde_json::Value>,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Build RESULT body (kind=ROWS)
        // This is complex in Cassandra protocol - for Phase 1, we send a simplified response
        let mut body = Vec::new();

        // Result kind (4 bytes): 0x0002 = Rows
        body.extend_from_slice(&0x00000002u32.to_be_bytes());

        // Metadata
        // Flags (4 bytes): 0x0001 = GlobalTablesSpec
        body.extend_from_slice(&0x00000001u32.to_be_bytes());

        // Column count (4 bytes)
        body.extend_from_slice(&(columns.len() as u32).to_be_bytes());

        // Global keyspace and table (for simplicity)
        let keyspace = b"system";
        let table = b"local";
        body.extend_from_slice(&(keyspace.len() as u16).to_be_bytes());
        body.extend_from_slice(keyspace);
        body.extend_from_slice(&(table.len() as u16).to_be_bytes());
        body.extend_from_slice(table);

        // Column specs
        for col in &columns {
            let name = col.get("name").and_then(|v| v.as_str()).unwrap_or("col");
            let col_type = col.get("type").and_then(|v| v.as_str()).unwrap_or("varchar");

            // Column name
            body.extend_from_slice(&(name.len() as u16).to_be_bytes());
            body.extend_from_slice(name.as_bytes());

            // Column type (simplified: 0x000D = varchar, 0x0009 = int)
            let type_code: u16 = match col_type {
                "int" => 0x0009,
                "boolean" => 0x0004,
                _ => 0x000D, // varchar
            };
            body.extend_from_slice(&type_code.to_be_bytes());
        }

        // Rows count (4 bytes)
        body.extend_from_slice(&(rows.len() as u32).to_be_bytes());

        // Row data
        for row in &rows {
            if let Some(row_arr) = row.as_array() {
                for (i, cell) in row_arr.iter().enumerate() {
                    // Cell value (bytes)
                    let col_type = if i < columns.len() {
                        columns[i].get("type").and_then(|v| v.as_str())
                    } else {
                        None
                    };
                    let cell_bytes = self.serialize_cell_value(cell, col_type);

                    if let Some(bytes) = cell_bytes {
                        body.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
                        body.extend_from_slice(&bytes);
                    } else {
                        // NULL value (-1)
                        body.extend_from_slice(&(-1i32).to_be_bytes());
                    }
                }
            }
        }

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Result,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → RESULT ({} rows)", rows.len());

        Ok(())
    }

    /// Serialize a cell value based on its type
    fn serialize_cell_value(&self, value: &serde_json::Value, _col_type: Option<&str>) -> Option<Vec<u8>> {
        match value {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s.as_bytes().to_vec()),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some((i as i32).to_be_bytes().to_vec())
                } else {
                    Some(n.to_string().as_bytes().to_vec())
                }
            }
            serde_json::Value::Bool(b) => {
                Some(vec![if *b { 1 } else { 0 }])
            }
            _ => Some(value.to_string().as_bytes().to_vec()),
        }
    }

    /// Handle PREPARE frame
    async fn handle_prepare(
        &self,
        frame: Envelope,
        conn_state: &mut CassandraConnectionState,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        debug!("Handling PREPARE from connection {}", connection_id);

        // Parse query from frame
        let query = self.parse_query(&frame)?;
        trace!("PREPARE query: {}", query);

        // Generate statement ID from query hash
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
        let mut hasher = DefaultHasher::new();
        query.hash(&mut hasher);
        let hash = hasher.finish();
        let statement_id = hash.to_be_bytes().to_vec();

        // Count parameters in query (simple heuristic: count '?' occurrences)
        let param_count = query.matches('?').count();

        // Store prepared statement
        conn_state.prepared_statements.insert(
            statement_id.clone(),
            (query.clone(), param_count),
        );

        debug!("Prepared statement ID {:?} with {} params", statement_id, param_count);

        // Call LLM to decide response
        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_PREPARE_EVENT,
            data: json!({
                "query": query,
                "statement_id": hex::encode(&statement_id),
                "param_count": param_count,
            }),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_prepared" => {
                            let columns = data.get("columns")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            self.send_prepared(frame.stream_id, statement_id, columns, param_count, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                ActionResult::CloseConnection => {
                    return Ok(false);
                }
                _ => {
                    warn!("Unexpected action result for PREPARE");
                }
            }
        }

        // Default: send empty prepared result
        self.send_prepared(frame.stream_id, statement_id, vec![], param_count, stream, status_tx).await?;
        Ok(true)
    }

    /// Handle EXECUTE frame
    async fn handle_execute(
        &self,
        frame: Envelope,
        conn_state: &mut CassandraConnectionState,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        debug!("Handling EXECUTE from connection {}", connection_id);

        // Parse statement ID and parameters from frame
        let (statement_id, params) = self.parse_execute(&frame)?;

        // Look up prepared statement
        let (query, expected_param_count) = conn_state
            .prepared_statements
            .get(&statement_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown prepared statement ID"))?
            .clone();

        trace!("EXECUTE statement: {} with {} params", query, params.len());

        // Validate parameter count
        if params.len() != expected_param_count {
            let err_msg = format!(
                "Expected {} parameters, got {}",
                expected_param_count,
                params.len()
            );
            self.send_error(frame.stream_id, 0x2200, &err_msg, stream, status_tx).await?;
            return Ok(true);
        }

        // Call LLM with query and bound parameters
        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_EXECUTE_EVENT,
            data: json!({
                "query": query,
                "statement_id": hex::encode(&statement_id),
                "parameters": params,
            }),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_result_rows" => {
                            let columns = data.get("columns")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            let rows = data.get("rows")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            self.send_result_rows(frame.stream_id, columns, rows, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                ActionResult::CloseConnection => {
                    return Ok(false);
                }
                _ => {
                    warn!("Unexpected action result for EXECUTE");
                }
            }
        }

        // Default: send empty result
        self.send_result_rows(frame.stream_id, vec![], vec![], stream, status_tx).await?;
        Ok(true)
    }

    /// Parse statement ID and parameters from EXECUTE frame
    fn parse_execute(&self, frame: &Envelope) -> Result<(Vec<u8>, Vec<serde_json::Value>)> {
        let body = &frame.body;
        if body.len() < 2 {
            return Err(anyhow::anyhow!("EXECUTE frame too short"));
        }

        // Read statement ID (short bytes)
        let id_len = u16::from_be_bytes([body[0], body[1]]) as usize;
        if body.len() < 2 + id_len {
            return Err(anyhow::anyhow!("EXECUTE frame truncated (statement ID)"));
        }

        let statement_id = body[2..2 + id_len].to_vec();
        let mut offset = 2 + id_len;

        // Parse query parameters (Phase 2: basic types only)
        // Skip consistency level (2 bytes)
        if body.len() < offset + 2 {
            return Err(anyhow::anyhow!("EXECUTE frame truncated (consistency)"));
        }
        offset += 2;

        // Skip flags (1 byte)
        if body.len() < offset + 1 {
            return Err(anyhow::anyhow!("EXECUTE frame truncated (flags)"));
        }
        let flags = body[offset];
        offset += 1;

        let mut params = Vec::new();

        // If VALUES flag is set, parse parameter values
        if flags & 0x01 != 0 {
            // Read parameter count (2 bytes)
            if body.len() < offset + 2 {
                return Err(anyhow::anyhow!("EXECUTE frame truncated (param count)"));
            }
            let param_count = u16::from_be_bytes([body[offset], body[offset + 1]]) as usize;
            offset += 2;

            // Parse each parameter (bytes or null)
            for _ in 0..param_count {
                if body.len() < offset + 4 {
                    return Err(anyhow::anyhow!("EXECUTE frame truncated (param length)"));
                }

                let param_len = i32::from_be_bytes([
                    body[offset],
                    body[offset + 1],
                    body[offset + 2],
                    body[offset + 3],
                ]);
                offset += 4;

                if param_len < 0 {
                    // Null value
                    params.push(serde_json::Value::Null);
                } else {
                    let param_len = param_len as usize;
                    if body.len() < offset + param_len {
                        return Err(anyhow::anyhow!("EXECUTE frame truncated (param data)"));
                    }

                    let param_bytes = &body[offset..offset + param_len];
                    // For Phase 2, treat all params as strings
                    let param_str = String::from_utf8_lossy(param_bytes).to_string();
                    params.push(json!(param_str));
                    offset += param_len;
                }
            }
        }

        Ok((statement_id, params))
    }

    /// Send RESULT (Prepared) response
    async fn send_prepared(
        &self,
        stream_id: i16,
        statement_id: Vec<u8>,
        columns: Vec<serde_json::Value>,
        param_count: usize,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut body = Vec::new();

        // Result kind: Prepared (0x0004)
        body.extend_from_slice(&0x00000004u32.to_be_bytes());

        // Statement ID (short bytes)
        body.extend_from_slice(&(statement_id.len() as u16).to_be_bytes());
        body.extend_from_slice(&statement_id);

        // Metadata for result set (what the query will return)
        // Flags: 0x0001 (GLOBAL_TABLES_SPEC)
        body.extend_from_slice(&0x00000001u32.to_be_bytes());

        // Column count
        body.extend_from_slice(&(columns.len() as u32).to_be_bytes());

        // Global keyspace and table (Phase 2: use placeholder)
        let keyspace = b"netget";
        body.extend_from_slice(&(keyspace.len() as u16).to_be_bytes());
        body.extend_from_slice(keyspace);

        let table = b"data";
        body.extend_from_slice(&(table.len() as u16).to_be_bytes());
        body.extend_from_slice(table);

        // Column specifications
        for col in columns {
            let name = col.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("col");
            let col_type = col.get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("varchar");

            // Column name
            let name_bytes = name.as_bytes();
            body.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
            body.extend_from_slice(name_bytes);

            // Column type (simple types only for Phase 2)
            let type_code: u16 = match col_type {
                "int" => 0x0009,
                "varchar" | "text" => 0x000D,
                "boolean" => 0x0004,
                _ => 0x000D, // Default to varchar
            };
            body.extend_from_slice(&type_code.to_be_bytes());
        }

        // Metadata for bound variables (parameters)
        // Flags: 0x0001 (GLOBAL_TABLES_SPEC)
        body.extend_from_slice(&0x00000001u32.to_be_bytes());

        // Parameter count
        body.extend_from_slice(&(param_count as u32).to_be_bytes());

        // Global keyspace and table for parameters
        body.extend_from_slice(&(keyspace.len() as u16).to_be_bytes());
        body.extend_from_slice(keyspace);
        body.extend_from_slice(&(table.len() as u16).to_be_bytes());
        body.extend_from_slice(table);

        // Parameter specifications (Phase 2: all varchar)
        for i in 0..param_count {
            let param_name = format!("param{}", i);
            let param_bytes = param_name.as_bytes();
            body.extend_from_slice(&(param_bytes.len() as u16).to_be_bytes());
            body.extend_from_slice(param_bytes);

            // Type: varchar (0x000D)
            body.extend_from_slice(&0x000Du16.to_be_bytes());
        }

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Result,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → RESULT (Prepared: {} params)");

        Ok(())
    }

    /// Send ERROR response
    async fn send_error(
        &self,
        stream_id: i16,
        error_code: u32,
        message: &str,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut body = Vec::new();

        // Error code (4 bytes)
        body.extend_from_slice(&error_code.to_be_bytes());

        // Error message (string)
        let msg_bytes = message.as_bytes();
        body.extend_from_slice(&(msg_bytes.len() as u16).to_be_bytes());
        body.extend_from_slice(msg_bytes);

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Error,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → ERROR 0x{:04X}", error_code);

        Ok(())
    }

    /// Handle AUTH_RESPONSE frame (Phase 3)
    async fn handle_auth_response(
        &self,
        frame: Envelope,
        conn_state: &mut CassandraConnectionState,
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<bool> {
        debug!("Handling AUTH_RESPONSE from connection {}", connection_id);

        // Parse credentials from frame body (SASL PLAIN format: \0username\0password)
        let body = &frame.body;

        // Extract username and password from SASL PLAIN format
        let (username, password) = self.parse_sasl_plain(body)?;

        trace!("AUTH_RESPONSE: username={}", username);

        // Call LLM to decide whether to accept authentication
        let protocol = CassandraProtocol::new(
            connection_id,
            self.app_state.clone(),
            status_tx.clone(),
        );

        let event = Event {
            event_type: &CASSANDRA_AUTH_EVENT,
            data: json!({
                "username": username,
                "password": password,
            }),
        };

        let server_id = self.server_id.context("Server ID not set")?;

        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await?;

        // Show messages
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        // Execute the protocol actions
        for action_result in execution_result.protocol_results {
            match action_result {
                ActionResult::Custom { name, data } => {
                    match name.as_str() {
                        "cassandra_auth_success" => {
                            conn_state.authenticated = true;
                            conn_state.username = Some(username);
                            self.send_auth_success(frame.stream_id, stream, status_tx).await?;
                            return Ok(true);
                        }
                        "cassandra_error" => {
                            let error_code = data.get("error_code")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0x0000) as u32;
                            let message = data.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            self.send_error(frame.stream_id, error_code, message, stream, status_tx).await?;
                            return Ok(false);  // Close connection on auth failure
                        }
                        _ => {}
                    }
                }
                ActionResult::CloseConnection => {
                    return Ok(false);
                }
                _ => {
                    warn!("Unexpected action result for AUTH_RESPONSE");
                }
            }
        }

        // Default: deny authentication
        self.send_error(frame.stream_id, 0x0100, "Authentication failed", stream, status_tx).await?;
        Ok(false)
    }

    /// Parse SASL PLAIN credentials (format: \0username\0password)
    fn parse_sasl_plain(&self, body: &[u8]) -> Result<(String, String)> {
        if body.is_empty() {
            return Err(anyhow::anyhow!("Empty AUTH_RESPONSE body"));
        }

        // Skip optional authorization identity (first \0-terminated string)
        let mut idx = 0;
        while idx < body.len() && body[idx] != 0 {
            idx += 1;
        }
        idx += 1; // Skip the \0

        if idx >= body.len() {
            return Err(anyhow::anyhow!("Invalid SASL PLAIN format"));
        }

        // Extract username
        let username_start = idx;
        while idx < body.len() && body[idx] != 0 {
            idx += 1;
        }
        let username = String::from_utf8_lossy(&body[username_start..idx]).to_string();
        idx += 1; // Skip the \0

        if idx >= body.len() {
            return Err(anyhow::anyhow!("Invalid SASL PLAIN format - missing password"));
        }

        // Extract password
        let password = String::from_utf8_lossy(&body[idx..]).to_string();

        Ok((username, password))
    }

    /// Send AUTHENTICATE response (request authentication)
    async fn _send_authenticate(
        &self,
        stream_id: i16,
        authenticator: &str,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut body = Vec::new();

        // Authenticator name (string)
        let auth_bytes = authenticator.as_bytes();
        body.extend_from_slice(&(auth_bytes.len() as u16).to_be_bytes());
        body.extend_from_slice(auth_bytes);

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::Authenticate,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → AUTHENTICATE ({})", authenticator);

        Ok(())
    }

    /// Send AUTH_SUCCESS response
    async fn send_auth_success(
        &self,
        stream_id: i16,
        stream: &mut TcpStream,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // AUTH_SUCCESS body can contain optional token (we send empty for SASL PLAIN)
        let body = vec![];

        let response = Envelope {
            version: Version::V4,
            direction: Direction::Response,
            flags: Flags::empty(),
            stream_id: stream_id,
            opcode: Opcode::AuthSuccess,
            body,
            tracing_id: None,
            warnings: vec![],
        };

        let bytes = response.encode_with(Compression::None)?;
        stream.write_all(&bytes).await?;

        console_trace!(status_tx, "[TRACE] Cassandra → AUTH_SUCCESS");

        Ok(())
    }
}
