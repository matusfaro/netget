//! PostgreSQL server implementation using pgwire
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use actions::{PostgresqlProtocol, POSTGRESQL_QUERY_EVENT};
use crate::protocol::Event;
use crate::state::app_state::AppState;
use anyhow::Result;
use pgwire::api::auth::noop::NoopStartupHandler;
use pgwire::api::copy::NoopCopyHandler;
use pgwire::api::portal::Portal;
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::{DataRowEncoder, DescribePortalResponse, DescribeStatementResponse, FieldFormat, FieldInfo, QueryResponse, Response, Tag};
use pgwire::api::stmt::StoredStatement;
use pgwire::api::{ClientInfo, PgWireHandlerFactory, Type};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use pgwire::tokio::process_socket;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// PostgreSQL server implementation
pub struct PostgresqlServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    #[allow(dead_code)]
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl PostgresqlServer {
    /// Create a new PostgreSQL server
    pub fn new(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Self {
        Self {
            llm_client,
            app_state,
            status_tx,
            server_id,
        }
    }

    /// Spawn PostgreSQL server with LLM integration
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

        info!("PostgreSQL server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] PostgreSQL server listening on {}", actual_addr));

        let server = Arc::new(PostgresqlServer::new(
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
                        debug!("PostgreSQL connection from {}", addr);
                        let _ = status_tx.send(format!("[DEBUG] PostgreSQL connection from {}", addr));

                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(actual_addr);

                        let handler_factory = Arc::new(PostgresqlHandlerFactory {
                            connection_id,
                            llm_client: server.llm_client.clone(),
                            app_state: server.app_state.clone(),
                            status_tx: status_tx.clone(),
                            server_id: server.server_id,
                            remote_addr: addr,
                        });

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

                        tokio::spawn(async move {
                            if let Err(e) = process_socket(
                                stream,
                                None,
                                handler_factory,
                            )
                            .await
                            {
                                error!("PostgreSQL connection error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("PostgreSQL accept error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] PostgreSQL accept error: {}", e));
                    }
                }
            }
        });

        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        Ok(actual_addr)
    }
}

/// Factory for creating PostgreSQL handlers
struct PostgresqlHandlerFactory {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
    remote_addr: SocketAddr,
}

impl PgWireHandlerFactory for PostgresqlHandlerFactory {
    type StartupHandler = NoopStartupHandler;
    type SimpleQueryHandler = PostgresqlHandler;
    type ExtendedQueryHandler = PostgresqlHandler;
    type CopyHandler = NoopCopyHandler;

    fn simple_query_handler(&self) -> Arc<Self::SimpleQueryHandler> {
        Arc::new(PostgresqlHandler {
            connection_id: self.connection_id,
            llm_client: self.llm_client.clone(),
            app_state: self.app_state.clone(),
            status_tx: self.status_tx.clone(),
            server_id: self.server_id,
            remote_addr: self.remote_addr,
            protocol: Arc::new(PostgresqlProtocol::new(
                self.connection_id,
                self.app_state.clone(),
                self.status_tx.clone(),
            )),
        })
    }

    fn extended_query_handler(&self) -> Arc<Self::ExtendedQueryHandler> {
        // Reuse the same handler for extended queries
        Arc::new(PostgresqlHandler {
            connection_id: self.connection_id,
            llm_client: self.llm_client.clone(),
            app_state: self.app_state.clone(),
            status_tx: self.status_tx.clone(),
            server_id: self.server_id,
            remote_addr: self.remote_addr,
            protocol: Arc::new(PostgresqlProtocol::new(
                self.connection_id,
                self.app_state.clone(),
                self.status_tx.clone(),
            )),
        })
    }

    fn startup_handler(&self) -> Arc<Self::StartupHandler> {
        Arc::new(NoopStartupHandler)
    }

    fn copy_handler(&self) -> Arc<Self::CopyHandler> {
        Arc::new(NoopCopyHandler)
    }
}

/// PostgreSQL connection handler
pub struct PostgresqlHandler {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    server_id: Option<crate::state::ServerId>,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    /// PostgreSQL protocol handler for action execution
    protocol: Arc<PostgresqlProtocol>,
}

