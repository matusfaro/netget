//! MSSQL client implementation using tiberius
pub mod actions;

pub use actions::MssqlClientProtocol;

use crate::client::mssql::actions::{
    MSSQL_CLIENT_CONNECTED_EVENT, MSSQL_CLIENT_ERROR_EVENT, MSSQL_CLIENT_QUERY_RESULT_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use futures::StreamExt;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tiberius::{AuthMethod, Client as TiberiusClient, Config, QueryItem, Row};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tracing::{debug, error, info, trace};

/// MSSQL client that connects to an MSSQL/SQL Server
pub struct MssqlClient;

impl MssqlClient {
    /// Connect to an MSSQL server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse connection string (format: "host:port" or "host:port;database=db;user=user;password=pass")
        let (host_port, config_params) = if remote_addr.contains(';') {
            let parts: Vec<&str> = remote_addr.splitn(2, ';').collect();
            (parts[0].to_string(), Some(parts[1].to_string()))
        } else {
            (remote_addr.clone(), None)
        };

        // Parse host and port
        let (host, port) = if host_port.contains(':') {
            let parts: Vec<&str> = host_port.split(':').collect();
            (parts[0].to_string(), parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(1433))
        } else {
            (host_port, 1433)
        };

        // Build tiberius config
        let mut config = Config::new();
        config.host(&host);
        config.port(port);
        config.trust_cert(); // For testing - accept self-signed certs

        // Parse optional connection parameters
        if let Some(params_str) = config_params {
            for param in params_str.split(';') {
                let kv: Vec<&str> = param.split('=').collect();
                if kv.len() == 2 {
                    match kv[0].to_lowercase().as_str() {
                        "database" => config.database(kv[1]),
                        "user" => config.authentication(AuthMethod::sql_server(kv[1], "")),
                        _ => {}
                    };
                }
            }
        }

        // Always use no authentication for now (works with our test server)
        // In production, parse credentials from connection string
        config.authentication(AuthMethod::None);

        // Connect to MSSQL server
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .context(format!("Failed to connect to MSSQL at {}:{}", host, port))?;

        let local_addr = tcp.local_addr()?;
        let remote_sock_addr = tcp.peer_addr()?;

        info!(
            "MSSQL client {} connected to {} (local: {})",
            client_id, remote_sock_addr, local_addr
        );

        // Create tiberius client
        let client = TiberiusClient::connect(config, tcp.compat_write())
            .await
            .context("Failed to create tiberius client")?;

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] MSSQL client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let client_arc = Arc::new(Mutex::new(client));
        let client_for_connected = client_arc.clone();

        // Call LLM with mssql_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &MSSQL_CLIENT_CONNECTED_EVENT,
                json!({
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(), // No memory yet for initial connection
                Some(&event),
                &MssqlClientProtocol::new(),
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Execute actions from LLM response
                    for action in result.actions {
                        if let Err(e) = Self::execute_action_internal(
                            client_for_connected.clone(),
                            action,
                            client_id,
                            &llm_client,
                            &app_state,
                            &status_tx,
                        )
                        .await
                        {
                            error!("Failed to execute action after connect: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error on mssql_connected event: {}", e);
                }
            }
        }

        // Note: Unlike Redis, MSSQL (via tiberius) is query-driven, not event-driven
        // We don't spawn a read loop here - queries are executed on-demand when LLM requests them
        // This is handled through subsequent LLM interactions where execute_query actions are sent

        // For demonstration, we'll keep the connection alive but queries must be explicitly triggered
        // Real usage would involve periodic LLM calls or a command loop

        info!("MSSQL client {} ready for queries", client_id);

        Ok(local_addr)
    }

    /// Execute an action (internal helper)
    fn execute_action_internal<'a>(
        client: Arc<Mutex<TiberiusClient<tokio_util::compat::Compat<TcpStream>>>>,
        action: serde_json::Value,
        client_id: ClientId,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
        let protocol = Arc::new(MssqlClientProtocol::new());

        match protocol.execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }
                if name == "mssql_query" =>
            {
                if let Some(query_str) = data.get("query").and_then(|v| v.as_str()) {
                    debug!("MSSQL client {} executing query: {}", client_id, query_str);

                    // Execute query and collect results (mutex held during query execution)
                    let result_outcome = {
                        let mut client_guard = client.lock().await;
                        Self::execute_and_collect_query(&mut client_guard, query_str).await
                    }; // Mutex released here

                    // Process results or errors
                    match result_outcome {
                        Ok(result) => {
                            debug!(
                                "MSSQL client {} received {} columns, {} rows",
                                client_id,
                                result.columns.len(),
                                result.rows.len()
                            );

                            // Call LLM with query result
                            if let Some(instruction) =
                                app_state.get_instruction_for_client(client_id).await
                            {
                                let event = Event::new(
                                    &MSSQL_CLIENT_QUERY_RESULT_EVENT,
                                    json!({
                                        "columns": result.columns,
                                        "rows": result.rows,
                                        "rows_affected": result.rows_affected,
                                    }),
                                );

                                let memory = app_state
                                    .get_memory_for_client(client_id)
                                    .await
                                    .unwrap_or_default();

                                match call_llm_for_client(
                                    llm_client,
                                    app_state,
                                    client_id.to_string(),
                                    &instruction,
                                    &memory,
                                    Some(&event),
                                    protocol.as_ref(),
                                    status_tx,
                                )
                                .await
                                {
                                    Ok(ClientLlmResult {
                                        actions,
                                        memory_updates,
                                    }) => {
                                        // Update memory
                                        if let Some(mem) = memory_updates {
                                            app_state.set_memory_for_client(client_id, mem).await;
                                        }

                                        // Execute follow-up actions
                                        for follow_action in actions {
                                            if let Err(e) = Self::execute_action_internal(
                                                client.clone(),
                                                follow_action,
                                                client_id,
                                                llm_client,
                                                app_state,
                                                status_tx,
                                            )
                                            .await
                                            {
                                                error!("Failed to execute follow-up action: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("LLM error for MSSQL client {}: {}", client_id, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("MSSQL query error: {}", e);

                            // Call LLM with error event
                            if let Some(instruction) =
                                app_state.get_instruction_for_client(client_id).await
                            {
                                let event = Event::new(
                                    &MSSQL_CLIENT_ERROR_EVENT,
                                    json!({
                                        "error_number": 50000,
                                        "message": e.to_string(),
                                    }),
                                );

                                let _ = call_llm_for_client(
                                    llm_client,
                                    app_state,
                                    client_id.to_string(),
                                    &instruction,
                                    &app_state
                                        .get_memory_for_client(client_id)
                                        .await
                                        .unwrap_or_default(),
                                    Some(&event),
                                    protocol.as_ref(),
                                    status_tx,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                info!("MSSQL client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            crate::llm::actions::client_trait::ClientActionResult::WaitForMore => {
                trace!("MSSQL client {} waiting for more data", client_id);
            }
            _ => {}
        }

        Ok(())
        })
    }

    /// Execute query and collect results
    async fn execute_and_collect_query(
        client: &mut TiberiusClient<tokio_util::compat::Compat<TcpStream>>,
        query: &str,
    ) -> Result<QueryResult> {
        let mut stream = client.query(query, &[]).await?;

        let mut columns = Vec::new();
        let mut rows = Vec::new();
        let rows_affected: u64;

        // Collect column metadata
        if let Some(cols) = stream.columns().await? {
            for col in cols {
                columns.push(json!({
                    "name": col.name(),
                    "type": format!("{:?}", col.column_type()),
                }));
            }
        }

        // Collect rows
        while let Some(item) = stream.next().await {
            match item {
                Ok(QueryItem::Row(row)) => {
                    let row_values = Self::row_to_json(&row)?;
                    rows.push(row_values);
                }
                Ok(QueryItem::Metadata(_)) => {
                    // Metadata already processed above
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        // For SELECT queries, rows_affected is typically the row count
        rows_affected = rows.len() as u64;

        Ok(QueryResult {
            columns,
            rows,
            rows_affected,
        })
    }

    /// Collect query results into JSON format (legacy - not used)
    #[allow(dead_code)]
    async fn collect_query_results(
        mut stream: tiberius::QueryStream<'_>,
    ) -> Result<QueryResult> {
        let mut columns = Vec::new();
        let mut rows = Vec::new();

        // Collect column metadata
        if let Some(cols) = stream.columns().await? {
            for col in cols {
                columns.push(json!({
                    "name": col.name(),
                    "type": format!("{:?}", col.column_type()),
                }));
            }
        }

        // Collect rows
        while let Some(item) = stream.next().await {
            match item {
                Ok(QueryItem::Row(row)) => {
                    let row_values = Self::row_to_json(&row)?;
                    rows.push(row_values);
                }
                Ok(QueryItem::Metadata(_)) => {
                    // Metadata already processed above
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        // For SELECT queries, rows_affected is typically 0 or the row count
        // For INSERT/UPDATE/DELETE, tiberius would return this in metadata
        // For simplicity, we'll use the row count as a proxy
        let rows_affected = rows.len() as u64;

        Ok(QueryResult {
            columns,
            rows,
            rows_affected,
        })
    }

    /// Convert a tiberius Row to JSON array
    fn row_to_json(row: &Row) -> Result<serde_json::Value> {
        let mut values = Vec::new();

        for i in 0..row.len() {
            let value = if let Ok(Some(s)) = row.try_get::<&str, _>(i) {
                json!(s)
            } else if let Ok(Some(n)) = row.try_get::<i32, _>(i) {
                json!(n)
            } else if let Ok(Some(n)) = row.try_get::<i64, _>(i) {
                json!(n)
            } else if let Ok(Some(b)) = row.try_get::<bool, _>(i) {
                json!(b)
            } else if let Ok(Some(f)) = row.try_get::<f32, _>(i) {
                json!(f)
            } else if let Ok(Some(f)) = row.try_get::<f64, _>(i) {
                json!(f)
            } else {
                // NULL or unsupported type
                json!(null)
            };

            values.push(value);
        }

        Ok(json!(values))
    }
}

/// Query result structure
struct QueryResult {
    columns: Vec<serde_json::Value>,
    rows: Vec<serde_json::Value>,
    rows_affected: u64,
}
