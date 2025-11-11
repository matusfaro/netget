//! Kafka client implementation
pub mod actions;

pub use actions::KafkaClientProtocol;

use anyhow::{Context, Result};
use rdkafka::{
    consumer::{Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
    ClientConfig, Message,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::kafka::actions::{
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
    KAFKA_CLIENT_CONNECTED_EVENT, KAFKA_CLIENT_MESSAGE_DELIVERED_EVENT,
    KAFKA_CLIENT_MESSAGE_RECEIVED_EVENT,
};

/// Kafka client mode
#[derive(Debug, Clone, PartialEq)]
enum KafkaClientMode {
    Producer,
    Consumer,
}

/// Kafka client that connects to a Kafka broker cluster
pub struct KafkaClient;

impl KafkaClient {
    /// Connect to a Kafka cluster with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup parameters
        let params = startup_params.context("Kafka client requires startup parameters")?;

        let mode_str = params.get_string("mode");

        let mode = match mode_str.to_lowercase().as_str() {
            "producer" => KafkaClientMode::Producer,
            "consumer" => KafkaClientMode::Consumer,
            _ => return Err(anyhow::anyhow!("Invalid mode: {}. Must be 'producer' or 'consumer'", mode_str)),
        };

        let client_id_str = params.get_optional_string("client_id")
            .unwrap_or_else(|| "netget-kafka-client".to_string());

        info!("Kafka client {} connecting to {} as {:?}", client_id, remote_addr, mode);

        match mode {
            KafkaClientMode::Producer => {
                Self::connect_producer(
                    remote_addr,
                    &client_id_str,
                    llm_client,
                    app_state,
                    status_tx,
                    client_id,
                )
                .await
            }
            KafkaClientMode::Consumer => {
                let group_id = params.get_optional_string("group_id")
                    .unwrap_or_else(|| "netget-consumer-group".to_string());

                let topics: Vec<String> = params.get_optional_array("topics")
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                Self::connect_consumer(
                    remote_addr,
                    &client_id_str,
                    &group_id,
                    topics,
                    llm_client,
                    app_state,
                    status_tx,
                    client_id,
                )
                .await
            }
        }
    }

    /// Connect as Kafka producer
    async fn connect_producer(
        brokers: String,
        client_id_str: &str,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Create producer
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &brokers)
            .set("client.id", client_id_str)
            .set("message.timeout.ms", "30000")
            .create()
            .context("Failed to create Kafka producer")?;


        // Update client status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] Kafka producer {} connected to {}", client_id, brokers);
        console_info!(status_tx, "__UPDATE_UI__");

        // Store producer in protocol data
        app_state
            .with_client_mut(client_id, |client_inst| {
                client_inst.set_protocol_field("mode".to_string(), serde_json::json!("producer"));
                client_inst.set_protocol_field("brokers".to_string(), serde_json::json!(brokers.clone()));
            })
            .await;

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(KafkaClientProtocol::new());
            let event = Event::new(
                &KAFKA_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "brokers": brokers,
                    "client_mode": "producer",
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            if let Ok(ClientLlmResult { actions, memory_updates }) = call_llm_for_client(
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
                // Update memory
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }

                // Execute initial actions
                for action in actions {
                    Self::execute_producer_action(
                        client_id,
                        &producer,
                        action,
                        &protocol,
                        &app_state,
                        &llm_client,
                        &status_tx,
                    )
                    .await;
                }
            }
        }

        // Spawn producer monitoring task
        let _producer_arc = Arc::new(producer);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Kafka producer {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy address
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Connect as Kafka consumer
    async fn connect_consumer(
        brokers: String,
        client_id_str: &str,
        group_id: &str,
        topics: Vec<String>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Create consumer
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &brokers)
            .set("group.id", group_id)
            .set("client.id", client_id_str)
            .set("enable.auto.commit", "false")
            .set("session.timeout.ms", "30000")
            .set("auto.offset.reset", "earliest")
            .create()
            .context("Failed to create Kafka consumer")?;

        // Subscribe to topics if provided
        if !topics.is_empty() {
            let topic_refs: Vec<&str> = topics.iter().map(|s| s.as_str()).collect();
            consumer
                .subscribe(&topic_refs)
                .context("Failed to subscribe to topics")?;
            info!("Kafka consumer {} subscribed to topics: {:?}", client_id, topics);
        }


        // Update client status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] Kafka consumer {} connected to {}", client_id, brokers);
        console_info!(status_tx, "__UPDATE_UI__");

        // Store consumer info in protocol data
        app_state
            .with_client_mut(client_id, |client_inst| {
                client_inst.set_protocol_field("mode".to_string(), serde_json::json!("consumer"));
                client_inst.set_protocol_field("brokers".to_string(), serde_json::json!(brokers.clone()));
                client_inst.set_protocol_field("group_id".to_string(), serde_json::json!(group_id));
            })
            .await;

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(KafkaClientProtocol::new());
            let event = Event::new(
                &KAFKA_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "brokers": brokers,
                    "client_mode": "consumer",
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            if let Ok(ClientLlmResult { actions, memory_updates }) = call_llm_for_client(
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
                // Update memory
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }

                // Execute initial actions (e.g., subscribe_topics)
                for action in actions {
                    Self::execute_consumer_action(
                        client_id,
                        &consumer,
                        action,
                        &protocol,
                        &app_state,
                        &llm_client,
                        &status_tx,
                    )
                    .await;
                }
            }
        }

        // Spawn consumer loop
        let consumer_arc = Arc::new(consumer);
        tokio::spawn(async move {
            loop {
                // Poll for messages
                match consumer_arc.recv().await {
                    Ok(message) => {
                        let topic = message.topic().to_string();
                        let partition = message.partition();
                        let offset = message.offset();
                        let timestamp = message.timestamp().to_millis();

                        let key = message
                            .key()
                            .and_then(|k| String::from_utf8(k.to_vec()).ok());

                        let payload = message
                            .payload()
                            .and_then(|p| String::from_utf8(p.to_vec()).ok())
                            .unwrap_or_default();

                        trace!(
                            "Kafka consumer {} received message: topic={}, partition={}, offset={}, payload_len={}",
                            client_id,
                            topic,
                            partition,
                            offset,
                            payload.len()
                        );

                        // Call LLM with message
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let protocol = Arc::new(KafkaClientProtocol::new());
                            let event = Event::new(
                                &KAFKA_CLIENT_MESSAGE_RECEIVED_EVENT,
                                serde_json::json!({
                                    "topic": topic,
                                    "partition": partition,
                                    "offset": offset,
                                    "key": key,
                                    "payload": payload,
                                    "timestamp": timestamp,
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
                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }

                                    // Execute actions
                                    for action in actions {
                                        Self::execute_consumer_action(
                                            client_id,
                                            &consumer_arc,
                                            action,
                                            &protocol,
                                            &app_state,
                                            &llm_client,
                                            &status_tx,
                                        )
                                        .await;
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for Kafka consumer {}: {}", client_id, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        console_error!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                }

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Kafka consumer {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy address
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute producer action
    async fn execute_producer_action(
        client_id: ClientId,
        producer: &FutureProducer,
        action: serde_json::Value,
        protocol: &Arc<KafkaClientProtocol>,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        match protocol.execute_action(action) {
            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data })
                if name == "kafka_produce" =>
            {
                if let (Some(topic), Some(payload)) = (
                    data.get("topic").and_then(|v| v.as_str()),
                    data.get("payload").and_then(|v| v.as_str()),
                ) {
                    let key = data.get("key").and_then(|v| v.as_str());

                    let mut record = FutureRecord::to(topic).payload(payload);

                    if let Some(k) = key {
                        record = record.key(k);
                    }

                    match producer.send(record, Duration::from_secs(30)).await {
                        Ok((partition, offset)) => {
                            info!(
                                "Kafka producer {} sent message to topic '{}' (partition={}, offset={})",
                                client_id, topic, partition, offset
                            );

                            // Call LLM with delivery confirmation
                            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                let event = Event::new(
                                    &KAFKA_CLIENT_MESSAGE_DELIVERED_EVENT,
                                    serde_json::json!({
                                        "topic": topic,
                                        "partition": partition,
                                        "offset": offset,
                                    }),
                                );

                                let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                                if let Ok(ClientLlmResult { memory_updates, .. }) = call_llm_for_client(
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
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }
                                }
                            }
                        }
                        Err((kafka_err, _)) => {
                            error!("Kafka producer {} failed to send message: {}", client_id, kafka_err);
                        }
                    }
                }
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                info!("Kafka producer {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
            }
            _ => {}
        }
    }

    /// Execute consumer action
    async fn execute_consumer_action(
        client_id: ClientId,
        consumer: &StreamConsumer,
        action: serde_json::Value,
        protocol: &Arc<KafkaClientProtocol>,
        app_state: &Arc<AppState>,
        _llm_client: &OllamaClient,
        _status_tx: &mpsc::UnboundedSender<String>,
    ) {
        match protocol.execute_action(action) {
            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data })
                if name == "kafka_subscribe" =>
            {
                if let Some(topics_arr) = data.get("topics").and_then(|v| v.as_array()) {
                    let topics: Vec<String> = topics_arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();

                    if !topics.is_empty() {
                        let topic_refs: Vec<&str> = topics.iter().map(|s| s.as_str()).collect();
                        if let Err(e) = consumer.subscribe(&topic_refs) {
                            error!("Kafka consumer {} failed to subscribe to topics: {}", client_id, e);
                        } else {
                            info!("Kafka consumer {} subscribed to topics: {:?}", client_id, topics);
                        }
                    }
                }
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, .. })
                if name == "kafka_commit" =>
            {
                if let Err(e) = consumer.commit_consumer_state(rdkafka::consumer::CommitMode::Async) {
                    error!("Kafka consumer {} failed to commit offset: {}", client_id, e);
                } else {
                    trace!("Kafka consumer {} committed offset", client_id);
                }
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                info!("Kafka consumer {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
            }
            _ => {}
        }
    }
}