#[async_trait::async_trait]
impl SimpleQueryHandler for PostgresqlHandler {
    async fn do_query<'a, C>(
        &self,
        _client: &mut C,
        query: &'a str,
    ) -> PgWireResult<Vec<Response<'a>>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        debug!("PostgreSQL SIMPLE QUERY: {}", query);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] PostgreSQL SIMPLE QUERY: {}", query));

        trace!("Simple query handler calling LLM for: {}", query);

        // Create query event
        let event = Event::new(
            &POSTGRESQL_QUERY_EVENT,
            serde_json::json!({
                "query": query,
            }),
        );

        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(0));

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
                // Process action results to find PostgreSQL responses
                for result in execution_result.protocol_results {
                    match result {
                        ActionResult::Custom { name, data } => {
                            match name.as_str() {
                                "postgresql_query_response" => {
                                    // Extract columns and rows from JSON data
                                    let columns = data.get("columns")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();
                                    let rows = data.get("rows")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();

                                    // Convert columns to FieldInfo
                                    let field_infos: Vec<FieldInfo> = columns
                                        .iter()
                                        .filter_map(|col| {
                                            let name = col.get("name")?.as_str()?;
                                            let type_name = col.get("type").and_then(|v| v.as_str()).unwrap_or("text");

                                            let pg_type = match type_name.to_lowercase().as_str() {
                                                "int2" | "smallint" => Type::INT2,
                                                "int4" | "int" | "integer" => Type::INT4,
                                                "int8" | "bigint" => Type::INT8,
                                                "float4" | "real" => Type::FLOAT4,
                                                "float8" | "double" | "double precision" => Type::FLOAT8,
                                                "bool" | "boolean" => Type::BOOL,
                                                "date" => Type::DATE,
                                                "time" => Type::TIME,
                                                "timestamp" => Type::TIMESTAMP,
                                                "varchar" | "text" | _ => Type::VARCHAR,
                                            };

                                            Some(FieldInfo::new(
                                                name.to_string(),
                                                None,
                                                None,
                                                pg_type,
                                                FieldFormat::Text,
                                            ))
                                        })
                                        .collect();

                                    // Create data rows as a stream
                                    let mut data_rows = Vec::new();
                                    for row_data in &rows {
                                        if let Some(row_values) = row_data.as_array() {
                                            let mut encoder = DataRowEncoder::new(Arc::new(field_infos.clone()));

                                            for (idx, value) in row_values.iter().enumerate() {
                                                if idx < field_infos.len() {
                                                    let value_str = json_value_to_string(value);
                                                    encoder.encode_field(&value_str.as_str()).map_err(|e| {
                                                        PgWireError::ApiError(Box::new(std::io::Error::new(
                                                            std::io::ErrorKind::InvalidData,
                                                            format!("Failed to encode field: {}", e),
                                                        )))
                                                    })?;
                                                }
                                            }
                                            data_rows.push(encoder.finish());
                                        }
                                    }

                                    // Convert Vec to Stream
                                    let row_stream = futures::stream::iter(data_rows);
                                    return Ok(vec![Response::Query(QueryResponse::new(
                                        Arc::new(field_infos),
                                        row_stream,
                                    ))]);
                                }
                                "postgresql_error" => {
                                    // Extract error info from JSON data
                                    let severity = data.get("severity")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("ERROR")
                                        .to_string();
                                    let code = data.get("code")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("XX000")
                                        .to_string();
                                    let message = data.get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown error")
                                        .to_string();

                                    let _ = self.status_tx.send(format!(
                                        "[ERROR] PostgreSQL error {} {}: {}",
                                        severity, code, message
                                    ));

                                    return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                                        severity,
                                        code,
                                        message,
                                    ))));
                                }
                                "postgresql_ok" => {
                                    // Extract tag from JSON data
                                    let tag = data.get("tag")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("OK");

                                    return Ok(vec![Response::Execution(Tag::new(tag))]);
                                }
                                _ => {
                                    // Unknown custom response, ignore
                                }
                            }
                        }
                        _ => {
                            // Other action results are informational, continue processing
                        }
                    }
                }

                // No PostgreSQL-specific response found, return empty result set
                Ok(vec![Response::Execution(Tag::new("OK"))])
            }
            Err(e) => {
                error!("LLM error for PostgreSQL query: {}", e);
                Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_string(),
                    "XX000".to_string(),
                    format!("LLM error: {}", e),
                ))))
            }
        }
    }
}

