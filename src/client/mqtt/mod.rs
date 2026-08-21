//! MQTT client implementation
pub mod actions;

pub use actions::MqttClientProtocol;

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, Publish, QoS};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::protocol::StartupParams;

use crate::client::llm_budget::call_llm_for_client;
use crate::client::mqtt::actions::{MQTT_CLIENT_CONNECTED_EVENT, MQTT_MESSAGE_RECEIVED_EVENT};
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event as ProtocolEvent;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// MQTT client that connects to an MQTT broker
pub struct MqttClient;

impl MqttClient {
    /// Connect to an MQTT broker with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse remote address (host:port)
        let parts: Vec<&str> = remote_addr.split(':').collect();
        let host = parts
            .get(0)
            .context("Missing host in remote_addr")?
            .to_string();
        let port: u16 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1883);

        // Extract startup parameters
        let mqtt_client_id = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("client_id"))
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("netget-{}", client_id));

        let username = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("username"))
            .transpose()?
            .flatten();

        let password = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("password"))
            .transpose()?
            .flatten();

        let keep_alive = startup_params
            .as_ref()
            .map(|p| p.get_optional_u64("keep_alive"))
            .transpose()?
            .flatten()
            .unwrap_or(60);

        let clean_session = startup_params
            .as_ref()
            .map(|p| p.get_optional_bool("clean_session"))
            .transpose()?
            .flatten()
            .unwrap_or(true);

        // Configure MQTT options
        let mut mqttoptions = MqttOptions::new(&mqtt_client_id, &host, port);
        mqttoptions.set_keep_alive(Duration::from_secs(keep_alive));
        mqttoptions.set_clean_session(clean_session);

        if let (Some(user), Some(pass)) = (username, password) {
            mqttoptions.set_credentials(user, pass);
        }

        info!(
            "MQTT client {} connecting to {}:{} with client_id={}",
            client_id, host, port, mqtt_client_id
        );

        // Create MQTT client
        let (mqtt_client, eventloop) = AsyncClient::new(mqttoptions, 10);

        // For returning the local address, we need to extract it from the eventloop
        // rumqttc doesn't expose local_addr directly, so we'll construct a fake SocketAddr
        // based on the remote address. The actual TCP connection is managed internally.
        let local_addr: SocketAddr = format!("0.0.0.0:0").parse().unwrap();

        // Clone for the event loop task
        let mqtt_client_clone = mqtt_client.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();

        // Set when a `disconnect` was asked for, so the event loop's read error that follows
        // is reported as a clean disconnect rather than as a connection failure.
        let disconnecting = Arc::new(AtomicBool::new(false));

        // Command channel for injected actions (the dashboard's [ send ]). Registered here,
        // before the event loop task starts, so it exists throughout the connected-event LLM
        // call that task makes on ConnAck - a manual `*` rule can park that call for minutes
        // and the operator must still be able to publish.
        let command_rx =
            crate::client::command_support::register_command_channel(&app_state, client_id).await;
        let cmd_client = mqtt_client.clone();
        let cmd_state = app_state.clone();
        let cmd_status_tx = status_tx.clone();
        let cmd_disconnecting = disconnecting.clone();
        let cmd_task = tokio::spawn(async move {
            command_loop(
                command_rx,
                cmd_client,
                client_id,
                cmd_state,
                cmd_status_tx,
                cmd_disconnecting,
            )
            .await;
        });
        app_state.register_client_task(client_id, cmd_task).await;

        // Spawn MQTT event loop
        let task_registrar = app_state.clone();
        let handle = tokio::spawn(async move {
            handle_mqtt_events(
                eventloop,
                mqtt_client_clone,
                llm_client,
                app_state_clone,
                status_tx_clone,
                client_id,
                mqtt_client_id,
                disconnecting,
            )
            .await;
        });
        task_registrar.register_client_task(client_id, handle).await;

        Ok(local_addr)
    }
}

