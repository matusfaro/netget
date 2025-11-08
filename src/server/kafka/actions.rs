//! Kafka protocol actions and LLM integration
//!
//! This module defines the action-based interface for Kafka protocol.
//! The LLM can control Kafka broker behavior through these actions.

use crate::llm::actions::protocol_trait::{ActionResult, Protocol};
use crate::llm::actions::{ActionDefinition, Parameter, ParameterDefinition, Server};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use serde_json::{json, Value};

/// Event: Kafka produce request received
pub static PRODUCE_REQUEST_EVENT: Lazy<EventType> = Lazy::new(|| {
    EventType::new(
        "kafka_produce_request",
        "Triggered when a Kafka producer sends records to a topic",
    )
    .with_actions(vec![produce_response_action(), error_response_action()])
    .with_parameters(vec![
        Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Topic name".to_string(),
            required: true,
        },
        Parameter {
            name: "partition".to_string(),
            type_hint: "number".to_string(),
            description: "Partition number".to_string(),
            required: true,
        },
        Parameter {
            name: "record_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of records in batch".to_string(),
            required: true,
        },
        Parameter {
            name: "first_key".to_string(),
            type_hint: "string".to_string(),
            description: "Key of first record (optional)".to_string(),
            required: false,
        },
        Parameter {
            name: "first_value_preview".to_string(),
            type_hint: "string".to_string(),
            description: "Preview of first record value".to_string(),
            required: true,
        },
    ])
});

/// Event: Kafka fetch request received
pub static FETCH_REQUEST_EVENT: Lazy<EventType> = Lazy::new(|| {
    EventType::new(
        "kafka_fetch_request",
        "Triggered when a Kafka consumer requests records from a topic",
    )
    .with_actions(vec![fetch_response_action(), error_response_action()])
    .with_parameters(vec![
        Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Topic name".to_string(),
            required: true,
        },
        Parameter {
            name: "partition".to_string(),
            type_hint: "number".to_string(),
            description: "Partition number".to_string(),
            required: true,
        },
        Parameter {
            name: "fetch_offset".to_string(),
            type_hint: "number".to_string(),
            description: "Offset to fetch from".to_string(),
            required: true,
        },
        Parameter {
            name: "max_bytes".to_string(),
            type_hint: "number".to_string(),
            description: "Maximum bytes to return".to_string(),
            required: true,
        },
    ])
});

/// Event: Kafka metadata request received
pub static METADATA_REQUEST_EVENT: Lazy<EventType> = Lazy::new(|| {
    EventType::new(
        "kafka_metadata_request",
        "Triggered when a client requests cluster/topic metadata",
    )
    .with_actions(vec![metadata_response_action(), error_response_action()])
    .with_parameters(vec![Parameter {
        name: "requested_topics".to_string(),
        type_hint: "array".to_string(),
        description: "Topics client wants metadata for (empty = all topics)".to_string(),
        required: false,
    }])
});

/// Event: Kafka offset commit request received
pub static OFFSET_COMMIT_REQUEST_EVENT: Lazy<EventType> = Lazy::new(|| {
    EventType::new(
        "kafka_offset_commit_request",
        "Triggered when a consumer commits offsets for a topic partition",
    )
    .with_actions(vec![
        offset_commit_response_action(),
        error_response_action(),
    ])
    .with_parameters(vec![
        Parameter {
            name: "group_id".to_string(),
            type_hint: "string".to_string(),
            description: "Consumer group ID".to_string(),
            required: true,
        },
        Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Topic name".to_string(),
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
            description: "Committed offset".to_string(),
            required: true,
        },
    ])
});

/// Kafka protocol implementation
pub struct KafkaProtocol;

impl KafkaProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Helper functions for action definitions

