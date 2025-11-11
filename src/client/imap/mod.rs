//! IMAP client implementation
pub mod actions;

pub use actions::ImapClientProtocol;

use anyhow::{Context, Result};
use async_imap::{Client as ImapAsyncClient, Session};
use futures::StreamExt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::imap::actions::IMAP_CLIENT_CONNECTED_EVENT;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// IMAP client that connects to an IMAP server
pub struct ImapClient;

impl ImapClient {
    /// Connect to an IMAP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract authentication credentials from startup params
        let (username, password) = if let Some(params) = startup_params {
            let username = params.get_string("username");
            let password = params.get_string("password");
            (username, password)
        } else {
            return Err(anyhow::anyhow!("IMAP client requires startup parameters: username, password"));
        };

        info!(
            "IMAP client {} connecting to {} (user: {})",
            client_id, remote_addr, username
        );

        // Connect to IMAP server via TCP
        let tcp_stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to IMAP at {}", remote_addr))?;

        let local_addr = tcp_stream.local_addr()?;

        // Convert tokio stream to futures-compatible stream
        let compat_stream = tcp_stream.compat();

        // Create IMAP client
        let imap_client = ImapAsyncClient::new(compat_stream);

        // Authenticate
        let session = match imap_client.login(&username, &password).await {
            Ok(session) => {
                info!("IMAP client {} authenticated successfully", client_id);
                session
            }
            Err((e, _)) => {
                error!("IMAP client {} authentication failed: {}", client_id, e);
                return Err(anyhow::anyhow!("IMAP login failed: {}", e));
            }
        };

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        console_info!(status_tx, "[CLIENT] IMAP client {} connected and authenticated", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Get initial instruction
        let instruction = if let Some(inst) = app_state.get_instruction_for_client(client_id).await {
            inst
        } else {
            return Err(anyhow::anyhow!("No instruction for IMAP client"));
        };

        // Spawn task to handle IMAP session with LLM integration
        let session_arc = Arc::new(Mutex::new(session));
        let protocol = Arc::new(actions::ImapClientProtocol::new());

        tokio::spawn(async move {
            // Call LLM with connected event
            let event = Event::new(
                &IMAP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "capabilities": vec!["IMAP4rev1"],
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

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

                    // Execute initial actions from LLM
                    for action in actions {
                        if let Err(e) = Self::execute_imap_action(
                            client_id,
                            &session_arc,
                            &protocol,
                            &llm_client,
                            &app_state,
                            &status_tx,
                            action,
                        )
                        .await
                        {
                            error!("Failed to execute IMAP action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for IMAP client {}: {}", client_id, e);
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute a single IMAP action and potentially trigger more LLM calls
    async fn execute_imap_action(
        client_id: ClientId,
        session: &Arc<Mutex<Session<tokio_util::compat::Compat<TcpStream>>>>,
        protocol: &Arc<ImapClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        action: serde_json::Value,
    ) -> Result<()> {
        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } => {
                Self::handle_custom_action(
                    client_id,
                    session,
                    &name,
                    &data,
                    protocol,
                    llm_client,
                    app_state,
                    status_tx,
                )
                .await?;
            }
            ClientActionResult::Disconnect => {
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                console_info!(status_tx, "__UPDATE_UI__");
            }
            ClientActionResult::WaitForMore => {
                debug!("IMAP client {} waiting for more events", client_id);
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle custom IMAP actions
    async fn handle_custom_action(
        client_id: ClientId,
        session: &Arc<Mutex<Session<tokio_util::compat::Compat<TcpStream>>>>,
        action_name: &str,
        action_data: &serde_json::Value,
        _protocol: &Arc<ImapClientProtocol>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        _status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match action_name {
            "select_mailbox" => {
                let mailbox = action_data
                    .get("mailbox")
                    .and_then(|v| v.as_str())
                    .context("Missing mailbox")?;

                trace!("IMAP client {} selecting mailbox: {}", client_id, mailbox);

                let mut session_guard = session.lock().await;
                let mailbox_info = session_guard
                    .select(mailbox)
                    .await
                    .context("Failed to select mailbox")?;

                let exists = mailbox_info.exists;
                let recent = mailbox_info.recent;

                info!(
                    "IMAP client {} selected mailbox '{}' ({} messages, {} recent)",
                    client_id, mailbox, exists, recent
                );

                drop(session_guard);

                // Note: Follow-up LLM call with mailbox selected event could be added here
                // but requires careful lifetime management with the session
                debug!("IMAP client {} completed select_mailbox action", client_id);
            }
            "search_messages" => {
                let criteria = action_data
                    .get("criteria")
                    .and_then(|v| v.as_str())
                    .context("Missing criteria")?;

                trace!("IMAP client {} searching: {}", client_id, criteria);

                let mut session_guard = session.lock().await;
                let message_ids = session_guard
                    .search(criteria)
                    .await
                    .context("Failed to search messages")?;

                let id_list: Vec<u32> = message_ids.iter().cloned().collect();
                info!(
                    "IMAP client {} found {} messages matching '{}'",
                    client_id,
                    id_list.len(),
                    criteria
                );

                drop(session_guard);

                // Note: Follow-up LLM call with search results could be added here
                debug!("IMAP client {} completed search_messages action, found {} messages", client_id, id_list.len());
            }
            "fetch_message" => {
                let message_id = action_data
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .context("Missing message_id")?;

                let parts = action_data
                    .get("parts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("BODY[]");

                trace!(
                    "IMAP client {} fetching message {}: {}",
                    client_id,
                    message_id,
                    parts
                );

                let mut session_guard = session.lock().await;
                let messages = session_guard
                    .fetch(message_id, parts)
                    .await
                    .context("Failed to fetch message")?;

                // Collect messages from stream
                let mut message_list = vec![];
                let mut messages = Box::pin(messages);
                while let Some(Ok(fetch)) = messages.next().await {
                    message_list.push(fetch);
                }

                // Drop the stream first to release the borrow
                drop(messages);
                drop(session_guard);

                // Process fetched messages
                for fetch in message_list {
                    let _body = fetch
                        .body()
                        .map(|b| String::from_utf8_lossy(b).to_string())
                        .unwrap_or_default();

                    let envelope = fetch.envelope();
                    let subject = envelope
                        .and_then(|e| e.subject.as_ref())
                        .and_then(|s| std::str::from_utf8(s).ok())
                        .unwrap_or("(no subject)");

                    let from = envelope
                        .and_then(|e| e.from.as_ref())
                        .and_then(|addrs| addrs.first())
                        .and_then(|addr| addr.mailbox.as_ref())
                        .and_then(|m| std::str::from_utf8(m).ok())
                        .unwrap_or("(unknown)");

                    info!(
                        "IMAP client {} fetched message {}: {} from {}",
                        client_id, message_id, subject, from
                    );

                    // Note: Follow-up LLM call with fetched message could be added here
                    debug!("IMAP client {} completed fetch_message action", client_id);
                }
            }
            "mark_as_read" => {
                let message_id = action_data
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .context("Missing message_id")?;

                trace!("IMAP client {} marking message {} as read", client_id, message_id);

                let mut session_guard = session.lock().await;
                let _ = session_guard
                    .store(message_id, "+FLAGS (\\Seen)")
                    .await
                    .context("Failed to mark message as read")?;

                info!("IMAP client {} marked message {} as read", client_id, message_id);
            }
            "mark_as_unread" => {
                let message_id = action_data
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .context("Missing message_id")?;

                trace!("IMAP client {} marking message {} as unread", client_id, message_id);

                let mut session_guard = session.lock().await;
                let _ = session_guard
                    .store(message_id, "-FLAGS (\\Seen)")
                    .await
                    .context("Failed to mark message as unread")?;

                info!("IMAP client {} marked message {} as unread", client_id, message_id);
            }
            "delete_message" => {
                let message_id = action_data
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .context("Missing message_id")?;

                trace!("IMAP client {} deleting message {}", client_id, message_id);

                let mut session_guard = session.lock().await;
                // Mark for deletion
                let _ = session_guard
                    .store(message_id, "+FLAGS (\\Deleted)")
                    .await
                    .context("Failed to mark message for deletion")?;

                // Expunge
                let _ = session_guard
                    .expunge()
                    .await
                    .context("Failed to expunge deleted messages")?;

                info!("IMAP client {} deleted message {}", client_id, message_id);
            }
            "list_mailboxes" => {
                trace!("IMAP client {} listing mailboxes", client_id);

                let mut session_guard = session.lock().await;
                let mailboxes = session_guard
                    .list(Some(""), Some("*"))
                    .await
                    .context("Failed to list mailboxes")?;

                // Collect mailboxes from stream
                let mut mailbox_list = vec![];
                let mut mailboxes = Box::pin(mailboxes);
                while let Some(Ok(mailbox)) = mailboxes.next().await {
                    mailbox_list.push(mailbox);
                }

                let mailbox_names: Vec<String> = mailbox_list
                    .iter()
                    .map(|m| m.name().to_string())
                    .collect();

                info!(
                    "IMAP client {} listed {} mailboxes",
                    client_id,
                    mailbox_names.len()
                );

                debug!("Mailboxes: {:?}", mailbox_names);
            }
            _ => {
                debug!("Unknown IMAP action: {}", action_name);
            }
        }

        Ok(())
    }
}
