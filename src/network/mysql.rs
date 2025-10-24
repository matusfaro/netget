//! MySQL server implementation using opensrv-mysql

use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::network::connection::ConnectionId;
use crate::network::mysql_actions::MysqlProtocol;
use crate::state::app_state::AppState;
use anyhow::Result;
use async_trait::async_trait;
use opensrv_mysql::{
    AsyncMysqlIntermediary, AsyncMysqlShim, Column, ColumnFlags, ColumnType, InitWriter,
    OkResponse, ParamParser, QueryResultWriter, StatementMetaWriter, StatusFlags,
};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

/// MySQL server implementation
pub struct MysqlServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl MysqlServer {
    /// Create a new MySQL server
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

    /// Spawn MySQL server with LLM integration
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

        info!("MySQL server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] MySQL server listening on {}", actual_addr));

        let server = Arc::new(MysqlServer::new(
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
                        debug!("MySQL connection from {}", addr);
                        let _ = status_tx.send(format!("[DEBUG] MySQL connection from {}", addr));

                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(actual_addr);

                        let handler = MysqlHandler::new(
                            connection_id,
                            server.llm_client.clone(),
                            server.app_state.clone(),
                            status_tx.clone(),
                            server.server_id,
                            addr,
                        );

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
                                protocol_info: ProtocolConnectionInfo::Mysql,
                            };
                            server
                                .app_state
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                        }

                        tokio::spawn(async move {
                            // MySQL requires split read/write streams
                            let (reader, writer) = tokio::io::split(stream);
                            if let Err(e) = AsyncMysqlIntermediary::run_on(handler, reader, writer).await {
                                error!("MySQL connection error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("MySQL accept error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] MySQL accept error: {}", e));
                    }
                }
            }
        });

        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        Ok(actual_addr)
    }
}

/// MySQL connection handler
pub struct MysqlHandler {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    server_id: Option<crate::state::ServerId>,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    /// MySQL protocol handler for action execution
    protocol: Arc<MysqlProtocol>,
    /// Prepared statements
    prepared_statements: Arc<Mutex<std::collections::HashMap<u32, String>>>,
    /// Next statement ID
    next_stmt_id: Arc<Mutex<u32>>,
}

impl MysqlHandler {
    pub fn new(
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
        remote_addr: SocketAddr,
    ) -> Self {
        let protocol = Arc::new(MysqlProtocol::new(
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
            prepared_statements: Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_stmt_id: Arc::new(Mutex::new(1)),
        }
    }
}

#[async_trait]
impl<W: tokio::io::AsyncWrite + Send + Unpin> AsyncMysqlShim<W> for MysqlHandler {
    type Error = io::Error;

    async fn on_prepare<'a>(
        &'a mut self,
        query: &'a str,
        info: StatementMetaWriter<'a, W>,
    ) -> io::Result<()> {
        debug!("MySQL PREPARE: {}", query);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL PREPARE: {}", query));

        // Store the prepared statement
        let mut next_id = self.next_stmt_id.lock().await;
        let stmt_id = *next_id;
        *next_id += 1;
        drop(next_id);

        let mut stmts = self.prepared_statements.lock().await;
        stmts.insert(stmt_id, query.to_string());
        drop(stmts);

        // Reply with the statement ID
        info.reply(stmt_id, &[], &[]).await
    }

    async fn on_execute<'a>(
        &'a mut self,
        stmt_id: u32,
        _params: ParamParser<'a>,
        results: QueryResultWriter<'a, W>,
    ) -> io::Result<()> {
        debug!("MySQL EXECUTE statement {}", stmt_id);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL EXECUTE statement {}", stmt_id));

        // Get the prepared statement
        let stmts = self.prepared_statements.lock().await;
        let query = stmts.get(&stmt_id).cloned();
        drop(stmts);

        if let Some(query) = query {
            // Treat as a regular query
            self.handle_query(&query, results).await
        } else {
            results
                .completed(OkResponse {
                    header: 0,
                    affected_rows: 0,
                    last_insert_id: 0,
                    status_flags: StatusFlags::empty(),
                    warnings: 0,
                    info: String::new(),
                    session_state_info: String::new(),
                })
                .await
        }
    }

    async fn on_close(&mut self, stmt_id: u32) {
        debug!("MySQL CLOSE statement {}", stmt_id);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL CLOSE statement {}", stmt_id));

        let mut stmts = self.prepared_statements.lock().await;
        stmts.remove(&stmt_id);
    }

    async fn on_query<'a>(
        &'a mut self,
        query: &'a str,
        results: QueryResultWriter<'a, W>,
    ) -> io::Result<()> {
        debug!("MySQL QUERY: {}", query);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL QUERY: {}", query));

        self.handle_query(query, results).await
    }

    async fn on_init<'a>(
        &'a mut self,
        _database: &'a str,
        writer: InitWriter<'a, W>,
    ) -> io::Result<()> {
        debug!("MySQL INIT DB: {}", _database);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] MySQL INIT DB: {}", _database));

        writer.ok().await
    }
}