fn publish_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "publish_message".to_string(),
        description: "Publish a message to a Kafka topic from the server side".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name to publish to".to_string(),
                required: true,
            },
            Parameter {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Message key (optional, for partitioning)".to_string(),
                required: false,
            },
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "Message value/payload".to_string(),
                required: true,
            },
            Parameter {
                name: "partition".to_string(),
                type_hint: "number".to_string(),
                description: "Target partition (optional, defaults to key-based routing)"
                    .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "publish_message",
            "topic": "orders",
            "key": "order123",
            "value": "{\"item\": \"laptop\", \"price\": 999}",
            "partition": 0
        }),
    }
}

fn create_topic_action() -> ActionDefinition {
    ActionDefinition {
        name: "create_topic".to_string(),
        description: "Create a new Kafka topic".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name".to_string(),
                required: true,
            },
            Parameter {
                name: "partitions".to_string(),
                type_hint: "number".to_string(),
                description: "Number of partitions (default: 1)".to_string(),
                required: false,
            },
            Parameter {
                name: "replication_factor".to_string(),
                type_hint: "number".to_string(),
                description: "Replication factor (default: 1)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "create_topic",
            "topic": "orders",
            "partitions": 3,
            "replication_factor": 1
        }),
    }
}

fn delete_topic_action() -> ActionDefinition {
    ActionDefinition {
        name: "delete_topic".to_string(),
        description: "Delete a Kafka topic".to_string(),
        parameters: vec![Parameter {
            name: "topic".to_string(),
            type_hint: "string".to_string(),
            description: "Topic name to delete".to_string(),
            required: true,
        }],
        example: json!({
            "type": "delete_topic",
            "topic": "orders"
        }),
    }
}

fn set_retention_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_retention".to_string(),
        description: "Set retention policy for a topic".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name".to_string(),
                required: true,
            },
            Parameter {
                name: "retention_hours".to_string(),
                type_hint: "number".to_string(),
                description: "Retention time in hours".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "set_retention",
            "topic": "orders",
            "retention_hours": 72
        }),
    }
}

fn produce_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "produce_response".to_string(),
        description: "Respond to a Kafka produce request".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name".to_string(),
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
                description: "Assigned offset for the record".to_string(),
                required: true,
            },
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "Kafka error code (0 = success)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "produce_response",
            "topic": "orders",
            "partition": 0,
            "offset": 42,
            "error_code": 0
        }),
    }
}

fn fetch_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "fetch_response".to_string(),
        description: "Respond to a Kafka fetch request with records".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name".to_string(),
                required: true,
            },
            Parameter {
                name: "partition".to_string(),
                type_hint: "number".to_string(),
                description: "Partition number".to_string(),
                required: true,
            },
            Parameter {
                name: "records".to_string(),
                type_hint: "array".to_string(),
                description: "Array of records to return [{offset, key, value}]".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "fetch_response",
            "topic": "orders",
            "partition": 0,
            "records": [
                {"offset": 40, "key": "order123", "value": "{\"item\": \"laptop\"}"},
                {"offset": 41, "key": "order124", "value": "{\"item\": \"mouse\"}"}
            ]
        }),
    }
}

fn metadata_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "metadata_response".to_string(),
        description: "Respond with cluster and topic metadata".to_string(),
        parameters: vec![
            Parameter {
                name: "brokers".to_string(),
                type_hint: "array".to_string(),
                description: "Array of broker info [{id, host, port}]".to_string(),
                required: true,
            },
            Parameter {
                name: "topics".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Array of topics [{name, partitions: [{partition, leader, replicas}]}]"
                        .to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "metadata_response",
            "brokers": [{"id": 0, "host": "localhost", "port": 9092}],
            "topics": [
                {
                    "name": "orders",
                    "partitions": [{"partition": 0, "leader": 0, "replicas": [0]}]
                }
            ]
        }),
    }
}

