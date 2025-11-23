//! Kafka client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Kafka client connected event
pub static KAFKA_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "kafka_connected",
        "Kafka client successfully connected to broker cluster",
    )
    .with_parameters(vec![
        Parameter {
            name: "brokers".to_string(),
            type_hint: "string".to_string(),
            description: "Kafka broker addresses".to_string(),
            required: true,
        },
        Parameter {
            name: "client_mode".to_string(),
            type_hint: "string".to_string(),
            description: "Client mode: producer or consumer".to_string(),
            required: true,
        },
    ])
});

/// Kafka message received event (for consumers)
pub static KAFKA_CLIENT_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "kafka_message_received",
        "Message received from Kafka topic",
    )
    .with_parameters(vec![
        Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Kafka topic name".to_string(),
            required: true,
        },
        Parameter {
            name: "partition".to_string(),
            type_hint: "number".to_string(),
            description: "Partition number".to_string(),
            required: true,
        },
        Parameter {
            name: "offset".to_string(),
            type_hint: "number".to_string(),
            description: "Message offset".to_string(),
            required: true,
        },
        Parameter {
            name: "key".to_string(),
            type_hint: "string".to_string(),
            description: "Message key (nullable)".to_string(),
            required: false,
        },
        Parameter {
            name: "payload".to_string(),
            type_hint: "string".to_string(),
            description: "Message payload".to_string(),
            required: true,
        },
        Parameter {
            name: "timestamp".to_string(),
            type_hint: "number".to_string(),
            description: "Message timestamp (milliseconds)".to_string(),
            required: false,
        },
    ])
});

/// Kafka message delivered event (for producers)
pub static KAFKA_CLIENT_MESSAGE_DELIVERED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "kafka_message_delivered",
        "Message successfully delivered to Kafka topic",
    )
    .with_parameters(vec![
        Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Kafka topic name".to_string(),
            required: true,
        },
        Parameter {
            name: "partition".to_string(),
            type_hint: "number".to_string(),
            description: "Partition number".to_string(),
            required: true,
        },
        Parameter {
            name: "offset".to_string(),
            type_hint: "number".to_string(),
            description: "Message offset".to_string(),
            required: true,
        },
    ])
});

/// Kafka client protocol action handler
pub struct KafkaClientProtocol;

impl KafkaClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for KafkaClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "mode".to_string(),
                description: "Client mode: 'producer' or 'consumer'".to_string(),
                type_hint: "string".to_string(),
                required: true,
                example: json!("producer"),
            },
            ParameterDefinition {
                name: "topics".to_string(),
                description: "Topics to subscribe to (consumer mode only)".to_string(),
                type_hint: "array".to_string(),
                required: false,
                example: json!(["my-topic", "another-topic"]),
            },
            ParameterDefinition {
                name: "group_id".to_string(),
                description: "Consumer group ID (consumer mode only)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("my-consumer-group"),
            },
            ParameterDefinition {
                name: "client_id".to_string(),
                description: "Kafka client ID for identification".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("netget-kafka-client"),
            },
        ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "produce_message".to_string(),
                description: "Produce a message to a Kafka topic (producer mode)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "topic".to_string(),
                        type_hint: "string".to_string(),
                        description: "Kafka topic name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "payload".to_string(),
                        type_hint: "string".to_string(),
                        description: "Message payload".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Message key (for partitioning)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "produce_message",
                    "topic": "my-topic",
                    "payload": "Hello Kafka",
                    "key": "user-123"
                }),
            },
            ActionDefinition {
                name: "subscribe_topics".to_string(),
                description: "Subscribe to topics (consumer mode)".to_string(),
                parameters: vec![Parameter {
                    name: "topics".to_string(),
                    type_hint: "array".to_string(),
                    description: "List of topic names to subscribe to".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "subscribe_topics",
                    "topics": ["topic1", "topic2"]
                }),
            },
            ActionDefinition {
                name: "commit_offset".to_string(),
                description: "Commit current offset (consumer mode)".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "commit_offset"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from Kafka cluster".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "produce_message".to_string(),
                description: "Produce a message in response to received data (producer mode)"
                    .to_string(),
                parameters: vec![
                    Parameter {
                        name: "topic".to_string(),
                        type_hint: "string".to_string(),
                        description: "Kafka topic name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "payload".to_string(),
                        type_hint: "string".to_string(),
                        description: "Message payload".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Message key".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "produce_message",
                    "topic": "response-topic",
                    "payload": "Processed data"
                }),
            },
            ActionDefinition {
                name: "commit_offset".to_string(),
                description: "Commit offset after processing message".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "commit_offset"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Kafka"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("kafka_connected", "Triggered when Kafka client connects to broker cluster"),
            EventType::new("kafka_message_received", "Triggered when Kafka consumer receives a message"),
            EventType::new("kafka_message_delivered", "Triggered when Kafka producer delivers a message"),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>Kafka"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "kafka",
            "kafka client",
            "connect to kafka",
            "kafka producer",
            "kafka consumer",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("rdkafka (librdkafka wrapper)")
                .llm_control("Full control over producing/consuming messages, topic subscription, offset management")
                .e2e_testing("Docker Kafka cluster")
                .build()
    }
    fn description(&self) -> &'static str {
        "Kafka client for distributed streaming and messaging"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to Kafka at localhost:9092 as producer and send a message to 'events' topic"
    }
    fn group_name(&self) -> &'static str {
        "Messaging"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for KafkaClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::kafka::KafkaClient;
            KafkaClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "produce_message" => {
                let topic = action
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .context("Missing 'topic' field")?
                    .to_string();

                let payload = action
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .context("Missing 'payload' field")?
                    .to_string();

                let key = action
                    .get("key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "kafka_produce".to_string(),
                    data: json!({
                        "topic": topic,
                        "payload": payload,
                        "key": key,
                    }),
                })
            }
            "subscribe_topics" => {
                let topics = action
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .context("Missing 'topics' field")?;

                let topic_names: Vec<String> = topics
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if topic_names.is_empty() {
                    return Err(anyhow::anyhow!("No valid topic names provided"));
                }

                Ok(ClientActionResult::Custom {
                    name: "kafka_subscribe".to_string(),
                    data: json!({
                        "topics": topic_names,
                    }),
                })
            }
            "commit_offset" => Ok(ClientActionResult::Custom {
                name: "kafka_commit".to_string(),
                data: json!({}),
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!(
                "Unknown Kafka client action: {}",
                action_type
            )),
        }
    }
}
