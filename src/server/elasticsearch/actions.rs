//! Elasticsearch protocol actions and event types
//!
//! Defines the actions the LLM can take in response to Elasticsearch API requests.

use crate::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use crate::llm::actions::{ActionDefinition, Parameter};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// Elasticsearch protocol handler
pub struct ElasticsearchProtocol {
    // Could store connection state here if needed
}

impl ElasticsearchProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

/// Elasticsearch request event - triggered when an Elasticsearch API request is received
pub static ELASTICSEARCH_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "elasticsearch_request",
        "Elasticsearch API request received",
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (GET, POST, PUT, DELETE)".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path (e.g., /index/_search, /_bulk)".to_string(),
            required: true,
        },
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "Detected operation type (search, index, get, delete, bulk, etc.)"
                .to_string(),
            required: true,
        },
        Parameter {
            name: "index".to_string(),
            type_hint: "string".to_string(),
            description: "Target index name (if available)".to_string(),
            required: false,
        },
        Parameter {
            name: "doc_id".to_string(),
            type_hint: "string".to_string(),
            description: "Document ID (if available)".to_string(),
            required: false,
        },
        Parameter {
            name: "request_body".to_string(),
            type_hint: "string".to_string(),
            description: "JSON request body".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_elasticsearch_response_action(),
        send_search_response_action(),
        send_index_response_action(),
        send_get_response_action(),
        send_bulk_response_action(),
        send_cluster_info_action(),
        show_message_action(),
    ])
});

fn send_elasticsearch_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_elasticsearch_response".to_string(),
        description: "Send generic Elasticsearch JSON response with HTTP status code".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (200, 400, 404, 500, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "JSON response body".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_elasticsearch_response",
            "status_code": 200,
            "body": "{\"acknowledged\": true}"
        }),
    }
}

fn send_search_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_search_response".to_string(),
        description: "Send search results with hits array".to_string(),
        parameters: vec![
            Parameter {
                name: "hits".to_string(),
                type_hint: "array".to_string(),
                description: "Array of search result documents with _id, _index, _source"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "total".to_string(),
                type_hint: "number".to_string(),
                description: "Total number of matching documents".to_string(),
                required: false,
            },
            Parameter {
                name: "took".to_string(),
                type_hint: "number".to_string(),
                description: "Time in milliseconds the search took".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_search_response",
            "hits": [
                {"_id": "1", "_index": "products", "_source": {"name": "Widget", "price": 19.99}},
                {"_id": "2", "_index": "products", "_source": {"name": "Gadget", "price": 29.99}}
            ],
            "total": 2,
            "took": 15
        }),
    }
}

fn send_index_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_index_response".to_string(),
        description: "Send document indexing confirmation".to_string(),
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
            Parameter {
                name: "result".to_string(),
                type_hint: "string".to_string(),
                description: "Result type: 'created' or 'updated'".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_index_response",
            "index": "products",
            "id": "abc123",
            "result": "created"
        }),
    }
}

fn send_get_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_get_response".to_string(),
        description: "Send document retrieval response".to_string(),
        parameters: vec![
            Parameter {
                name: "found".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether the document was found".to_string(),
                required: true,
            },
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
            Parameter {
                name: "source".to_string(),
                type_hint: "object".to_string(),
                description: "Document source data (if found)".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_get_response",
            "found": true,
            "index": "products",
            "id": "abc123",
            "source": {"name": "Widget", "price": 19.99}
        }),
    }
}

fn send_bulk_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bulk_response".to_string(),
        description: "Send bulk operation results".to_string(),
        parameters: vec![
            Parameter {
                name: "items".to_string(),
                type_hint: "array".to_string(),
                description: "Array of operation results with status for each item".to_string(),
                required: true,
            },
            Parameter {
                name: "errors".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether any operations failed".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_bulk_response",
            "items": [
                {"index": {"_index": "products", "_id": "1", "status": 201}},
                {"delete": {"_index": "products", "_id": "2", "status": 200}}
            ],
            "errors": false
        }),
    }
}

fn send_cluster_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_cluster_info".to_string(),
        description: "Send cluster information response".to_string(),
        parameters: vec![
            Parameter {
                name: "cluster_name".to_string(),
                type_hint: "string".to_string(),
                description: "Cluster name".to_string(),
                required: false,
            },
            Parameter {
                name: "status".to_string(),
                type_hint: "string".to_string(),
                description: "Cluster status: 'green', 'yellow', or 'red'".to_string(),
                required: false,
            },
            Parameter {
                name: "version".to_string(),
                type_hint: "string".to_string(),
                description: "Elasticsearch version".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_cluster_info",
            "cluster_name": "llm-elasticsearch",
            "status": "green",
            "version": "8.0.0"
        }),
    }
}