fn offset_commit_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "offset_commit_response".to_string(),
        description: "Acknowledge offset commit".to_string(),
        parameters: vec![
            Parameter {
                name: "topic".to_string(),
                type_hint: "string".to_string(),
                description: "Topic name".to_string(),
                required: true,
            },
            Parameter {
                name: "partition".to_string(),
                type_hint: "number".to_string(),
                description: "Partition number".to_string(),
                required: true,
            },
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "Kafka error code (0 = success)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "offset_commit_response",
            "topic": "orders",
            "partition": 0,
            "error_code": 0
        }),
    }
}

fn error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "error_response".to_string(),
        description: "Respond with an error".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "Kafka error code (e.g., 3 = Unknown topic)".to_string(),
                required: true,
            },
            Parameter {
                name: "error_message".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error description".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "error_response",
            "error_code": 3,
            "error_message": "Unknown topic or partition"
        }),
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for KafkaProtocol {
        fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
            vec![
                ParameterDefinition {
                    name: "cluster_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Unique cluster identifier".to_string(),
                    required: false,
                    example: json!("netget-kafka-1"),
                },
                ParameterDefinition {
                    name: "broker_id".to_string(),
                    type_hint: "number".to_string(),
                    description: "Broker ID within the cluster".to_string(),
                    required: false,
                    example: json!(0),
                },
                ParameterDefinition {
                    name: "auto_create_topics".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Automatically create topics on first produce".to_string(),
                    required: false,
                    example: json!(true),
                },
                ParameterDefinition {
                    name: "default_partitions".to_string(),
                    type_hint: "number".to_string(),
                    description: "Default partition count for auto-created topics".to_string(),
                    required: false,
                    example: json!(1),
                },
                ParameterDefinition {
                    name: "log_retention_hours".to_string(),
                    type_hint: "number".to_string(),
                    description: "Log retention time in hours".to_string(),
                    required: false,
                    example: json!(168),
                },
            ]
        }
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                publish_message_action(),
                create_topic_action(),
                delete_topic_action(),
                set_retention_action(),
            ]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                produce_response_action(),
                fetch_response_action(),
                metadata_response_action(),
                offset_commit_response_action(),
                error_response_action(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "KAFKA"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                PRODUCE_REQUEST_EVENT.clone(),
                FETCH_REQUEST_EVENT.clone(),
                METADATA_REQUEST_EVENT.clone(),
                OFFSET_COMMIT_REQUEST_EVENT.clone(),
            ]
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>KAFKA"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["kafka", "kafka broker", "via kafka"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("kafka-protocol v0.13 wire format, manual broker logic")
                .llm_control("Message routing, topic management, consumer offsets")
                .e2e_testing("kafka-client / rdkafka")
                .notes("Core APIs only, in-memory storage, no replication")
                .build()
        }
        fn description(&self) -> &'static str {
            "Apache Kafka broker for distributed message streaming"
        }
        fn example_prompt(&self) -> &'static str {
            "Start a Kafka broker on port 9092 with topics 'orders' and 'events'. Accept all produce requests and return the last 10 messages on fetch."
        }
        fn group_name(&self) -> &'static str {
            "Database"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for KafkaProtocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
        {
            Box::pin(async move {
                use crate::server::kafka::KafkaServer;
                KafkaServer::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.server_id,
                    ctx.startup_params,
                )
                .await
            })
        }
        fn execute_action(&self, action: Value) -> Result<ActionResult> {
            let action_type = action
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'type' field in action"))?;
    
            match action_type {
                // Async actions - return Custom result for async execution
                "publish_message" | "create_topic" | "delete_topic" | "set_retention" => {
                    Ok(ActionResult::Custom {
                        name: action_type.to_string(),
                        data: action,
                    })
                }
                // Sync actions - return Custom result for protocol handler
                "produce_response"
                | "fetch_response"
                | "metadata_response"
                | "offset_commit_response"
                | "error_response" => Ok(ActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                }),
                _ => Err(anyhow!("Unknown Kafka action type: {}", action_type)),
            }
        }
}

