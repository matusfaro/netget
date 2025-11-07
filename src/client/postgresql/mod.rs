//! PostgreSQL client implementation
pub mod actions;

pub use actions::PostgresqlClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::postgresql::actions::{POSTGRESQL_CLIENT_CONNECTED_EVENT, POSTGRESQL_CLIENT_QUERY_RESULT_EVENT};

/// PostgreSQL client that connects to a PostgreSQL server
pub struct PostgresqlClient;

impl PostgresqlClient {
    /// Connect to a PostgreSQL server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract connection parameters from startup_params if provided
        let (database, user, password) = if let Some(params) = &startup_params {
            let database = params
                .get_optional_string("database")
                .unwrap_or_else(|| "postgres".to_string());
            let user = params
                .get_optional_string("user")
                .unwrap_or_else(|| "postgres".to_string());
            let password = params
                .get_optional_string("password")
                .unwrap_or_else(|| "".to_string());
            (database, user, password)
        } else {
            ("postgres".to_string(), "postgres".to_string(), "".to_string())
        };

        // Build connection string
        let conn_str = if password.is_empty() {
            format!("host={} user={} dbname={}", remote_addr, user, database)
        } else {
            format!(
                "host={} user={} password={} dbname={}",
                remote_addr, user, password, database
            )
        };

        info!(
            "PostgreSQL client {} connecting to {} (user={}, database={})",
            client_id, remote_addr, user, database
        );

        // Connect to PostgreSQL server
        let (client, connection) = tokio_postgres::connect(&conn_str, tokio_postgres::NoTls)
            .await
            .context(format!("Failed to connect to PostgreSQL at {}", remote_addr))?;

        // Get the local address from the connection's underlying socket
        // Note: tokio-postgres doesn't expose local_addr directly, so we parse from remote_addr
        let local_addr: SocketAddr = format!("{}:0", remote_addr.split(':').next().unwrap_or("127.0.0.1"))
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap());

        info!("PostgreSQL client {} connected to {}", client_id, remote_addr);

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] PostgreSQL client {} connected",
            client_id
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn connection task
        let status_tx_clone = status_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("PostgreSQL client {} connection error: {}", client_id, e);
                let _ = status_tx_clone.send(format!(
                    "[CLIENT] PostgreSQL client {} connection error: {}",
                    client_id, e
                ));
            }
        });

        // Send connected event to LLM
        let client_arc = Arc::new(tokio::sync::Mutex::new(client));

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::postgresql::actions::PostgresqlClientProtocol::new());
            let event = Event::new(
                &POSTGRESQL_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "database": database,
                    "user": user,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            let client_arc_clone = client_arc.clone();
            let llm_client_clone = llm_client.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client_clone,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                )
                .await
                {
                    Ok(ClientLlmResult {
                        actions,
                        memory_updates,
                    }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            match protocol.execute_action(action) {
                                Ok(
                                    crate::llm::actions::client_trait::ClientActionResult::Custom {
                                        name,
                                        data,
                                    },
                                ) if name == "pg_query" => {
                                    if let Some(query) = data.get("query").and_then(|v| v.as_str()) {
                                        trace!("PostgreSQL client {} executing: {}", client_id, query);

                                        let pg_client = client_arc_clone.lock().await;
                                        match pg_client.query(query, &[]).await {
                                            Ok(rows) => {
                                                // Convert rows to JSON
                                                let result = rows
                                                    .iter()
                                                    .map(|row| {
                                                        let mut obj =
                                                            serde_json::Map::new();
                                                        for (idx, col) in
                                                            row.columns().iter().enumerate()
                                                        {
                                                            let value: Option<String> =
                                                                row.get(idx);
                                                            obj.insert(
                                                                col.name().to_string(),
                                                                serde_json::json!(value),
                                                            );
                                                        }
                                                        serde_json::Value::Object(obj)
                                                    })
                                                    .collect::<Vec<_>>();

                                                info!(
                                                    "PostgreSQL client {} query returned {} rows",
                                                    client_id,
                                                    result.len()
                                                );

                                                // Call LLM with query result
                                                let event = Event::new(
                                                    &POSTGRESQL_CLIENT_QUERY_RESULT_EVENT,
                                                    serde_json::json!({
                                                        "query": query,
                                                        "rows": result,
                                                        "row_count": result.len(),
                                                    }),
                                                );

                                                let memory = app_state_clone
                                                    .get_memory_for_client(client_id)
                                                    .await
                                                    .unwrap_or_default();

                                                if let Ok(ClientLlmResult {
                                                    actions: _,
                                                    memory_updates,
                                                }) = call_llm_for_client(
                                                    &llm_client_clone,
                                                    &app_state_clone,
                                                    client_id.to_string(),
                                                    &instruction,
                                                    &memory,
                                                    Some(&event),
                                                    protocol.as_ref(),
                                                    &status_tx_clone,
                                                )
                                                .await
                                                {
                                                    if let Some(mem) = memory_updates {
                                                        app_state_clone
                                                            .set_memory_for_client(client_id, mem)
                                                            .await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!(
                                                    "PostgreSQL client {} query error: {}",
                                                    client_id, e
                                                );
                                                let _ = status_tx_clone.send(format!(
                                                    "[CLIENT] PostgreSQL client {} query error: {}",
                                                    client_id, e
                                                ));
                                            }
                                        }
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                    info!("PostgreSQL client {} disconnecting", client_id);
                                    app_state_clone
                                        .update_client_status(
                                            client_id,
                                            ClientStatus::Disconnected,
                                        )
                                        .await;
                                    let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for PostgreSQL client {}: {}", client_id, e);
                    }
                }
            });
        }

        Ok(local_addr)
    }
}