impl MysqlHandler {
    async fn handle_query<'a, W: tokio::io::AsyncWrite + Send + Unpin>(
        &'a mut self,
        query: &str,
        results: QueryResultWriter<'a, W>,
    ) -> io::Result<()> {
        trace!("Calling LLM for MySQL query: {}", query);

        // Build context for LLM
        let _context = serde_json::json!({
            "query": query,
            "connection_id": self.connection_id.to_string(),
        });

        // Call LLM with actions
        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(0));
        let event_description = format!("SQL Query: {}", query);

        let llm_result = crate::llm::call_llm_with_actions(
            &self.llm_client,
            &self.app_state,
            server_id,
            &event_description,
            Some(self.protocol.as_ref()),
            vec![],
        )
        .await;

        match llm_result {
            Ok(execution_result) => {
                // Process action results to find MySQL responses
                for result in execution_result.protocol_results {
                    match result {
                        ActionResult::MysqlQueryResponse { columns, rows } => {
                            // Send result set
                            return send_result_set(results, columns, rows).await;
                        }
                        ActionResult::MysqlError { error_code, message } => {
                            // Send error - opensrv uses completed for errors too
                            let _ = self.status_tx.send(format!(
                                "[ERROR] MySQL error {}: {}",
                                error_code, message
                            ));
                            return results
                                .completed(OkResponse {
                                    header: 0,
                                    affected_rows: 0,
                                    last_insert_id: 0,
                                    status_flags: StatusFlags::empty(),
                                    warnings: 0,
                                    info: String::new(),
                                    session_state_info: String::new(),
                                })
                                .await;
                        }
                        ActionResult::MysqlOk {
                            affected_rows,
                            last_insert_id,
                        } => {
                            // Send OK response
                            return results
                                .completed(OkResponse {
                                    header: 0,
                                    affected_rows,
                                    last_insert_id,
                                    status_flags: StatusFlags::empty(),
                                    warnings: 0,
                                    info: String::new(),
                                    session_state_info: String::new(),
                                })
                                .await;
                        }
                        _ => {
                            // Other action results are informational, continue processing
                        }
                    }
                }

                // No MySQL-specific response found, return empty result set
                results
                    .completed(OkResponse {
                        header: 0,
                        affected_rows: 0,
                        last_insert_id: 0,
                        status_flags: StatusFlags::empty(),
                        warnings: 0,
                        info: String::new(),
                        session_state_info: String::new(),
                    })
                    .await
            }
            Err(e) => {
                error!("LLM error for MySQL query: {}", e);
                results
                    .completed(OkResponse {
                        header: 0,
                        affected_rows: 0,
                        last_insert_id: 0,
                        status_flags: StatusFlags::empty(),
                        warnings: 0,
                        info: String::new(),
                        session_state_info: String::new(),
                    })
                    .await
            }
        }
    }
}

/// Send a result set to the client
async fn send_result_set<'a, W: tokio::io::AsyncWrite + Send + Unpin>(
    results: QueryResultWriter<'a, W>,
    columns: Vec<serde_json::Value>,
    rows: Vec<serde_json::Value>,
) -> io::Result<()> {
    // Parse column definitions
    let mut cols = Vec::new();
    for col_def in &columns {
        let name = col_def
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("column");

        let col_type = col_def
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("VARCHAR");

        let mysql_type = match col_type.to_uppercase().as_str() {
            "INT" | "INTEGER" => ColumnType::MYSQL_TYPE_LONG,
            "BIGINT" => ColumnType::MYSQL_TYPE_LONGLONG,
            "SMALLINT" => ColumnType::MYSQL_TYPE_SHORT,
            "TINYINT" => ColumnType::MYSQL_TYPE_TINY,
            "FLOAT" => ColumnType::MYSQL_TYPE_FLOAT,
            "DOUBLE" => ColumnType::MYSQL_TYPE_DOUBLE,
            "DECIMAL" => ColumnType::MYSQL_TYPE_DECIMAL,
            "DATE" => ColumnType::MYSQL_TYPE_DATE,
            "TIME" => ColumnType::MYSQL_TYPE_TIME,
            "DATETIME" | "TIMESTAMP" => ColumnType::MYSQL_TYPE_DATETIME,
            "BLOB" | "BINARY" => ColumnType::MYSQL_TYPE_BLOB,
            "TEXT" => ColumnType::MYSQL_TYPE_STRING,
            _ => ColumnType::MYSQL_TYPE_VAR_STRING,
        };

        cols.push(Column {
            table: "".to_string(),
            column: name.to_string(),
            coltype: mysql_type,
            colflags: ColumnFlags::empty(),
        });
    }

    // Start the result set
    let mut row_writer = results.start(&cols).await?;

    // Write rows
    for row_data in &rows {
        if let Some(row_values) = row_data.as_array() {
            // Convert JSON values to Strings (simplified - ToMysqlValue is implemented for String)
            let values: Vec<String> = row_values
                .iter()
                .map(|v| json_to_mysql_string(v))
                .collect();

            row_writer.write_row(values).await?;
        }
    }

    // Finish the result set
    row_writer.finish().await
}

/// Convert JSON value to MySQL string representation
fn json_to_mysql_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => v.to_string(),
    }
}
