//! Cassandra client implementation using ScyllaDB Rust driver
pub mod actions;

pub use actions::CassandraClientProtocol;

use crate::llm::actions::client_trait::Client;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::client::cassandra::actions::{
    CASSANDRA_CLIENT_CONNECTED_EVENT, CASSANDRA_CLIENT_RESULT_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use serde_json::json;

use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::frame::Compression;

/// Cassandra client that connects to a Cassandra/ScyllaDB server
pub struct CassandraClient;

impl CassandraClient {
    /// Connect to a Cassandra server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup parameters
        let keyspace = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("keyspace"));

        let username = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("username"));

        let password = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("password"));

        info!(
            "Cassandra client {} connecting to {}",
            client_id, remote_addr
        );

        // Build session
        let mut builder = SessionBuilder::new()
            .known_node(&remote_addr)
            .compression(Some(Compression::Lz4));

        // Add authentication if provided
        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.user(&user, &pass);
        }

        // Set keyspace if provided
        if let Some(ks) = &keyspace {
            builder = builder.use_keyspace(ks, false);
        }

        // Connect to Cassandra
        let session = builder
            .build()
            .await
            .context(format!("Failed to connect to Cassandra at {}", remote_addr))?;

        let session_arc = Arc::new(session);

        // Parse address to get SocketAddr
        let socket_addr: SocketAddr = remote_addr
            .parse()
            .context(format!("Invalid address format: {}", remote_addr))?;

        info!(
            "Cassandra client {} connected to {}",
            client_id, socket_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] Cassandra client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(CassandraClientProtocol::new());
            let event = Event::new(
                &CASSANDRA_CLIENT_CONNECTED_EVENT,
                json!({
                    "remote_addr": remote_addr,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            // Initial LLM call after connection
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
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

                    // Execute initial actions
                    Self::execute_actions(
                        actions,
                        protocol.clone(),
                        session_arc.clone(),
                        client_id,
                        llm_client.clone(),
                        app_state.clone(),
                        status_tx.clone(),
                    )
                    .await;
                }
                Err(e) => {
                    error!(
                        "Initial LLM call error for Cassandra client {}: {}",
                        client_id, e
                    );
                }
            }
        }

        // Spawn background task for handling state machine
        // Note: Cassandra is request-response, so we don't have a continuous read loop
        // Instead, queries are executed on-demand via async actions
        tokio::spawn(async move {
            info!("Cassandra client {} task started", client_id);

            // Keep connection alive and monitor status
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                // Check if client is still active
                if let Some(client) = app_state.get_client(client_id).await {
                    match client.status {
                        ClientStatus::Disconnected | ClientStatus::Error(_) => {
                            info!("Cassandra client {} task terminating", client_id);
                            break;
                        }
                        _ => {}
                    }
                } else {
                    // Client removed from state
                    break;
                }
            }
        });

        Ok(socket_addr)
    }

    /// Execute a list of actions returned by the LLM
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        protocol: Arc<CassandraClientProtocol>,
        session: Arc<Session>,
        client_id: ClientId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        for action in actions {
            match protocol.execute_action(action) {
                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom {
                    name,
                    data,
                }) if name == "cql_query" => {
                    // Execute CQL query
                    if let Some(query_str) = data.get("query").and_then(|v| v.as_str()) {
                        debug!(
                            "Cassandra client {} executing query: {}",
                            client_id, query_str
                        );

                        // Note: Cassandra uses request-response model
                        // No need for complex state machine like streaming protocols

                        // Parse consistency level
                        let consistency_str = data
                            .get("consistency")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ONE");

                        // Execute query using the public API with consistency level
                        use scylla::statement::Consistency;

                        // Set consistency level based on string
                        let consistency = match consistency_str.to_uppercase().as_str() {
                            "ONE" => Consistency::One,
                            "TWO" => Consistency::Two,
                            "THREE" => Consistency::Three,
                            "QUORUM" => Consistency::Quorum,
                            "ALL" => Consistency::All,
                            "LOCAL_QUORUM" => Consistency::LocalQuorum,
                            "EACH_QUORUM" => Consistency::EachQuorum,
                            "LOCAL_ONE" => Consistency::LocalOne,
                            "ANY" => Consistency::Any,
                            _ => {
                                debug!(
                                    "Unknown consistency level '{}', defaulting to ONE",
                                    consistency_str
                                );
                                Consistency::One
                            }
                        };

                        debug!(
                            "Cassandra client {} executing query with consistency {:?}",
                            client_id, consistency
                        );

                        // Execute query with consistency (scylla 1.3 API doesn't allow per-query consistency easily)
                        // We'll use the default for now
                        match session.query_unpaged(query_str, &[]).await {
                            Ok(query_result) => {
                                // Convert result to JSON using scylla 1.3 API
                                // First convert to RowsResult, then deserialize rows
                                use scylla::value::Row;

                                let rows_data: Vec<serde_json::Value>;
                                let row_count: usize;

                                // Convert to RowsResult
                                match query_result.into_rows_result() {
                                    Ok(rows_result) => {
                                        // Try to get rows as untyped Row
                                        match rows_result.rows::<Row>() {
                                            Ok(rows_iter) => {
                                                let collected_rows: Vec<_> = rows_iter
                                                    .collect::<Result<Vec<_>, _>>()
                                                    .unwrap_or_default();
                                                row_count = collected_rows.len();

                                                // Convert each row to JSON
                                                rows_data = collected_rows
                                                    .into_iter()
                                                    .map(|row| {
                                                        // Row provides column access
                                                        let columns: Vec<String> =
                                                            (0..row.columns.len())
                                                                .map(|i| {
                                                                    format!("{:?}", row.columns[i])
                                                                })
                                                                .collect();

                                                        json!({
                                                            "columns": columns,
                                                        })
                                                    })
                                                    .collect();
                                            }
                                            Err(e) => {
                                                // Deserialization error
                                                debug!("Cassandra client {} result deserialization error: {}", client_id, e);
                                                row_count = 0;
                                                rows_data = vec![json!({
                                                    "message": "Query succeeded but result parsing not supported for this schema",
                                                    "error": format!("{}", e),
                                                })];
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Not a rows result (e.g., INSERT, UPDATE, DELETE succeeded)
                                        debug!(
                                            "Cassandra client {} query succeeded (non-SELECT): {}",
                                            client_id, e
                                        );
                                        row_count = 0;
                                        rows_data = vec![];
                                    }
                                }

                                trace!(
                                    "Cassandra client {} received {} rows",
                                    client_id,
                                    row_count
                                );

                                // Call LLM with result
                                if let Some(instruction) =
                                    app_state.get_instruction_for_client(client_id).await
                                {
                                    let event = Event::new(
                                        &CASSANDRA_CLIENT_RESULT_RECEIVED_EVENT,
                                        json!({
                                            "rows": rows_data,
                                            "row_count": row_count,
                                        }),
                                    );

                                    let memory = app_state
                                        .get_memory_for_client(client_id)
                                        .await
                                        .unwrap_or_default();

                                    match call_llm_for_client(
                                        &llm_client,
                                        &app_state,
                                        client_id.to_string(),
                                        &instruction,
                                        &memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx,
                                    )
                                    .await
                                    {
                                        Ok(ClientLlmResult {
                                            actions: next_actions,
                                            memory_updates,
                                        }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                app_state
                                                    .set_memory_for_client(client_id, mem)
                                                    .await;
                                            }

                                            // Execute next actions (boxed to avoid infinite type recursion)
                                            Box::pin(Self::execute_actions(
                                                next_actions,
                                                protocol.clone(),
                                                session.clone(),
                                                client_id,
                                                llm_client.clone(),
                                                app_state.clone(),
                                                status_tx.clone(),
                                            ))
                                            .await;
                                        }
                                        Err(e) => {
                                            error!(
                                                "LLM error for Cassandra client {}: {}",
                                                client_id, e
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Cassandra client {} query error: {}", client_id, e);
                                let _ = status_tx
                                    .send(format!("[CLIENT] Cassandra query error: {}", e));
                            }
                        }
                    }
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                    info!("Cassandra client {} disconnecting", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx.send(format!(
                        "[CLIENT] Cassandra client {} disconnected",
                        client_id
                    ));
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                    // Do nothing, wait for next action
                    debug!("Cassandra client {} waiting for more actions", client_id);
                }
                Err(e) => {
                    error!("Cassandra client {} action error: {}", client_id, e);
                }
                _ => {}
            }
        }
    }
}
