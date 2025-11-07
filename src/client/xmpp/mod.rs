//! XMPP (Jabber) client implementation
pub mod actions;

pub use actions::XmppClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::xmpp::actions::{
    XMPP_CLIENT_CONNECTED_EVENT,
    XMPP_CLIENT_MESSAGE_RECEIVED_EVENT,
    XMPP_CLIENT_PRESENCE_RECEIVED_EVENT,
};

use tokio_xmpp::{Client as XmppClient, Event as XmppEvent};
use tokio_xmpp::jid::Jid;
use xmpp_parsers::{
    message::{Lang, Message, MessageType},
    presence::{Presence, Show as PresenceShow, Type as PresenceType},
};
use futures::StreamExt;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_events: Vec<XmppEvent>,
    memory: String,
}

/// XMPP client that connects to an XMPP/Jabber server
pub struct XmppClientConnection;

impl XmppClientConnection {
    /// Connect to an XMPP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse JID and password from remote_addr or get from startup params
        let (jid, password, _server_addr) = Self::parse_connection_info(&remote_addr, &app_state, client_id).await?;

        info!("XMPP client {} connecting as {}", client_id, jid);
        let _ = status_tx.send(format!("[CLIENT] XMPP client {} connecting...", client_id));

        // Create XMPP client
        let mut xmpp_client = XmppClient::new(jid.clone(), password);

