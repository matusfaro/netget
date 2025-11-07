//! Elasticsearch client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Elasticsearch client connected event
pub static ELASTICSEARCH_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "elasticsearch_connected",
        "Elasticsearch client initialized and ready to execute operations"
    )
    .with_parameters(vec![
        Parameter {
            name: "cluster_url".to_string(),
            type_hint: "string".to_string(),
            description: "Elasticsearch cluster URL".to_string(),
            required: true,
        },
    ])
});

/// Elasticsearch client response received event
pub static ELASTICSEARCH_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "elasticsearch_response_received",
        "Elasticsearch response received from cluster"
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "Operation type (index, search, delete, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "status_code".to_string(),
            type_hint: "number".to_string(),
            description: "HTTP status code".to_string(),
            required: true,
        },
        Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Response body as JSON".to_string(),
            required: true,
        },
    ])
});

/// Elasticsearch client protocol action handler
pub struct ElasticsearchClientProtocol;

impl ElasticsearchClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for ElasticsearchClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::elasticsearch::ElasticsearchClient;
            ElasticsearchClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "username".to_string(),
                description: "Username for authentication (optional)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("elastic"),
            },
            ParameterDefinition {
                name: "password".to_string(),
                description: "Password for authentication (optional)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("changeme"),
            },
            ParameterDefinition {
                name: "default_index".to_string(),
                description: "Default index for operations".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("my-index"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "index_document".to_string(),
                description: "Index a document into Elasticsearch".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Document ID (optional, will be generated if not provided)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "document".to_string(),
                        type_hint: "object".to_string(),
                        description: "Document to index as JSON object".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "index_document",
                    "index": "users",
                    "id": "1",
                    "document": {
                        "name": "John Doe",
                        "email": "john@example.com",
                        "age": 30
                    }
                }),
            },
            ActionDefinition {
                name: "search".to_string(),
                description: "Search documents in Elasticsearch".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name (or comma-separated list)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "query".to_string(),
                        type_hint: "object".to_string(),
                        description: "Search query in Elasticsearch Query DSL format".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "search",
                    "index": "users",
                    "query": {
                        "match": {
                            "name": "John"
                        }
                    }
                }),
            },
            ActionDefinition {
                name: "get_document".to_string(),
                description: "Get a document by ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Document ID".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "get_document",
                    "index": "users",
                    "id": "1"
                }),
            },
            ActionDefinition {
                name: "delete_document".to_string(),
                description: "Delete a document by ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Document ID".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "delete_document",
                    "index": "users",
                    "id": "1"
                }),
            },
            ActionDefinition {
                name: "bulk_operation".to_string(),
                description: "Execute bulk operations (index, update, delete)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "operations".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of bulk operations (each contains action and optional document)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "bulk_operation",
                    "operations": [
                        {
                            "action": "index",
                            "index": "users",
                            "id": "1",
                            "document": {"name": "Alice"}
                        },
                        {
                            "action": "delete",
                            "index": "users",
                            "id": "2"
                        }
                    ]
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the Elasticsearch cluster".to_string(),
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
                name: "index_document".to_string(),
                description: "Index another document in response to search results".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "document".to_string(),
                        type_hint: "object".to_string(),
                        description: "Document to index".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "index_document",
                    "index": "logs",
                    "document": {"level": "info", "message": "Search executed"}
                }),
            },
            ActionDefinition {
                name: "search".to_string(),
                description: "Perform another search based on results".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "string".to_string(),
                        description: "Index name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "query".to_string(),
                        type_hint: "object".to_string(),
                        description: "Search query".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "search",
                    "index": "users",
                    "query": {"match_all": {}}
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "index_document" => {
                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .context("Missing 'index' field")?
                    .to_string();

                let id = action
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let document = action
                    .get("document")
                    .context("Missing 'document' field")?
                    .clone();

                Ok(ClientActionResult::Custom {
                    name: "index_document".to_string(),
                    data: json!({
                        "index": index,
                        "id": id,
                        "document": document,
                    }),
                })
            }
            "search" => {
                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .context("Missing 'index' field")?
                    .to_string();

                let query = action
                    .get("query")
                    .context("Missing 'query' field")?
                    .clone();

                Ok(ClientActionResult::Custom {
                    name: "search".to_string(),
                    data: json!({
                        "index": index,
                        "query": query,
                    }),
                })
            }
            "get_document" => {
                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .context("Missing 'index' field")?
                    .to_string();

                let id = action
                    .get("id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'id' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "get_document".to_string(),
                    data: json!({
                        "index": index,
                        "id": id,
                    }),
                })
            }
            "delete_document" => {
                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .context("Missing 'index' field")?
                    .to_string();

                let id = action
                    .get("id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'id' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "delete_document".to_string(),
                    data: json!({
                        "index": index,
                        "id": id,
                    }),
                })
            }
            "bulk_operation" => {
                let operations = action
                    .get("operations")
                    .and_then(|v| v.as_array())
                    .context("Missing 'operations' field")?
                    .clone();

                Ok(ClientActionResult::Custom {
                    name: "bulk_operation".to_string(),
                    data: json!({
                        "operations": operations,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown Elasticsearch client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Elasticsearch"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "elasticsearch_connected".to_string(),
                description: "Triggered when Elasticsearch client is initialized".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "elasticsearch_response_received".to_string(),
                description: "Triggered when Elasticsearch client receives a response".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>Elasticsearch"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "elasticsearch",
            "es",
            "elastic",
            "search",
            "index",
            "elasticsearch client",
            "connect to elasticsearch",
        ]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("elasticsearch official Rust crate (HTTP-based)")
            .llm_control("Full control over indexing, searching, and document operations")
            .e2e_testing("Local Elasticsearch instance or Docker container")
            .build()
    }

    fn description(&self) -> &'static str {
        "Elasticsearch client for search and analytics operations"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to http://localhost:9200 and index a document"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}
