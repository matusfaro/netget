//! MySQL client implementation
pub mod actions;

pub use actions::MysqlClientProtocol;

use anyhow::{Context, Result};
use mysql_async::{prelude::*, Conn, OptsBuilder, Row};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::client::llm_budget::call_llm_for_client;
use crate::client::mysql::actions::{
    MYSQL_CLIENT_CONNECTED_EVENT, MYSQL_CLIENT_RESULT_RECEIVED_EVENT,
};
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client_handles::{ClientCommand, ClientSendOutcome};
use crate::state::{ClientId, ClientStatus};

/// What one executed action did to the MySQL connection.
///
/// Returned by [`MysqlClient::apply_action`], which is the single place a `mysql_query`
/// reaches the server — the connected-event LLM path and injected dashboard commands both
/// go through it.
enum Applied {
    /// A query ran; the rows are the JSON result set reported back to the model.
    Query {
        query: String,
        rows: Vec<serde_json::Value>,
    },
    /// The session should end.
    Disconnect,
    /// The action executed but touched the connection in no way.
    Nothing(&'static str),
}

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
            .transpose()?
            .unwrap_or_else(|| "root".to_string());
        let password = startup_params
            .as_ref()
            .map(|p| p.get_string("password"))
            .transpose()?
            .unwrap_or_else(|| "".to_string());
        let database: Option<String> = startup_params
            .as_ref()
            .map(|p| p.get_string("database"))
            .transpose()?;

        // Parse remote_addr to get host and port
        let (host, port) = if let Some((h, p)) = remote_addr.split_once(':') {
            (h.to_string(), p.parse::<u16>().context("Invalid port")?)
        } else {
            (remote_addr.clone(), 3306)
        };

        // Build MySQL connection options.
        //
        // `max_allowed_packet` / `wait_timeout` / `prefer_socket` are pinned deliberately.
        // Left unset, `Conn::new` asks the server for `@@max_allowed_packet`,
        // `@@wait_timeout` and `@@socket` and feeds each answer to `from_value`, which
        // **panics** rather than erroring when the value is not the type it expects. The
        // server at the other end of a NetGet client is frequently one whose replies a model
        // composed, so that panic is reachable from an ordinary LLM answer. Supplying the
        // values here means the settings query is never issued. `prefer_socket(false)` is
        // right on its own terms too: the user gave us a TCP address to use.
        let mut opts_builder = OptsBuilder::default()
            .ip_or_hostname(&host)
            .tcp_port(port)
            .user(Some(&username))
            .pass(Some(&password))
            .max_allowed_packet(Some(16 * 1024 * 1024))
            .wait_timeout(Some(28800))
            .prefer_socket(false);

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

        // Wrap connection in Arc<Mutex> for shared access. The command loop below holds a
        // clone, which is also what keeps the connection alive: before it existed, a client
        // with no instruction dropped its only `Conn` the moment this function returned.
        let conn_arc = Arc::new(Mutex::new(conn));
        let protocol = Arc::new(MysqlClientProtocol::new());

        // Command channel for injected actions (the dashboard's [ send ]).
        // Registered BEFORE the connected-event LLM call, which a manual `*` rule can park
        // for minutes — the operator must be able to reach the client while it waits.
        let command_rx =
            crate::client::command_support::register_command_channel(&app_state, client_id).await;
        let cmd_task = tokio::spawn(Self::command_loop(
            command_rx,
            protocol.clone(),
            conn_arc.clone(),
            client_id,
            llm_client.clone(),
            app_state.clone(),
            status_tx.clone(),
        ));
        app_state.register_client_task(client_id, cmd_task).await;

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
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

