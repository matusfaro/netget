//! MSSQL server implementation using manual TDS protocol
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error};
use actions::{MssqlProtocol, MSSQL_QUERY_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// MSSQL server implementation
pub struct MssqlServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl MssqlServer {
    /// Create a new MSSQL server
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

    /// Spawn MSSQL server with LLM integration
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

        info!("MSSQL server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] MSSQL server listening on {}", actual_addr));

        let server = Arc::new(MssqlServer::new(
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            Some(server_id),
        ));

        let status_tx_clone = status_tx.clone();

        // Spawn the accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        console_debug!(status_tx, "MSSQL connection from {}", addr);

                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(actual_addr);

                        // Track the connection
                        if let Some(server_id) = server.server_id {
                            use crate::state::server::{
                                ConnectionState as ServerConnectionState, ConnectionStatus,
                                ProtocolConnectionInfo,
                            };
                            let now = std::time::Instant::now();
                            let conn_state = ServerConnectionState {
                                id: connection_id,
                                remote_addr: addr,
                                local_addr: local_addr_conn,
                                bytes_sent: 0,
                                bytes_received: 0,
                                packets_sent: 0,
                                packets_received: 0,
                                last_activity: now,
                                status: ConnectionStatus::Active,
                                status_changed_at: now,
                                protocol_info: ProtocolConnectionInfo::empty(),
                            };
                            server
                                .app_state
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                        }

                        let handler = MssqlHandler::new(
                            connection_id,
                            server.llm_client.clone(),
                            server.app_state.clone(),
                            status_tx.clone(),
                            server.server_id,
                            addr,
                        );

                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_connection(stream).await {
                                error!("MSSQL connection error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "MSSQL accept error: {}", e);
                    }
                }
            }
        });

        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        Ok(actual_addr)
    }
}

/// MSSQL connection handler
pub struct MssqlHandler {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    server_id: Option<crate::state::ServerId>,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    /// MSSQL protocol handler for action execution
    protocol: Arc<MssqlProtocol>,
}

impl MssqlHandler {
    pub fn new(
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
        remote_addr: SocketAddr,
    ) -> Self {
        let protocol = Arc::new(MssqlProtocol::new(
            connection_id,
            app_state.clone(),
            status_tx.clone(),
        ));

        Self {
            connection_id,
            llm_client,
            app_state,
            status_tx,
            server_id,
            remote_addr,
            protocol,
        }
    }

    /// Handle a single MSSQL connection
    async fn handle_connection(self, mut stream: TcpStream) -> Result<()> {
        info!("MSSQL connection established");

        // Handle TDS protocol negotiation and queries
        loop {
            // Read TDS packet header (8 bytes)
            let header = match self.read_tds_header(&mut stream).await {
                Ok(h) => h,
                Err(e) => {
                    debug!("Error reading TDS header: {}", e);
                    break;
                }
            };

            if header.length < 8 {
                debug!("Invalid TDS packet length: {}", header.length);
                break;
            }

            // Read packet data
            let data_len = header.length - 8;
            let mut data = vec![0u8; data_len as usize];
            stream.read_exact(&mut data).await?;

            trace!("TDS packet type: 0x{:02x}, length: {}", header.packet_type, header.length);

            match header.packet_type {
                0x12 => {
                    // Pre-Login
                    debug!("Received Pre-Login packet");
                    self.send_prelogin_response(&mut stream).await?;
                }
                0x10 => {
                    // TDS7/TDS8 Login
                    debug!("Received Login packet");
                    self.send_login_response(&mut stream).await?;
                }
                0x01 => {
                    // SQL Batch
                    debug!("Received SQL Batch packet");
                    let query = self.parse_sql_batch(&data)?;
                    debug!("SQL Query: {}", query);
                    self.handle_query(&mut stream, &query).await?;
                }
                0x03 => {
                    // RPC Request (sp_executesql, sp_prepare, etc.)
                    debug!("Received RPC Request");
                    let query = self.parse_rpc_request(&data)?;
                    if !query.is_empty() {
                        debug!("RPC Query: {}", query);
                        self.handle_query(&mut stream, &query).await?;
                    } else {
                        debug!("RPC call without extractable query (ignoring)");
                        // Send empty result set for RPCs we can't parse
                        self.send_empty_result(&mut stream).await?;
                    }
                }
                0x0E => {
                    // Bulk Load
                    debug!("Received Bulk Load (not implemented)");
                    self.send_error(&mut stream, 40002, "Bulk load not supported", 16).await?;
                }
                0x07 => {
                    // Attention (cancel)
                    debug!("Received Attention signal");
                    break;
                }
                _ => {
                    debug!("Unknown TDS packet type: 0x{:02x}", header.packet_type);
                    self.send_error(&mut stream, 40002, "Unknown packet type", 16).await?;
                }
            }
        }

        Ok(())
    }