        // Store JID in app state
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "jid".to_string(),
                serde_json::json!(jid.to_string()),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] XMPP client {} connected as {}", client_id, jid));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_events: Vec::new(),
            memory: String::new(),
        }));

        // Call LLM with connected event
        let instruction = app_state.get_instruction_for_client(client_id).await.unwrap_or_default();
        let protocol = Arc::new(crate::client::xmpp::actions::XmppClientProtocol::new());
        let event = Event::new(
            &XMPP_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "jid": jid.to_string(),
            }),
        );

        // Create channel for sending stanzas to the event loop
        let (stanza_tx, mut stanza_rx) = mpsc::unbounded_channel();
        let stanza_tx = Arc::new(stanza_tx);

        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &client_data.lock().await.memory,
            Some(&event),
            protocol.as_ref(),
            &status_tx,
        ).await {
            Ok(ClientLlmResult { actions, memory_updates }) => {
                if let Some(mem) = memory_updates {
                    client_data.lock().await.memory = mem;
                }

                // Execute initial actions
                for action in actions {
                    Self::execute_action_result(
                        action,
                        protocol.clone(),
                        stanza_tx.clone(),
                        client_id,
                        &status_tx,
                    ).await;
                }
            }
            Err(e) => {
                error!("LLM error on XMPP connect for client {}: {}", client_id, e);
            }
        }

        // Spawn event loop that handles both sending and receiving
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle outgoing stanzas
                    Some(stanza) = stanza_rx.recv() => {
                        if let Err(e) = xmpp_client.send_stanza(stanza).await {
                            error!("Failed to send XMPP stanza: {}", e);
                            let _ = status_tx.send(format!("[ERROR] Failed to send XMPP stanza: {}", e));
                        }
                    }
                    // Handle incoming events
                    Some(xmpp_event) = xmpp_client.next() => {
                trace!("XMPP client {} received event: {:?}", client_id, xmpp_event);

                // Handle event with LLM
                let mut client_data_lock = client_data.lock().await;

                match client_data_lock.state {
                    ConnectionState::Idle => {
                        // Process immediately
                        client_data_lock.state = ConnectionState::Processing;
                        drop(client_data_lock);

                        Self::handle_xmpp_event(
                            xmpp_event,
                            &llm_client,
                            &app_state,
                            client_id,
                            &client_data,
                            protocol.clone(),
                            stanza_tx.clone(),
                            &status_tx,
                        ).await;

                        // Process queued events
                        let mut client_data_lock = client_data.lock().await;
                        let queued = std::mem::take(&mut client_data_lock.queued_events);
                        client_data_lock.state = ConnectionState::Idle;
                        drop(client_data_lock);

                        for queued_event in queued {
                            Self::handle_xmpp_event(
                                queued_event,
                                &llm_client,
                                &app_state,
                                client_id,
                                &client_data,
                                protocol.clone(),
                                stanza_tx.clone(),
                                &status_tx,
                            ).await;
                        }
                    }
                    ConnectionState::Processing => {
                        // Queue event
                        client_data_lock.queued_events.push(xmpp_event);
                        client_data_lock.state = ConnectionState::Accumulating;
                    }
                    ConnectionState::Accumulating => {
                        // Continue queuing
                        client_data_lock.queued_events.push(xmpp_event);
                    }
                }
                    }
                    // No more events - connection closed
                    else => {
                        info!("XMPP client {} connection closed", client_id);
                        break;
                    }
                }
            }

            info!("XMPP client {} disconnected", client_id);
            app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
            let _ = status_tx.send(format!("[CLIENT] XMPP client {} disconnected", client_id));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        });

        // Return dummy local address (XMPP handles this internally)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Parse connection information from remote_addr and startup params
    async fn parse_connection_info(
        remote_addr: &str,
        app_state: &Arc<AppState>,
        client_id: ClientId,
    ) -> Result<(Jid, String, String)> {
        // Try to get from startup params first
        let params = app_state.with_client_mut(client_id, |client| {
            let jid = client.get_protocol_field("jid")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let pass = client.get_protocol_field("password")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (jid, pass)
        }).await;

        let (jid_str, password) = params.unwrap_or((None, None));

        let (jid_str, password) = match (jid_str, password) {
            (Some(j), Some(p)) => (j, p),
            _ => {
                // Parse from remote_addr: "user@domain@password"
                let parts: Vec<&str> = remote_addr.split('@').collect();
                if parts.len() < 3 {
                    return Err(anyhow::anyhow!(
                        "Invalid XMPP address format. Expected: user@domain@password or set jid/password in startup params"
                    ));
                }
                let user = parts[0];
                let domain = parts[1];
                let password = parts[2..].join("@"); // In case password contains @

                (format!("{}@{}", user, domain), password)
            }
        };

        let jid: Jid = jid_str.parse()
            .context("Invalid JID format")?;

        // Server address is typically the domain from JID
        let server_addr = remote_addr.split('@').nth(1)
            .and_then(|s| s.split(':').next())
            .unwrap_or("localhost")
            .to_string();

        Ok((jid, password, server_addr))
    }

    /// Handle an XMPP event with LLM
    async fn handle_xmpp_event(
        xmpp_event: XmppEvent,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        client_id: ClientId,
        client_data: &Arc<Mutex<ClientData>>,
        protocol: Arc<XmppClientProtocol>,
        stanza_tx: Arc<mpsc::UnboundedSender<tokio_xmpp::Stanza>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let event_opt = match xmpp_event {
            XmppEvent::Online { .. } => {
                debug!("XMPP client {} online", client_id);
                None // Already handled in connect
            }
            XmppEvent::Disconnected(_e) => {
                warn!("XMPP client {} disconnected", client_id);
                None
            }
            XmppEvent::Stanza(stanza) => {
                // Match on stanza type to avoid clone
                use tokio_xmpp::Stanza;
                match stanza {
                    Stanza::Message(msg) => {
                        let from = msg.from.as_ref().map(|j| j.to_string()).unwrap_or_default();
                        let to = msg.to.as_ref().map(|j| j.to_string()).unwrap_or_default();
                        let body = msg.bodies.get(&Lang::default()).cloned().unwrap_or_default();
                        let msg_type = format!("{:?}", msg.type_);

                        info!("XMPP client {} received message from {}: {}", client_id, from, body);

                        Some(Event::new(
                            &XMPP_CLIENT_MESSAGE_RECEIVED_EVENT,
                            serde_json::json!({
                                "from": from,
                                "to": to,
                                "body": body,
                                "message_type": msg_type,
                            }),
                        ))
                    }
                    Stanza::Presence(presence) => {
                        let from = presence.from.as_ref().map(|j| j.to_string()).unwrap_or_default();
                        let presence_type = format!("{:?}", presence.type_);
                        let show = presence.show.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default();
                        let status = presence.statuses.get(&Lang::default()).cloned().unwrap_or_default();

                        debug!("XMPP client {} received presence from {}: {:?}", client_id, from, presence_type);

                        Some(Event::new(
                            &XMPP_CLIENT_PRESENCE_RECEIVED_EVENT,
                            serde_json::json!({
                                "from": from,
                                "presence_type": presence_type,
                                "show": show,
                                "status": status,
                            }),
                        ))
                    }
                    Stanza::Iq(_iq) => {
                        debug!("XMPP client {} received IQ stanza (not yet supported)", client_id);
                        None
                    }
                }
            }
        };

        if let Some(event) = event_opt {
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                match call_llm_for_client(
                    llm_client,
                    app_state,
                    client_id.to_string(),
                    &instruction,
                    &client_data.lock().await.memory,
                    Some(&event),
                    protocol.as_ref(),
                    status_tx,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        if let Some(mem) = memory_updates {
                            client_data.lock().await.memory = mem;
                        }

                        // Execute actions
                        for action in actions {
                            Self::execute_action_result(
                                action,
                                protocol.clone(),
                                stanza_tx.clone(),
                                client_id,
                                status_tx,
                            ).await;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for XMPP client {}: {}", client_id, e);
                    }
                }
            }
        }
    }

    /// Execute an action result
    async fn execute_action_result(
        action: serde_json::Value,
        protocol: Arc<XmppClientProtocol>,
        stanza_tx: Arc<mpsc::UnboundedSender<tokio_xmpp::Stanza>>,
        client_id: ClientId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::client_trait::Client;

        match protocol.as_ref().execute_action(action) {
            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                match name.as_str() {
                    "send_message" => {
                        if let (Some(to), Some(body)) = (
                            data.get("to").and_then(|v| v.as_str()),
                            data.get("body").and_then(|v| v.as_str()),
                        ) {
                            let to_jid: Result<Jid> = to.parse().context("Invalid JID");
                            if let Ok(jid) = to_jid {
                                let mut message = Message::new(Some(jid));
                                message.type_ = MessageType::Chat;
                                message.bodies.insert(Lang::default(), body.to_string());

                                use tokio_xmpp::Stanza;
                                if let Err(e) = stanza_tx.send(Stanza::Message(message)) {
                                    error!("Failed to send XMPP message: {}", e);
                                    let _ = status_tx.send(format!("[ERROR] Failed to send XMPP message: {}", e));
                                } else {
                                    trace!("XMPP client {} sent message to {}", client_id, to);
                                }
                            }
                        }
                    }
                    "send_presence" => {
                        let show = data.get("show").and_then(|v| v.as_str());
                        let status = data.get("status").and_then(|v| v.as_str());

                        let mut presence = Presence::new(PresenceType::None);

                        if let Some(show_str) = show {
                            presence.show = match show_str {
                                "away" => Some(PresenceShow::Away),
                                "chat" => Some(PresenceShow::Chat),
                                "dnd" => Some(PresenceShow::Dnd),
                                "xa" => Some(PresenceShow::Xa),
                                _ => None,
                            };
                        }

                        if let Some(status_str) = status {
                            presence.statuses.insert(Lang::default(), status_str.to_string());
                        }

                        use tokio_xmpp::Stanza;
                        if let Err(e) = stanza_tx.send(Stanza::Presence(presence)) {
                            error!("Failed to send XMPP presence: {}", e);
                            let _ = status_tx.send(format!("[ERROR] Failed to send XMPP presence: {}", e));
                        } else {
                            trace!("XMPP client {} sent presence", client_id);
                        }
                    }
                    _ => {
                        warn!("Unknown custom action: {}", name);
                    }
                }
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                info!("XMPP client {} disconnecting", client_id);
                // The client will disconnect when the channel closes
            }
            Ok(_) => {}
            Err(e) => {
                error!("Action execution error for XMPP client {}: {}", client_id, e);
            }
        }
    }
}