            let protocol_clone = protocol.clone();
            let conn_clone = conn_arc.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            let task_registrar = app_state.clone();
            let handle = tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol_clone.as_ref(),
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
                                &protocol_clone,
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
            task_registrar.register_client_task(client_id, handle).await;
        }

        Ok(socket_addr)
    }

    /// Drain injected commands until the channel closes (the client was removed or stopped)
    /// or an injected `disconnect` ends the session.
    ///
    /// The generic `command_support::handle_stream_client_command` cannot serve this client:
    /// there is no socket NetGet owns — `mysql_async` does — so `execute_query` yields
    /// `ClientActionResult::Custom` and the effect goes through the shared
    /// [`Self::apply_action`], the same function the LLM path uses.
    #[allow(clippy::too_many_arguments)]
    async fn command_loop(
        mut command_rx: mpsc::Receiver<ClientCommand>,
        protocol: Arc<MysqlClientProtocol>,
        conn: Arc<Mutex<Conn>>,
        client_id: ClientId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::protocol_trait::Protocol;
        use crate::state::AccessLogOwner;

        while let Some(command) = command_rx.recv().await {
            let action = command.action.clone();

            let mut follow_up: Option<Vec<serde_json::Value>> = None;
            let outcome = match protocol.execute_action(action.clone()) {
                Err(e) => Ok(ClientSendOutcome::Rejected {
                    error: e.to_string(),
                }),
                Ok(result) => {
                    match Self::apply_action(result, &conn, client_id, &app_state, &status_tx).await
                    {
                        Err(e) => Err(e),
                        Ok(Applied::Disconnect) => Ok(ClientSendOutcome::Disconnected),
                        Ok(Applied::Nothing(what)) => Ok(ClientSendOutcome::Executed {
                            detail: what.to_string(),
                        }),
                        Ok(Applied::Query { query, rows }) => {
                            let detail = format!(
                                "query executed by mysql_async ({} rows); \
                                 no byte count — the driver owns the socket: {}",
                                rows.len(),
                                crate::utils::truncate::truncate_for_log(&query, 80)
                            );
                            follow_up = Some(rows);
                            Ok(ClientSendOutcome::Executed { detail })
                        }
                    }
                }
            };

            let outcome_json = match &outcome {
                Ok(outcome) => serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null),
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            app_state
                .record_access_log(
                    AccessLogOwner::Client(client_id.as_u32()),
                    protocol.protocol_name(),
                    None,
                    "injected_action",
                    action,
                    vec![outcome_json],
                )
                .await;

            let disconnect = matches!(outcome, Ok(ClientSendOutcome::Disconnected));
            if let Err(e) = &outcome {
                error!("MySQL client {} injected action failed: {}", client_id, e);
                let _ = status_tx.send(format!(
                    "[WARN] Client {} injected action failed: {}",
                    client_id, e
                ));
            }
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            crate::client::command_support::reply(command, outcome);

            if disconnect {
                Self::mark_disconnected(client_id, &app_state, &status_tx).await;
                break;
            }

            // The model must see the result of a user-injected query exactly as it sees the
            // result of one it asked for. Done *after* the reply so the dashboard is not
            // blocked for the length of an LLM round-trip.
            if let Some(rows) = follow_up {
                Self::report_query_result(
                    client_id,
                    rows,
                    &protocol,
                    &app_state,
                    &llm_client,
                    &status_tx,
                )
                .await;
            }
        }

        // Every exit path lands here: drop the command handle so the dashboard stops
        // offering [ send ] on a client whose loop is gone.
        app_state.remove_client_handle(client_id).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());
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
        match Self::apply_action(
            protocol.execute_action(action)?,
            conn,
            client_id,
            app_state,
            status_tx,
        )
        .await?
        {
            Applied::Query { rows, .. } => {
                Self::report_query_result(
                    client_id, rows, protocol, app_state, llm_client, status_tx,
                )
                .await;
            }
            Applied::Disconnect => {
                info!("MySQL client {} disconnecting", client_id);
                Self::mark_disconnected(client_id, app_state, status_tx).await;
            }
            Applied::Nothing(what) => {
                trace!("MySQL client {} action had no effect: {}", client_id, what);
            }
        }
        Ok(())
    }

    /// Put one executed action on the connection. Shared by the connected-event LLM path and
    /// injected commands so the query path exists exactly once.
    async fn apply_action(
        result: ClientActionResult,
        conn: &Arc<Mutex<Conn>>,
        client_id: ClientId,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Applied> {
        match result {
            ClientActionResult::Custom { name, data } if name == "mysql_query" => {
                let query_str = data
                    .get("query")
                    .and_then(|v| v.as_str())
                    .context("Missing 'query' in mysql_query action data")?
                    .to_string();

                trace!("MySQL client {} executing query: {}", client_id, query_str);

                let rows: Result<Vec<Row>> = {
                    let mut conn_guard = conn.lock().await;
                    conn_guard
                        .query(&query_str)
                        .await
                        .context("Failed to execute query")
                };

                match rows {
                    Ok(rows) => {
                        let json_rows = rows_to_json(&rows);
                        info!(
                            "MySQL client {} query returned {} rows",
                            client_id,
                            json_rows.len()
                        );
                        Ok(Applied::Query {
                            query: query_str,
                            rows: json_rows,
                        })
                    }
                    Err(e) => {
                        error!("MySQL client {} query error: {}", client_id, e);
                        app_state
                            .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        Err(e)
                    }
                }
            }
            ClientActionResult::Custom { name, .. } => {
                trace!(
                    "MySQL client {} ignoring custom result '{}'",
                    client_id,
                    name
                );
                Ok(Applied::Nothing("custom result not handled by this client"))
            }
            ClientActionResult::Disconnect => Ok(Applied::Disconnect),
            ClientActionResult::WaitForMore => Ok(Applied::Nothing("wait_for_more")),
            ClientActionResult::NoAction => Ok(Applied::Nothing("no_action")),
            ClientActionResult::SendData(_) => Ok(Applied::Nothing(
                "send_data is not meaningful for a mysql_async connection",
            )),
            ClientActionResult::Multiple(_) => Ok(Applied::Nothing(
                "multiple results not handled by this client",
            )),
        }
    }

    /// Raise `mysql_result_received` with a query's rows and run whatever the model answers.
    async fn report_query_result(
        client_id: ClientId,
        json_rows: Vec<serde_json::Value>,
        protocol: &Arc<MysqlClientProtocol>,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let Some(instruction) = app_state.get_instruction_for_client(client_id).await else {
            return;
        };

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

                // Execute new actions (simple non-recursive execution).
                // For MySQL, queries are typically one-shot responses; more complex flows
                // can be handled by the LLM in the instruction.
                for new_action in actions {
                    match protocol.execute_action(new_action) {
                        Ok(ClientActionResult::Disconnect) => {
                            info!(
                                "MySQL client {} disconnecting after query result",
                                client_id
                            );
                            Self::mark_disconnected(client_id, app_state, status_tx).await;
                        }
                        _ => {
                            trace!(
                                "MySQL client {} received additional action after query",
                                client_id
                            );
                        }
                    }
                }
            }
            Err(e) => {
                error!("LLM error for MySQL client {}: {}", client_id, e);
            }
        }
    }

    async fn mark_disconnected(
        client_id: ClientId,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        app_state
            .update_client_status(client_id, ClientStatus::Disconnected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] MySQL client {} disconnected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());
    }
}

/// Convert a MySQL result set into the JSON rows reported to the model.
fn rows_to_json(rows: &[Row]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (idx, col) in row.columns_ref().iter().enumerate() {
                let value = match row.as_ref(idx) {
                    Some(mysql_async::Value::NULL) => serde_json::Value::Null,
                    Some(mysql_async::Value::Bytes(b)) => {
                        serde_json::Value::String(String::from_utf8_lossy(b).to_string())
                    }
                    Some(mysql_async::Value::Int(i)) => serde_json::Value::Number((*i).into()),
                    Some(mysql_async::Value::UInt(u)) => serde_json::Value::Number((*u).into()),
                    Some(mysql_async::Value::Float(f)) => serde_json::Number::from_f64(*f as f64)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Some(mysql_async::Value::Double(d)) => serde_json::Number::from_f64(*d)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Some(mysql_async::Value::Date(y, m, d, h, min, s, us)) => {
                        serde_json::Value::String(format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                            y, m, d, h, min, s, us
                        ))
                    }
                    Some(mysql_async::Value::Time(is_neg, d, h, m, s, us)) => {
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
        .collect()
}
