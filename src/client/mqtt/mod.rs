//! MQTT client implementation
pub mod actions;

pub use actions::MqttClientProtocol;

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, Publish, QoS};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::protocol::StartupParams;

use crate::client::mqtt::actions::{
    MQTT_CLIENT_CONNECTED_EVENT, MQTT_MESSAGE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
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
        let host = parts.get(0).context("Missing host in remote_addr")?.to_string();
        let port: u16 = parts
            .get(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(1883);

        // Extract startup parameters
        let mqtt_client_id = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("client_id"))
            .unwrap_or_else(|| format!("netget-{}", client_id));

        let username = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("username"));

        let password = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("password"));

        let keep_alive = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u64("keep_alive"))
            .unwrap_or(60);

        let clean_session = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("clean_session"))
            .unwrap_or(true);

        // Configure MQTT options
        let mut mqttoptions = MqttOptions::new(&mqtt_client_id, &host, port);
        mqttoptions.set_keep_alive(Duration::from_secs(keep_alive));
        mqttoptions.set_clean_session(clean_session);

        if let (Some(user), Some(pass)) = (username, password) {
            mqttoptions.set_credentials(user, pass);
        }

        info!("MQTT client {} connecting to {}:{} with client_id={}",
            client_id, host, port, mqtt_client_id);

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

        // Spawn MQTT event loop
        tokio::spawn(async move {
            handle_mqtt_events(
                eventloop,
                mqtt_client_clone,
                llm_client,
                app_state_clone,
                status_tx_clone,
                client_id,
                mqtt_client_id,
            )
            .await;
        });

        Ok(local_addr)
    }
}

/// Handle MQTT events from the broker
async fn handle_mqtt_events(
    mut eventloop: EventLoop,
    mqtt_client: AsyncClient,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    client_id: ClientId,
    mqtt_client_id: String,
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
                            app_state.update_client_status(client_id, ClientStatus::Connected).await;
                            let _ = status_tx.send(format!("[CLIENT] MQTT client {} connected", client_id));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());

                            // Call LLM with connected event
                            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                let protocol = Arc::new(MqttClientProtocol::new());
                                let event = ProtocolEvent::new(
                                    &MQTT_CLIENT_CONNECTED_EVENT,
                                    serde_json::json!({
                                        "remote_addr": format!("connected"),
                                        "client_id": mqtt_client_id,
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
                                    Ok(result) => {
                                        handle_llm_actions(
                                            result,
                                            &mqtt_client,
                                            &app_state,
                                            client_id,
                                            &protocol,
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
                        )
                        .await;
                    }
                    Event::Incoming(Packet::SubAck(suback)) => {
                        debug!("MQTT client {} subscription acknowledged: {:?}", client_id, suback);
                        // Could optionally notify LLM of successful subscription
                    }
                    Event::Incoming(Packet::Disconnect) => {
                        info!("MQTT client {} disconnected by broker", client_id);
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        let _ = status_tx.send(format!("[CLIENT] MQTT client {} disconnected", client_id));
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
                error!("MQTT client {} connection error: {}", client_id, e);
                app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                let _ = status_tx.send(format!("[CLIENT] MQTT client {} error: {}", client_id, e));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
                break;
            }
        }
    }
}

/// Handle incoming MQTT message
async fn handle_incoming_message(
    publish: &Publish,
    mqtt_client: &AsyncClient,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
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

        let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

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
                handle_llm_actions(result, mqtt_client, app_state, client_id, &protocol).await;
            }
            Err(e) => {
                error!("LLM error for MQTT client {}: {}", client_id, e);
            }
        }
    }
}

/// Handle LLM action results
async fn handle_llm_actions(
    result: ClientLlmResult,
    mqtt_client: &AsyncClient,
    app_state: &Arc<AppState>,
    client_id: ClientId,
    protocol: &Arc<MqttClientProtocol>,
) {
    // Update memory
    if let Some(mem) = result.memory_updates {
        app_state.set_memory_for_client(client_id, mem).await;
    }

    // Execute actions
    for action in result.actions {
        match protocol.execute_action(action) {
            Ok(ClientActionResult::Custom { name, data }) => {
                match name.as_str() {
                    "mqtt_subscribe" => {
                        if let (Some(topics), Some(qos)) = (
                            data.get("topics").and_then(|v| v.as_array()),
                            data.get("qos").and_then(|v| v.as_u64()),
                        ) {
                            let qos_level = match qos {
                                0 => QoS::AtMostOnce,
                                1 => QoS::AtLeastOnce,
                                2 => QoS::ExactlyOnce,
                                _ => QoS::AtMostOnce,
                            };

                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    if let Err(e) = mqtt_client.subscribe(topic_str, qos_level).await {
                                        error!("MQTT client {} subscribe error: {}", client_id, e);
                                    } else {
                                        info!("MQTT client {} subscribed to '{}' with QoS {}", client_id, topic_str, qos);
                                    }
                                }
                            }
                        }
                    }
                    "mqtt_publish" => {
                        if let (Some(topic), Some(payload)) = (
                            data.get("topic").and_then(|v| v.as_str()),
                            data.get("payload").and_then(|v| v.as_str()),
                        ) {
                            let qos = data.get("qos").and_then(|v| v.as_u64()).unwrap_or(0);
                            let retain = data.get("retain").and_then(|v| v.as_bool()).unwrap_or(false);

                            let qos_level = match qos {
                                0 => QoS::AtMostOnce,
                                1 => QoS::AtLeastOnce,
                                2 => QoS::ExactlyOnce,
                                _ => QoS::AtMostOnce,
                            };

                            if let Err(e) = mqtt_client
                                .publish(topic, qos_level, retain, payload.as_bytes())
                                .await
                            {
                                error!("MQTT client {} publish error: {}", client_id, e);
                            } else {
                                info!("MQTT client {} published to '{}': {}", client_id, topic, payload);
                            }
                        }
                    }
                    "mqtt_unsubscribe" => {
                        if let Some(topics) = data.get("topics").and_then(|v| v.as_array()) {
                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    if let Err(e) = mqtt_client.unsubscribe(topic_str).await {
                                        error!("MQTT client {} unsubscribe error: {}", client_id, e);
                                    } else {
                                        info!("MQTT client {} unsubscribed from '{}'", client_id, topic_str);
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        debug!("Unknown MQTT custom action: {}", name);
                    }
                }
            }
            Ok(ClientActionResult::Disconnect) => {
                info!("MQTT client {} disconnecting", client_id);
                if let Err(e) = mqtt_client.disconnect().await {
                    error!("MQTT client {} disconnect error: {}", client_id, e);
                }
            }
            Ok(ClientActionResult::WaitForMore) => {
                debug!("MQTT client {} waiting for more messages", client_id);
            }
            Ok(ClientActionResult::SendData(_)) => {
                // Not used for MQTT
            }
            Ok(ClientActionResult::NoAction) => {
                // No action needed
            }
            Ok(ClientActionResult::Multiple(_)) => {
                // Multiple actions not currently supported for MQTT
                debug!("Multiple actions not supported for MQTT client");
            }
            Err(e) => {
                error!("Error executing MQTT action: {}", e);
            }
        }
    }
}