fn show_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "show_message".to_string(),
        description: "Display a message in the TUI output panel".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Message to display".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "show_message",
            "message": "Indexed document in products index"
        }),
    }
}

pub fn get_elasticsearch_event_types() -> Vec<EventType> {
    vec![ELASTICSEARCH_REQUEST_EVENT.clone()]
}

// Implement Protocol trait (common functionality)
impl Protocol for ElasticsearchProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
                crate::llm::actions::ParameterDefinition {
                    name: "send_first".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Whether the server should send the first message after connection (not typically needed for this protocol)".to_string(),
                    required: false,
                    example: serde_json::json!(false),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // No async actions for Elasticsearch currently
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_elasticsearch_response_action(),
            send_search_response_action(),
            send_index_response_action(),
            send_get_response_action(),
            send_bulk_response_action(),
            send_cluster_info_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Elasticsearch"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_elasticsearch_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>ELASTICSEARCH"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["elasticsearch", "opensearch"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.5 HTTP server with manual ES API")
            .llm_control("Search, index, cluster operations")
            .e2e_testing("curl / elasticsearch client")
            .notes("Virtual data (no persistence)")
            .build()
    }
    fn description(&self) -> &'static str {
        "Elasticsearch search engine"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an Elasticsearch server on port 9200"
    }
    fn group_name(&self) -> &'static str {
        "Database"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for ElasticsearchProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::elasticsearch::ElasticsearchServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            ElasticsearchServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;

        match action_type {
            "send_elasticsearch_response" => {
                let status_code = action
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid status_code"))?
                    as u16;

                let body = action
                    .get("body")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing body"))?
                    .to_string();

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": status_code,
                        "body": body
                    }),
                })
            }
            "send_search_response" => {
                let hits = action
                    .get("hits")
                    .ok_or_else(|| anyhow::anyhow!("Missing hits"))?;

                let total = action
                    .get("total")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_else(|| hits.as_array().map(|a| a.len() as u64).unwrap_or(0));

                let took = action.get("took").and_then(|v| v.as_u64()).unwrap_or(10);

                let response = serde_json::json!({
                    "took": took,
                    "timed_out": false,
                    "_shards": {
                        "total": 1,
                        "successful": 1,
                        "skipped": 0,
                        "failed": 0
                    },
                    "hits": {
                        "total": {
                            "value": total,
                            "relation": "eq"
                        },
                        "max_score": 1.0,
                        "hits": hits
                    }
                });

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_index_response" => {
                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing index"))?;

                let id = action
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing id"))?;

                let result = action
                    .get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("created");

                let response = serde_json::json!({
                    "_index": index,
                    "_id": id,
                    "_version": 1,
                    "result": result,
                    "_shards": {
                        "total": 2,
                        "successful": 1,
                        "failed": 0
                    },
                    "_seq_no": 0,
                    "_primary_term": 1
                });

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": if result == "created" { 201 } else { 200 },
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_get_response" => {
                let found = action
                    .get("found")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let index = action
                    .get("index")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing index"))?;

                let id = action
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing id"))?;

                let response = if found {
                    let source = action
                        .get("source")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));

                    serde_json::json!({
                        "_index": index,
                        "_id": id,
                        "_version": 1,
                        "_seq_no": 0,
                        "_primary_term": 1,
                        "found": true,
                        "_source": source
                    })
                } else {
                    serde_json::json!({
                        "_index": index,
                        "_id": id,
                        "found": false
                    })
                };

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": if found { 200 } else { 404 },
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_bulk_response" => {
                let items = action
                    .get("items")
                    .ok_or_else(|| anyhow::anyhow!("Missing items"))?;

                let errors = action
                    .get("errors")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let response = serde_json::json!({
                    "took": 10,
                    "errors": errors,
                    "items": items
                });

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_cluster_info" => {
                let cluster_name = action
                    .get("cluster_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("llm-elasticsearch");

                let _status = action
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("green");

                let version = action
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("8.0.0-llm");

                let response = serde_json::json!({
                    "name": "netget-node-1",
                    "cluster_name": cluster_name,
                    "cluster_uuid": "abcd1234",
                    "version": {
                        "number": version,
                        "build_flavor": "default",
                        "build_type": "tar",
                        "build_hash": "llm",
                        "build_date": "2025-01-01T00:00:00.000Z",
                        "build_snapshot": false,
                        "lucene_version": "9.0.0",
                        "minimum_wire_compatibility_version": "7.17.0",
                        "minimum_index_compatibility_version": "7.0.0"
                    },
                    "tagline": "You Know, for Search (powered by LLM)"
                });

                Ok(ActionResult::Custom {
                    name: "elasticsearch_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