/// Handle MQTT events from the broker
#[allow(clippy::too_many_arguments)]
async fn handle_mqtt_events(
    mut eventloop: EventLoop,
    mqtt_client: AsyncClient,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    client_id: ClientId,
    mqtt_client_id: String,
    disconnecting: Arc<AtomicBool>,
) {
    let mut connected = false;

    loop {
        match eventloop.poll().await {
            Ok(notification) => {
                trace!("MQTT client {} event: {:?}", client_id, notification);

                match notification {
                    Event::Incoming(Packet::ConnAck(_)) => {
                        if !connected {
                            connected = true;
                            info!("MQTT client {} connected to broker", client_id);
                            app_state
                                .update_client_status(client_id, ClientStatus::Connected)
                                .await;
                            let _ = status_tx
                                .send(format!("[CLIENT] MQTT client {} connected", client_id));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());

                            // Call LLM with connected event
                            if let Some(instruction) =
                                app_state.get_instruction_for_client(client_id).await
                            {
                                let protocol = Arc::new(MqttClientProtocol::new());
                                let event = ProtocolEvent::new(
                                    &MQTT_CLIENT_CONNECTED_EVENT,
                                    serde_json::json!({
                                        "remote_addr": format!("connected"),
                                        "client_id": mqtt_client_id,
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
                                    Ok(result) => {
                                        handle_llm_actions(
                                            result,
                                            &mqtt_client,
                                            &app_state,
                                            client_id,
                                            &protocol,
                                            &disconnecting,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        error!("LLM error for MQTT client {}: {}", client_id, e);
                                    }
                                }
                            }
                        }
                    }
                    Event::Incoming(Packet::Publish(publish)) => {
                        handle_incoming_message(
                            &publish,
                            &mqtt_client,
                            &llm_client,
                            &app_state,
                            &status_tx,
                            client_id,
                            &disconnecting,
                        )
                        .await;
                    }
                    Event::Incoming(Packet::SubAck(suback)) => {
                        debug!(
                            "MQTT client {} subscription acknowledged: {:?}",
                            client_id, suback
                        );
                        // Could optionally notify LLM of successful subscription
                    }
                    Event::Incoming(Packet::Disconnect) => {
                        info!("MQTT client {} disconnected by broker", client_id);
                        app_state
                            .update_client_status(client_id, ClientStatus::Disconnected)
                            .await;
                        let _ = status_tx
                            .send(format!("[CLIENT] MQTT client {} disconnected", client_id));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    Event::Outgoing(_) => {
                        // Outgoing packets are logged at trace level
                    }
                    _ => {
                        // Other events (PingReq, PingResp, etc.)
                    }
                }
            }
            Err(e) => {
                // A `disconnect` action closes the socket, and rumqttc reports the closed
                // socket as a poll error. Reporting that as ClientStatus::Error would draw a
                // deliberate hang-up as a failure, so the requested case is separated out.
                if disconnecting.load(Ordering::SeqCst) {
                    info!("MQTT client {} disconnected on request", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ =
                        status_tx.send(format!("[CLIENT] MQTT client {} disconnected", client_id));
                } else {
                    error!("MQTT client {} connection error: {}", client_id, e);
                    app_state
                        .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                        .await;
                    let _ =
                        status_tx.send(format!("[CLIENT] MQTT client {} error: {}", client_id, e));
                }
                let _ = status_tx.send("__UPDATE_UI__".to_string());
                break;
            }
        }
    }

    // Every exit path lands here: drop the command handle so the dashboard stops offering
    // [ send ] on a dead connection. It also closes the command channel, which ends
    // `command_loop`.
    app_state.remove_client_handle(client_id).await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());
}

/// Handle incoming MQTT message
#[allow(clippy::too_many_arguments)]
async fn handle_incoming_message(
    publish: &Publish,
    mqtt_client: &AsyncClient,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    disconnecting: &Arc<AtomicBool>,
) {
    let topic = publish.topic.clone();
    let payload = String::from_utf8_lossy(&publish.payload).to_string();
    let qos = publish.qos as u8;
    let retain = publish.retain;

    debug!(
        "MQTT client {} received message on topic '{}': {} bytes",
        client_id,
        topic,
        publish.payload.len()
    );

    // Call LLM with message received event
    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
        let protocol = Arc::new(MqttClientProtocol::new());
        let event = ProtocolEvent::new(
            &MQTT_MESSAGE_RECEIVED_EVENT,
            serde_json::json!({
                "topic": topic,
                "payload": payload,
                "qos": qos,
                "retain": retain,
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
            Ok(result) => {
                handle_llm_actions(
                    result,
                    mqtt_client,
                    app_state,
                    client_id,
                    &protocol,
                    disconnecting,
                )
                .await;
            }
            Err(e) => {
                error!("LLM error for MQTT client {}: {}", client_id, e);
            }
        }
    }
}

/// What [`apply_action`] did with one executed action.
enum MqttApplied {
    /// Handed to rumqttc, whose event loop writes the packet on the socket. rumqttc returns
    /// as soon as the request is accepted and reports no byte count, so this is deliberately
    /// not a "sent N bytes" claim.
    Queued(String),
    /// Ran, but nothing was put on the wire.
    NoWire(String),
    /// A disconnect was requested and sent to the broker.
    Disconnect,
}

/// Apply one executed action to the live broker connection.
///
/// Shared by the LLM path ([`handle_llm_actions`]) and by injected commands
/// ([`command_loop`]), so the mapping from `mqtt_publish`/`mqtt_subscribe`/`mqtt_unsubscribe`
/// onto `rumqttc` calls exists exactly once and an injected action behaves identically to an
/// LLM-produced one.
async fn apply_action(
    action_result: ClientActionResult,
    mqtt_client: &AsyncClient,
    client_id: ClientId,
    disconnecting: &Arc<AtomicBool>,
) -> Result<MqttApplied> {
    match action_result {
        ClientActionResult::Custom { name, data } => match name.as_str() {
            "mqtt_subscribe" => {
                let topics = data
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .context("mqtt_subscribe result has no 'topics' array")?;
                let qos = data.get("qos").and_then(|v| v.as_u64()).unwrap_or(0);
                let qos_level = qos_from_u64(qos);

                let mut subscribed: Vec<String> = Vec::new();
                for topic in topics {
                    if let Some(topic_str) = topic.as_str() {
                        mqtt_client
                            .subscribe(topic_str, qos_level)
                            .await
                            .with_context(|| format!("subscribe to '{topic_str}' failed"))?;
                        info!(
                            "MQTT client {} subscribed to '{}' with QoS {}",
                            client_id, topic_str, qos
                        );
                        subscribed.push(topic_str.to_string());
                    }
                }
                if subscribed.is_empty() {
                    return Ok(MqttApplied::NoWire(
                        "mqtt_subscribe: no valid topic strings".to_string(),
                    ));
                }
                Ok(MqttApplied::Queued(format!(
                    "SUBSCRIBE to [{}] at QoS {} accepted by rumqttc",
                    subscribed.join(", "),
                    qos
                )))
            }
            "mqtt_publish" => {
                let topic = data
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .context("mqtt_publish result has no 'topic'")?;
                let payload = data
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .context("mqtt_publish result has no 'payload'")?;
                let qos = data.get("qos").and_then(|v| v.as_u64()).unwrap_or(0);
                let retain = data
                    .get("retain")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                mqtt_client
                    .publish(topic, qos_from_u64(qos), retain, payload.as_bytes())
                    .await
                    .with_context(|| format!("publish to '{topic}' failed"))?;
                info!(
                    "MQTT client {} published to '{}': {}",
                    client_id, topic, payload
                );
                Ok(MqttApplied::Queued(format!(
                    "PUBLISH to '{}' ({} byte payload, QoS {}, retain {}) accepted by rumqttc",
                    topic,
                    payload.len(),
                    qos,
                    retain
                )))
            }
            "mqtt_unsubscribe" => {
                let topics = data
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .context("mqtt_unsubscribe result has no 'topics' array")?;
                let mut removed: Vec<String> = Vec::new();
                for topic in topics {
                    if let Some(topic_str) = topic.as_str() {
                        mqtt_client
                            .unsubscribe(topic_str)
                            .await
                            .with_context(|| format!("unsubscribe from '{topic_str}' failed"))?;
                        info!(
                            "MQTT client {} unsubscribed from '{}'",
                            client_id, topic_str
                        );
                        removed.push(topic_str.to_string());
                    }
                }
                if removed.is_empty() {
                    return Ok(MqttApplied::NoWire(
                        "mqtt_unsubscribe: no valid topic strings".to_string(),
                    ));
                }
                Ok(MqttApplied::Queued(format!(
                    "UNSUBSCRIBE from [{}] accepted by rumqttc",
                    removed.join(", ")
                )))
            }
            other => Err(anyhow::anyhow!(
                "MQTT client cannot apply custom result '{}'",
                other
            )),
        },
        ClientActionResult::Disconnect => {
            info!("MQTT client {} disconnecting", client_id);
            // Set before the request goes out, so the poll error rumqttc raises for the
            // closed socket is already attributable when the event loop sees it.
            disconnecting.store(true, Ordering::SeqCst);
            mqtt_client
                .disconnect()
                .await
                .inspect_err(|_| disconnecting.store(false, Ordering::SeqCst))
                .context("disconnect request failed")?;
            Ok(MqttApplied::Disconnect)
        }
        ClientActionResult::WaitForMore => Ok(MqttApplied::NoWire(
            "waiting for the next broker message; nothing sent".to_string(),
        )),
        ClientActionResult::NoAction => {
            Ok(MqttApplied::NoWire("no_action: nothing sent".to_string()))
        }
        ClientActionResult::SendData(_) => {
            Err(anyhow::anyhow!("MQTT has no raw-byte channel; use publish"))
        }
        ClientActionResult::Multiple(_) => Err(anyhow::anyhow!(
            "MQTT client does not support Multiple action results"
        )),
    }
}

fn qos_from_u64(qos: u64) -> QoS {
    match qos {
        1 => QoS::AtLeastOnce,
        2 => QoS::ExactlyOnce,
        _ => QoS::AtMostOnce,
    }
}

/// Handle LLM action results
async fn handle_llm_actions(
    result: ClientLlmResult,
    mqtt_client: &AsyncClient,
    app_state: &Arc<AppState>,
    client_id: ClientId,
    protocol: &Arc<MqttClientProtocol>,
    disconnecting: &Arc<AtomicBool>,
) {
    // Update memory
    if let Some(mem) = result.memory_updates {
        app_state.set_memory_for_client(client_id, mem).await;
    }

    // Execute actions
    for action in result.actions {
        match protocol.execute_action(action) {
            Ok(action_result) => {
                match apply_action(action_result, mqtt_client, client_id, disconnecting).await {
                    Ok(MqttApplied::Queued(detail)) => {
                        debug!("MQTT client {}: {}", client_id, detail);
                    }
                    Ok(MqttApplied::NoWire(detail)) => {
                        debug!("MQTT client {}: {}", client_id, detail);
                    }
                    Ok(MqttApplied::Disconnect) => {}
                    Err(e) => {
                        error!("MQTT client {} could not apply action: {}", client_id, e);
                    }
                }
            }
            Err(e) => {
                error!("Error executing MQTT action: {}", e);
            }
        }
    }
}

/// Drain injected commands (the dashboard's \[ send \]) until the channel closes - which
/// happens when the client is removed or the broker event loop exits - or until an injected
/// `disconnect` ends the session.
///
/// This is option (a) of the library-driven archetype: `rumqttc::AsyncClient` is a cheap
/// clonable handle to the event loop's request channel, so no `Arc<Mutex<_>>` and no
/// restructuring was needed - the command loop holds its own clone and applies actions
/// through the same [`apply_action`] the LLM path uses.
async fn command_loop(
    mut command_rx: mpsc::Receiver<crate::state::client_handles::ClientCommand>,
    mqtt_client: AsyncClient,
    client_id: ClientId,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    disconnecting: Arc<AtomicBool>,
) {
    use crate::llm::actions::protocol_trait::Protocol;
    use crate::state::client_handles::ClientSendOutcome;
    use crate::state::AccessLogOwner;

    let protocol = MqttClientProtocol::new();

    while let Some(command) = command_rx.recv().await {
        let action = command.action.clone();
        let outcome: Result<ClientSendOutcome> = match protocol.execute_action(action.clone()) {
            Err(e) => Ok(ClientSendOutcome::Rejected {
                error: e.to_string(),
            }),
            Ok(result) => match apply_action(result, &mqtt_client, client_id, &disconnecting).await
            {
                // Never `Sent`: rumqttc accepts the request into its event loop's queue and
                // returns; the bytes are written by that loop afterwards and it reports no
                // count, so claiming a byte count here would be a guess.
                Ok(MqttApplied::Queued(detail)) => Ok(ClientSendOutcome::Executed { detail }),
                Ok(MqttApplied::NoWire(detail)) => Ok(ClientSendOutcome::Executed { detail }),
                Ok(MqttApplied::Disconnect) => Ok(ClientSendOutcome::Disconnected),
                Err(e) => Err(e),
            },
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
            error!("MQTT client {} injected action failed: {}", client_id, e);
            let _ = status_tx.send(format!(
                "[WARN] Client {} injected action failed: {}",
                client_id, e
            ));
        }
        let _ = status_tx.send("__UPDATE_UI__".to_string());
        crate::client::command_support::reply(command, outcome);

        if disconnect {
            // The event loop drops the handle on its way out too; doing it here as well means
            // the dashboard stops offering [ send ] the moment the DISCONNECT went out.
            app_state.remove_client_handle(client_id).await;
            break;
        }
    }
}