#[async_trait::async_trait]
impl ExtendedQueryHandler for PostgresqlHandler {
    type Statement = String;
    type QueryParser = PostgresqlQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        Arc::new(PostgresqlQueryParser)
    }

    async fn do_query<'a, 'b: 'a, C>(
        &'b self,
        _client: &mut C,
        portal: &'a Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response<'a>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        // Extract the SQL from the portal and execute it like a simple query
        let sql = &portal.statement.statement;

        debug!("PostgreSQL QUERY (extended): {}", sql);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] PostgreSQL QUERY (extended): {}", sql));

        debug!("Extended query handler calling LLM for: {}", sql);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] Extended query handler calling LLM for: {}", sql));

        // Create query event
        let event = Event::new(
            &POSTGRESQL_QUERY_EVENT,
            serde_json::json!({
                "query": sql,
            }),
        );

        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(0));

        let llm_result = call_llm(
            &self.llm_client,
            &self.app_state,
            server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await;

        debug!("Extended query handler LLM call completed");
        let _ = self
            .status_tx
            .send(format!("[DEBUG] Extended query handler LLM call completed"));

        match llm_result {
            Ok(execution_result) => {
                let num_results = execution_result.protocol_results.len();
                debug!("Extended query handler received {} protocol results", num_results);
                let _ = self.status_tx.send(format!("[DEBUG] Extended query handler received {num_results} protocol results"));

                // Process action results to find PostgreSQL responses
                for (idx, result) in execution_result.protocol_results.into_iter().enumerate() {
                    debug!("Processing protocol result {}: {:?}", idx, result);
                    let _ = self.status_tx.send(format!("[DEBUG] Processing protocol result {idx}: {result:?}"));

                    match result {
                        ActionResult::Custom { name, data } => {
                            match name.as_str() {
                                "postgresql_query_response" => {
                                    // Extract columns and rows from JSON data
                                    let columns = data.get("columns")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();
                                    let rows = data.get("rows")
                                        .and_then(|v| v.as_array())
                                        .cloned()
                                        .unwrap_or_default();

                                    debug!("Found postgresql_query_response with {} columns and {} rows", columns.len(), rows.len());
                                    let _ = self.status_tx.send(format!("[DEBUG] Found postgresql_query_response with {} columns and {} rows", columns.len(), rows.len()));

                                    // Convert columns to FieldInfo
                                    let field_infos: Vec<FieldInfo> = columns
                                        .iter()
                                        .filter_map(|col| {
                                            let name = col.get("name")?.as_str()?;
                                            let type_name = col.get("type").and_then(|v| v.as_str()).unwrap_or("text");

                                            let pg_type = match type_name.to_lowercase().as_str() {
                                                "int2" | "smallint" => Type::INT2,
                                                "int4" | "int" | "integer" => Type::INT4,
                                                "int8" | "bigint" => Type::INT8,
                                                "float4" | "real" => Type::FLOAT4,
                                                "float8" | "double" | "double precision" => Type::FLOAT8,
                                                "bool" | "boolean" => Type::BOOL,
                                                "date" => Type::DATE,
                                                "time" => Type::TIME,
                                                "timestamp" => Type::TIMESTAMP,
                                                "varchar" | "text" | _ => Type::VARCHAR,
                                            };

                                            Some(FieldInfo::new(
                                                name.to_string(),
                                                None,
                                                None,
                                                pg_type,
                                                FieldFormat::Text,
                                            ))
                                        })
                                        .collect();

                                    // Create data rows as a stream
                                    let mut data_rows = Vec::new();
                                    for row_data in &rows {
                                        if let Some(row_values) = row_data.as_array() {
                                            let mut encoder = DataRowEncoder::new(Arc::new(field_infos.clone()));

                                            for (idx, value) in row_values.iter().enumerate() {
                                                if idx < field_infos.len() {
                                                    let value_str = json_value_to_string(value);
                                                    encoder.encode_field(&value_str.as_str()).map_err(|e| {
                                                        PgWireError::ApiError(Box::new(std::io::Error::new(
                                                            std::io::ErrorKind::InvalidData,
                                                            format!("Failed to encode field: {}", e),
                                                        )))
                                                    })?;
                                                }
                                            }
                                            data_rows.push(encoder.finish());
                                        }
                                    }

                                    // Convert Vec to Stream
                                    let row_stream = futures::stream::iter(data_rows);
                                    return Ok(Response::Query(QueryResponse::new(
                                        Arc::new(field_infos),
                                        row_stream,
                                    )));
                                }
                                "postgresql_error" => {
                                    // Extract error info from JSON data
                                    let severity = data.get("severity")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("ERROR")
                                        .to_string();
                                    let code = data.get("code")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("XX000")
                                        .to_string();
                                    let message = data.get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown error")
                                        .to_string();

                                    let _ = self.status_tx.send(format!(
                                        "[ERROR] PostgreSQL error {} {}: {}",
                                        severity, code, message
                                    ));

                                    return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                                        severity,
                                        code,
                                        message,
                                    ))));
                                }
                                "postgresql_ok" => {
                                    // Extract tag from JSON data
                                    let tag = data.get("tag")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("OK");

                                    return Ok(Response::Execution(Tag::new(tag)));
                                }
                                _ => {
                                    // Unknown custom response, ignore
                                }
                            }
                        }
                        _ => {
                            // Other action results are informational, continue processing
                        }
                    }
                }

                // No PostgreSQL-specific response found, return empty result set
                warn!("Extended query handler: No PostgreSQL response found in {} results, returning OK", num_results);
                let _ = self.status_tx.send(format!("[WARN] No PostgreSQL response action found from LLM after processing {} results", num_results));

                // For SELECT queries, return an empty result set instead of OK
                if sql.trim_start().to_uppercase().starts_with("SELECT") {
                    debug!("Returning empty result set for SELECT query");
                    let _ = self.status_tx.send("[DEBUG] Returning empty result set for SELECT query".to_string());
                    let empty_fields = vec![];
                    let empty_stream = futures::stream::empty();
                    Ok(Response::Query(QueryResponse::new(
                        Arc::new(empty_fields),
                        empty_stream,
                    )))
                } else {
                    Ok(Response::Execution(Tag::new("OK")))
                }
            }
            Err(e) => {
                error!("LLM error for PostgreSQL query (extended): {}", e);
                let _ = self.status_tx.send(format!("[ERROR] LLM error (extended): {}", e));

                // For SELECT queries, try to return an empty result set instead of error
                // This prevents the client from seeing "invalid column" errors
                if sql.trim_start().to_uppercase().starts_with("SELECT") {
                    warn!("Returning empty result set for SELECT after LLM error");
                    let _ = self.status_tx.send("[WARN] Returning empty result set after LLM error".to_string());
                    let empty_fields = vec![];
                    let empty_stream = futures::stream::empty();
                    Ok(Response::Query(QueryResponse::new(
                        Arc::new(empty_fields),
                        empty_stream,
                    )))
                } else {
                    Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                        "ERROR".to_string(),
                        "XX000".to_string(),
                        format!("LLM error: {}", e),
                    ))))
                }
            }
        }
    }

    async fn do_describe_statement<C>(
        &self,
        _client: &mut C,
        _stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        Ok(DescribeStatementResponse::new(vec![], vec![]))
    }

    async fn do_describe_portal<C>(
        &self,
        _client: &mut C,
        _portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        Ok(DescribePortalResponse::new(vec![]))
    }
}

/// Query parser for PostgreSQL
pub struct PostgresqlQueryParser;

#[async_trait::async_trait]
impl pgwire::api::stmt::QueryParser for PostgresqlQueryParser {
    type Statement = String;

    async fn parse_sql(&self, sql: &str, _types: &[Type]) -> PgWireResult<Self::Statement> {
        Ok(sql.to_string())
    }
}

/// Convert JSON value to string representation
fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "".to_string(),
        serde_json::Value::Bool(b) => if *b { "t" } else { "f" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
    }
}