    /// Read TDS packet header (8 bytes)
    async fn read_tds_header(&self, stream: &mut TcpStream) -> Result<TdsHeader> {
        let mut header_bytes = [0u8; 8];
        stream.read_exact(&mut header_bytes).await?;

        Ok(TdsHeader {
            packet_type: header_bytes[0],
            status: header_bytes[1],
            length: u16::from_be_bytes([header_bytes[2], header_bytes[3]]),
            spid: u16::from_be_bytes([header_bytes[4], header_bytes[5]]),
            packet_id: header_bytes[6],
            window: header_bytes[7],
        })
    }

    /// Send Pre-Login response
    async fn send_prelogin_response(&self, stream: &mut TcpStream) -> Result<()> {
        // Simplified Pre-Login response
        // Version: 16.0.0.0 (SQL Server 2022)
        // Encryption: NOT_SUP (0x02)
        let mut response = Vec::new();

        // Calculate offsets (all token headers = 3 tokens * 5 bytes + 1 terminator = 16 bytes)
        let header_size = 16u16;
        let version_offset = header_size; // 16 (0x10)
        let version_length = 6u16;
        let encryption_offset = version_offset + version_length; // 22 (0x16)
        let encryption_length = 1u16;
        let threadid_offset = encryption_offset + encryption_length; // 23 (0x17)
        let threadid_length = 4u16;

        // Version token (0x00)
        response.push(0x00);
        response.extend_from_slice(&version_offset.to_be_bytes()); // Offset: 0x00, 0x10
        response.extend_from_slice(&version_length.to_be_bytes()); // Length: 0x00, 0x06

        // Encryption token (0x01)
        response.push(0x01);
        response.extend_from_slice(&encryption_offset.to_be_bytes()); // Offset: 0x00, 0x16
        response.extend_from_slice(&encryption_length.to_be_bytes()); // Length: 0x00, 0x01

        // ThreadID token (0x03)
        response.push(0x03);
        response.extend_from_slice(&threadid_offset.to_be_bytes()); // Offset: 0x00, 0x17
        response.extend_from_slice(&threadid_length.to_be_bytes()); // Length: 0x00, 0x04

        // Terminator
        response.push(0xFF);

        // Version data (16.0.0.0)
        response.extend_from_slice(&[0x10, 0x00, 0x00, 0x00, 0x00, 0x00]);

        // Encryption: ENCRYPT_NOT_SUP (0x02) - encryption not supported
        response.push(0x02);

        // ThreadID: 0
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Send Login response (accept all logins)
    async fn send_login_response(&self, stream: &mut TcpStream) -> Result<()> {
        let _ = self.status_tx.send("[DEBUG] MSSQL → Login accepted".to_string());

        let mut response = Vec::new();

        // ENVCHANGE: Database context
        let db_name = "master";
        let db_name_utf16: Vec<u8> = db_name.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let mut envchange_db = Vec::new();
        envchange_db.push(0xE3); // ENVCHANGE token
        let envchange_db_len = 1 + 1 + db_name_utf16.len() + 1 + db_name_utf16.len();
        envchange_db.extend_from_slice(&(envchange_db_len as u16).to_le_bytes());
        envchange_db.push(0x01); // Type: Database
        envchange_db.push(db_name.len() as u8); // New value length (in characters)
        envchange_db.extend_from_slice(&db_name_utf16);
        envchange_db.push(db_name.len() as u8); // Old value length (in characters)
        envchange_db.extend_from_slice(&db_name_utf16);
        response.extend_from_slice(&envchange_db);

        // ENVCHANGE: Language (us_english)
        let lang = "us_english";
        let lang_utf16: Vec<u8> = lang.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let mut envchange_lang = Vec::new();
        envchange_lang.push(0xE3); // ENVCHANGE token
        let envchange_lang_len = 1 + 1 + lang_utf16.len() + 1 + 0; // old value is empty
        envchange_lang.extend_from_slice(&(envchange_lang_len as u16).to_le_bytes());
        envchange_lang.push(0x02); // Type: Language
        envchange_lang.push(lang.len() as u8); // New value length (in characters)
        envchange_lang.extend_from_slice(&lang_utf16);
        envchange_lang.push(0x00); // Old value length (empty)
        response.extend_from_slice(&envchange_lang);

        // ENVCHANGE: Packet size ("4096" as string)
        let pkt_size_new = "4096";
        let pkt_size_old = "512"; // Default packet size
        let pkt_size_new_utf16: Vec<u8> = pkt_size_new.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let pkt_size_old_utf16: Vec<u8> = pkt_size_old.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let mut envchange_pkt = Vec::new();
        envchange_pkt.push(0xE3); // ENVCHANGE token
        let envchange_pkt_len = 1 + 1 + pkt_size_new_utf16.len() + 1 + pkt_size_old_utf16.len();
        envchange_pkt.extend_from_slice(&(envchange_pkt_len as u16).to_le_bytes());
        envchange_pkt.push(0x04); // Type: Packet size
        envchange_pkt.push(pkt_size_new.len() as u8); // New value length (in characters)
        envchange_pkt.extend_from_slice(&pkt_size_new_utf16);
        envchange_pkt.push(pkt_size_old.len() as u8); // Old value length (in characters)
        envchange_pkt.extend_from_slice(&pkt_size_old_utf16);
        response.extend_from_slice(&envchange_pkt);

        // INFO message
        let msg = "Login succeeded";
        let msg_utf16: Vec<u8> = msg.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        response.push(0xAB); // INFO token
        let info_len = 4 + 1 + 1 + 2 + msg_utf16.len() + 1 + 1 + 4;
        response.extend_from_slice(&(info_len as u16).to_le_bytes());
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Error number
        response.push(0x01); // State
        response.push(0x00); // Class (severity)
        response.extend_from_slice(&(msg.len() as u16).to_le_bytes()); // Message length (character count)
        response.extend_from_slice(&msg_utf16);
        response.push(0x00); // Server name length
        response.push(0x00); // Procedure name length
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Line number

        // DONE token
        response.push(0xFD); // DONE token
        response.extend_from_slice(&[0x00, 0x00]); // Status
        response.extend_from_slice(&[0x00, 0x00]); // CurCmd
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // DoneRowCount

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Parse SQL Batch packet
    fn parse_sql_batch(&self, data: &[u8]) -> Result<String> {
        // SQL Batch format:
        // - Header (22 bytes for TDS 7.4+)
        // - SQL text (Unicode UTF-16LE)

        if data.len() < 22 {
            return Ok(String::new());
        }

        // Skip header, extract SQL text
        let sql_bytes = &data[22..];

        // Decode UTF-16LE
        let sql_u16: Vec<u16> = sql_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        Ok(String::from_utf16_lossy(&sql_u16).trim().to_string())
    }

    /// Parse RPC Request packet to extract SQL query
    fn parse_rpc_request(&self, data: &[u8]) -> Result<String> {
        // RPC format is complex - we'll try to extract any UTF-16 strings that look like SQL
        // Most RPC calls are sp_executesql with the SQL as first parameter

        if data.len() < 10 {
            return Ok(String::new());
        }

        // Debug: log first 200 bytes as hex
        let preview_len = std::cmp::min(data.len(), 200);
        let hex_preview: String = data[..preview_len]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        debug!("RPC data preview (first {} bytes): {}", preview_len, hex_preview);

        // Try to find UTF-16 encoded SQL in the RPC data
        // Look for common SQL keywords as markers
        for start in (0..data.len().saturating_sub(10)).step_by(2) {
            // Try to decode as UTF-16LE
            let chunk_len = std::cmp::min(data.len() - start, 2000);
            if chunk_len < 10 {
                break; // Not enough data left
            }
            let chunk = &data[start..start + chunk_len];

            if chunk.len() % 2 != 0 {
                continue;
            }

            let text_u16: Vec<u16> = chunk
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();

            let text = String::from_utf16_lossy(&text_u16);

            // Check if this looks like SQL (contains SELECT, INSERT, UPDATE, DELETE, CREATE, etc.)
            let text_upper = text.to_uppercase();
            if text_upper.contains("SELECT ") ||
               text_upper.contains("SELECT") || // Also check without space
               text_upper.contains("INSERT ") ||
               text_upper.contains("UPDATE ") ||
               text_upper.contains("DELETE ") ||
               text_upper.contains("CREATE ") ||
               text_upper.contains("DROP ") ||
               text_upper.contains("ALTER ") {
                // Found SQL - extract it by finding the SQL keyword and taking everything until null or non-printable chars
                let sql_start_keywords = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER"];
                for keyword in &sql_start_keywords {
                    if let Some(pos) = text_upper.find(keyword) {
                        let sql_part = &text[pos..];
                        // Take only printable ASCII characters (SQL queries should be ASCII)
                        let sql: String = sql_part
                            .chars()
                            .take_while(|c| c.is_ascii() && (*c == '\n' || *c == '\r' || *c == '\t' || !c.is_ascii_control()))
                            .collect();
                        let sql = sql.trim().to_string();
                        if !sql.is_empty() && sql.len() >= keyword.len() {
                            debug!("Extracted SQL from RPC at offset {}: {}", start, sql);
                            return Ok(sql);
                        }
                    }
                }
            }
        }

        Ok(String::new())
    }

    /// Handle SQL query with LLM
    async fn handle_query(&self, stream: &mut TcpStream, query: &str) -> Result<()> {
        trace!("Calling LLM for MSSQL query: {}", query);

        // Create query event
        let event = Event::new(
            &MSSQL_QUERY_EVENT,
            serde_json::json!({
                "query": query,
            }),
        );

        let server_id = self
            .server_id
            .unwrap_or_else(|| crate::state::ServerId::new(0));

        let llm_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await;

        match llm_result {
            Ok(execution_result) => {
                // Process action results to find MSSQL responses
                for result in execution_result.protocol_results {
                    match result {
                        ActionResult::Custom { name, data } => {
                            match name.as_str() {
                                "mssql_query_response" => {
                                    let columns = data
                                        .get("columns")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();
                                    let rows = data
                                        .get("rows")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();

                                    return self.send_result_set(stream, columns, rows).await;
                                }
                                "mssql_error" => {
                                    let error_number = data
                                        .get("error_number")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(50000) as u32;
                                    let message = data
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown error");
                                    let severity = data
                                        .get("severity")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(16) as u8;

                                    return self.send_error(stream, error_number, message, severity).await;
                                }
                                "mssql_ok" => {
                                    let rows_affected = data
                                        .get("rows_affected")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);

                                    return self.send_done(stream, rows_affected).await;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }

                // No MSSQL-specific response found, return empty done
                self.send_done(stream, 0).await
            }
            Err(e) => {
                error!("LLM error for MSSQL query: {}", e);
                self.send_error(stream, 50000, &format!("LLM error: {}", e), 16).await
            }
        }
    }

    /// Send result set
    async fn send_result_set(
        &self,
        stream: &mut TcpStream,
        columns: Vec<serde_json::Value>,
        rows: Vec<serde_json::Value>,
    ) -> Result<()> {
        let mut response = Vec::new();

        // COLMETADATA token
        response.push(0x81);
        response.extend_from_slice(&(columns.len() as u16).to_le_bytes());

        for col in &columns {
            let col_name = col.get("name").and_then(|v| v.as_str()).unwrap_or("column");
            let col_type = col.get("type").and_then(|v| v.as_str()).unwrap_or("NVARCHAR");

            // Column definition (simplified - NVARCHAR for all types)
            response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // UserType
            response.extend_from_slice(&[0x00, 0x00]); // Flags
            response.push(get_tds_type(col_type)); // Type
            response.extend_from_slice(&[0xFF, 0xFF]); // Max length
            response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]); // Collation

            // Column name
            response.push(col_name.len() as u8);
            response.extend_from_slice(col_name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>().as_slice());
        }

        // ROW tokens
        for row in &rows {
            response.push(0xD1); // ROW token

            if let Some(row_values) = row.as_array() {
                for value in row_values {
                    let value_str = json_to_string(value);
                    let value_u16: Vec<u16> = value_str.encode_utf16().collect();
                    let value_bytes: Vec<u8> = value_u16.iter().flat_map(|c| c.to_le_bytes()).collect();

                    // Length prefix (2 bytes for NVARCHAR)
                    response.extend_from_slice(&(value_bytes.len() as u16).to_le_bytes());
                    response.extend_from_slice(&value_bytes);
                }
            }
        }

        // DONE token
        response.push(0xFD);
        response.extend_from_slice(&[0x00, 0x00]); // Status: final
        response.extend_from_slice(&[0xC1, 0x00]); // CurCmd
        response.extend_from_slice(&(rows.len() as u64).to_le_bytes());

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Send empty result set (just DONE token)
    async fn send_empty_result(&self, stream: &mut TcpStream) -> Result<()> {
        let mut response = Vec::new();

        // DONE token
        response.push(0xFD);
        response.extend_from_slice(&[0x00, 0x00]); // Status
        response.extend_from_slice(&[0x00, 0x00]); // CurCmd
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // DoneRowCount

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Send error response
    async fn send_error(&self, stream: &mut TcpStream, error_number: u32, message: &str, severity: u8) -> Result<()> {
        let mut response = Vec::new();

        // ERROR token (0xAA)
        response.push(0xAA);

        let msg_u16: Vec<u16> = message.encode_utf16().collect();
        let msg_bytes: Vec<u8> = msg_u16.iter().flat_map(|c| c.to_le_bytes()).collect();

        let token_len = 4 + 1 + 1 + 2 + msg_bytes.len() + 1 + 1 + 4;
        response.extend_from_slice(&(token_len as u16).to_le_bytes());

        response.extend_from_slice(&error_number.to_le_bytes());
        response.push(0x01); // State
        response.push(severity); // Class (severity)
        response.extend_from_slice(&(msg_u16.len() as u16).to_le_bytes());
        response.extend_from_slice(&msg_bytes);
        response.push(0x00); // Server name length
        response.push(0x00); // Procedure name length
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Line number

        // DONE token
        response.push(0xFD);
        response.extend_from_slice(&[0x00, 0x00]); // Status
        response.extend_from_slice(&[0x00, 0x00]); // CurCmd
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Send DONE token
    async fn send_done(&self, stream: &mut TcpStream, rows_affected: u64) -> Result<()> {
        let mut response = Vec::new();

        response.push(0xFD); // DONE token
        response.extend_from_slice(&[0x00, 0x00]); // Status: final
        response.extend_from_slice(&[0xC1, 0x00]); // CurCmd
        response.extend_from_slice(&rows_affected.to_le_bytes());

        self.send_tds_packet(stream, 0x04, &response).await
    }

    /// Send TDS packet with header
    async fn send_tds_packet(&self, stream: &mut TcpStream, packet_type: u8, data: &[u8]) -> Result<()> {
        let total_len = 8 + data.len();
        let mut packet = Vec::with_capacity(total_len);

        // TDS header
        packet.push(packet_type); // Type
        packet.push(0x01); // Status (EOM)
        packet.extend_from_slice(&(total_len as u16).to_be_bytes()); // Length
        packet.extend_from_slice(&[0x00, 0x00]); // SPID
        packet.push(0x01); // PacketID
        packet.push(0x00); // Window

        // Data
        packet.extend_from_slice(data);

        stream.write_all(&packet).await?;
        stream.flush().await?;

        Ok(())
    }
}

/// TDS packet header
#[allow(dead_code)]
struct TdsHeader {
    packet_type: u8,
    #[allow(dead_code)]
    status: u8,
    length: u16,
    #[allow(dead_code)]
    spid: u16,
    #[allow(dead_code)]
    packet_id: u8,
    #[allow(dead_code)]
    window: u8,
}

/// Map SQL type name to TDS type code
fn get_tds_type(type_name: &str) -> u8 {
    match type_name.to_uppercase().as_str() {
        "INT" | "INTEGER" => 0x38,       // INTN
        "BIGINT" => 0x7F,                 // INT8
        "SMALLINT" => 0x34,               // INT2
        "TINYINT" => 0x30,                // INT1
        "BIT" => 0x32,                    // BIT
        "FLOAT" | "REAL" => 0x3B,         // FLT4/FLT8
        "NVARCHAR" | "NCHAR" | "NTEXT" => 0xE7, // NVARCHAR
        "VARCHAR" | "CHAR" | "TEXT" => 0xA7,    // VARCHAR
        _ => 0xE7,                        // Default: NVARCHAR
    }
}

/// Convert JSON value to string
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
    }
}
