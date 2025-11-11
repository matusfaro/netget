//! MySQL client implementation
pub mod actions;

pub use actions::MysqlClientProtocol;

use anyhow::{Context, Result};
use mysql_async::{prelude::*, Conn, OptsBuilder, Row};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::client::mysql::actions::{
    MYSQL_CLIENT_CONNECTED_EVENT, MYSQL_CLIENT_RESULT_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// MySQL client that connects to a MySQL server
pub struct MysqlClient;

impl MysqlClient {
    /// Connect to a MySQL server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup parameters
        let username = startup_params
            .as_ref()
            .map(|p| p.get_string("username"))
            .unwrap_or_else(|| "root".to_string());
        let password = startup_params
            .as_ref()
            .map(|p| p.get_string("password"))
            .unwrap_or_else(|| "".to_string());
        let database: Option<String> = startup_params.as_ref().map(|p| p.get_string("database"));

        // Parse remote_addr to get host and port
        let (host, port) = if let Some((h, p)) = remote_addr.split_once(':') {
            (h.to_string(), p.parse::<u16>().context("Invalid port")?)
        } else {
            (remote_addr.clone(), 3306)
        };

        // Build MySQL connection options
        let mut opts_builder = OptsBuilder::default()
            .ip_or_hostname(&host)
            .tcp_port(port)
            .user(Some(&username))
            .pass(Some(&password));

        if let Some(db) = database.as_ref() {
            opts_builder = opts_builder.db_name(Some(db.as_str()));
        }

        // Connect to MySQL server
        let conn = Conn::new(opts_builder)
            .await
            .context(format!("Failed to connect to MySQL at {}", remote_addr))?;

        info!("MySQL client {} connected to {}", client_id, remote_addr);

        // For SocketAddr, we'll create a fake one since mysql_async doesn't expose the actual socket
        // We'll parse the remote_addr to create a SocketAddr
        let socket_addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .context("Failed to parse socket address")?;

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] MySQL client {} connected to {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Wrap connection in Arc<Mutex> for shared access
        let conn_arc = Arc::new(Mutex::new(conn));

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::mysql::actions::MysqlClientProtocol::new());
            let event = Event::new(
                &MYSQL_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            let conn_clone = conn_arc.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client,
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
                            if let Err(e) = Self::execute_llm_action(
                                client_id,
                                action,
                                &protocol,
                                &conn_clone,
                                &app_state_clone,
                                &llm_client,
                                &status_tx_clone,
                            )
                            .await
                            {
                                error!("Error executing MySQL action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for MySQL client {}: {}", client_id, e);
                    }
                }
            });
        }

        Ok(socket_addr)
    }

    /// Execute an action from the LLM
    async fn execute_llm_action(
        client_id: ClientId,
        action: serde_json::Value,
        protocol: &Arc<MysqlClientProtocol>,
        conn: &Arc<Mutex<Conn>>,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match protocol.execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }
                if name == "mysql_query" =>
            {
                if let Some(query_str) = data.get("query").and_then(|v| v.as_str()) {
                    trace!("MySQL client {} executing query: {}", client_id, query_str);

                    let mut conn_guard = conn.lock().await;

                    // Execute query
                    let result: Result<Vec<Row>> = conn_guard
                        .query(query_str)
                        .await
                        .context("Failed to execute query");

                    drop(conn_guard);

                    match result {
                        Ok(rows) => {
                            // Convert rows to JSON
                            let json_rows: Vec<serde_json::Value> = rows
                                .iter()
                                .map(|row| {
                                    let mut obj = serde_json::Map::new();
                                    for (idx, col) in row.columns_ref().iter().enumerate() {
                                        let value = match row.as_ref(idx) {
                                            Some(mysql_async::Value::NULL) => {
                                                serde_json::Value::Null
                                            }
                                            Some(mysql_async::Value::Bytes(b)) => {
                                                serde_json::Value::String(
                                                    String::from_utf8_lossy(b).to_string(),
                                                )
                                            }
                                            Some(mysql_async::Value::Int(i)) => {
                                                serde_json::Value::Number((*i).into())
                                            }
                                            Some(mysql_async::Value::UInt(u)) => {
                                                serde_json::Value::Number((*u).into())
                                            }
                                            Some(mysql_async::Value::Float(f)) => {
                                                serde_json::Number::from_f64(*f as f64)
                                                    .map(serde_json::Value::Number)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                            Some(mysql_async::Value::Double(d)) => {
                                                serde_json::Number::from_f64(*d)
                                                    .map(serde_json::Value::Number)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                            Some(mysql_async::Value::Date(
                                                y,
                                                m,
                                                d,
                                                h,
                                                min,
                                                s,
                                                us,
                                            )) => serde_json::Value::String(format!(
                                                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                                                y, m, d, h, min, s, us
                                            )),
                                            Some(mysql_async::Value::Time(
                                                is_neg,
                                                d,
                                                h,
                                                m,
                                                s,
                                                us,
                                            )) => {
                                                let sign = if *is_neg { "-" } else { "" };
                                                serde_json::Value::String(format!(
                                                    "{}{} {:02}:{:02}:{:02}.{:06}",
                                                    sign, d, h, m, s, us
                                                ))
                                            }
                                            None => serde_json::Value::Null,
                                        };
                                        obj.insert(col.name_str().to_string(), value);
                                    }
                                    serde_json::Value::Object(obj)
                                })
                                .collect();

                            info!(
                                "MySQL client {} query returned {} rows",
                                client_id,
                                json_rows.len()
                            );

                            // Call LLM with result
                            if let Some(instruction) =
                                app_state.get_instruction_for_client(client_id).await
                            {
                                let event = Event::new(
                                    &MYSQL_CLIENT_RESULT_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "result": json_rows,
                                        "row_count": json_rows.len(),
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

                                        // Execute new actions (simple non-recursive execution)
                                        // For MySQL, queries are typically one-shot responses
                                        // More complex flows can be handled by the LLM in the instruction
                                        for new_action in actions {
                                            match protocol.execute_action(new_action) {
                                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                    info!("MySQL client {} disconnecting after query result", client_id);
                                                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                                                    let _ = status_tx.send(format!("[CLIENT] MySQL client {} disconnected", client_id));
                                                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                                                }
                                                _ => {
                                                    // Other actions would require full recursion
                                                    // For now, MySQL client handles simple query-response cycles
                                                    trace!("MySQL client {} received additional action after query", client_id);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("LLM error for MySQL client {}: {}", client_id, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("MySQL client {} query error: {}", client_id, e);
                            app_state
                                .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                                .await;
                            let _ = status_tx.send("__UPDATE_UI__".to_string());
                        }
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                info!("MySQL client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send(format!("[CLIENT] MySQL client {} disconnected", client_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            crate::llm::actions::client_trait::ClientActionResult::WaitForMore => {
                trace!("MySQL client {} waiting for more data", client_id);
            }
            _ => {}
        }
        Ok(())
    }
}
