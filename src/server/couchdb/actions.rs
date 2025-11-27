//! CouchDB protocol actions and event types
//!
//! Defines the actions the LLM can take in response to CouchDB API requests.

use crate::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use crate::llm::actions::{ActionDefinition, Parameter};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// CouchDB protocol handler
pub struct CouchDbProtocol {
    // Could store connection state here if needed
}

impl CouchDbProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

/// CouchDB request event - triggered when a CouchDB API request is received
pub static COUCHDB_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "couchdb_request",
        "CouchDB API request received",
        json!({
            "type": "send_server_info",
            "version": "3.5.1"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (GET, POST, PUT, DELETE, HEAD)".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path (e.g., /, /{db}, /{db}/{docid})".to_string(),
            required: true,
        },
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "Detected operation type (server_info, db_create, doc_get, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "database".to_string(),
            type_hint: "string".to_string(),
            description: "Database name (if applicable)".to_string(),
            required: false,
        },
        Parameter {
            name: "doc_id".to_string(),
            type_hint: "string".to_string(),
            description: "Document ID (if applicable)".to_string(),
            required: false,
        },
        Parameter {
            name: "query_params".to_string(),
            type_hint: "object".to_string(),
            description: "URL query parameters".to_string(),
            required: false,
        },
        Parameter {
            name: "request_body".to_string(),
            type_hint: "string".to_string(),
            description: "JSON request body".to_string(),
            required: false,
        },
        Parameter {
            name: "authorization".to_string(),
            type_hint: "string".to_string(),
            description: "Authorization header (for basic auth)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_couchdb_response_action(),
        send_server_info_action(),
        send_db_info_action(),
        send_doc_response_action(),
        send_all_dbs_action(),
        send_all_docs_action(),
        send_bulk_docs_response_action(),
        send_view_response_action(),
        send_changes_response_action(),
        send_replication_response_action(),
        send_auth_required_action(),
    ])
});

fn send_couchdb_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_couchdb_response".to_string(),
        description: "Send generic CouchDB JSON response with HTTP status code".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (200, 201, 400, 401, 404, 409, 500, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "JSON response body".to_string(),
                required: true,
            },
            Parameter {
                name: "etag".to_string(),
                type_hint: "string".to_string(),
                description: "ETag header value (for document revisions)".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_couchdb_response",
            "status_code": 200,
            "body": "{\"ok\": true}",
            "etag": "\"1-abc123\""
        }),
    }
}

fn send_server_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_server_info".to_string(),
        description: "Send CouchDB server welcome/info response (GET /)".to_string(),
        parameters: vec![
            Parameter {
                name: "version".to_string(),
                type_hint: "string".to_string(),
                description: "CouchDB version number".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_server_info",
            "version": "3.5.1"
        }),
    }
}

fn send_db_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_db_info".to_string(),
        description: "Send database information response".to_string(),
        parameters: vec![
            Parameter {
                name: "db_name".to_string(),
                type_hint: "string".to_string(),
                description: "Database name".to_string(),
                required: true,
            },
            Parameter {
                name: "doc_count".to_string(),
                type_hint: "number".to_string(),
                description: "Number of documents in database".to_string(),
                required: false,
            },
            Parameter {
                name: "update_seq".to_string(),
                type_hint: "string".to_string(),
                description: "Update sequence number".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_db_info",
            "db_name": "mydb",
            "doc_count": 42,
            "update_seq": "42-abc"
        }),
    }
}

fn send_doc_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_doc_response".to_string(),
        description: "Send document response (GET, PUT, POST, DELETE)".to_string(),
        parameters: vec![
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether the operation succeeded".to_string(),
                required: true,
            },
            Parameter {
                name: "doc_id".to_string(),
                type_hint: "string".to_string(),
                description: "Document ID".to_string(),
                required: true,
            },
            Parameter {
                name: "rev".to_string(),
                type_hint: "string".to_string(),
                description: "Document revision (format: seq-hash, e.g., 1-abc123)".to_string(),
                required: true,
            },
            Parameter {
                name: "document".to_string(),
                type_hint: "object".to_string(),
                description: "Document data (for GET requests)".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "Error type (conflict, not_found, etc.)".to_string(),
                required: false,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error reason".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_doc_response",
            "success": true,
            "doc_id": "user1",
            "rev": "1-abc123",
            "document": {"name": "Alice", "age": 30}
        }),
    }
}

fn send_all_dbs_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_all_dbs".to_string(),
        description: "Send list of all databases (GET /_all_dbs)".to_string(),
        parameters: vec![
            Parameter {
                name: "databases".to_string(),
                type_hint: "array".to_string(),
                description: "Array of database names".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_all_dbs",
            "databases": ["_replicator", "_users", "mydb", "testdb"]
        }),
    }
}

fn send_all_docs_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_all_docs".to_string(),
        description: "Send list of all documents in database (GET /{db}/_all_docs)".to_string(),
        parameters: vec![
            Parameter {
                name: "total_rows".to_string(),
                type_hint: "number".to_string(),
                description: "Total number of documents".to_string(),
                required: true,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of document rows with id, key, value (rev)".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_all_docs",
            "total_rows": 2,
            "rows": [
                {"id": "doc1", "key": "doc1", "value": {"rev": "1-abc"}},
                {"id": "doc2", "key": "doc2", "value": {"rev": "1-def"}}
            ]
        }),
    }
}

fn send_bulk_docs_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bulk_docs_response".to_string(),
        description: "Send bulk documents operation results (POST /{db}/_bulk_docs)".to_string(),
        parameters: vec![
            Parameter {
                name: "results".to_string(),
                type_hint: "array".to_string(),
                description: "Array of results with ok, id, rev for each document".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_bulk_docs_response",
            "results": [
                {"ok": true, "id": "doc1", "rev": "1-abc"},
                {"ok": true, "id": "doc2", "rev": "1-def"}
            ]
        }),
    }
}

fn send_view_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_view_response".to_string(),
        description: "Send view query results (MapReduce)".to_string(),
        parameters: vec![
            Parameter {
                name: "total_rows".to_string(),
                type_hint: "number".to_string(),
                description: "Total number of rows".to_string(),
                required: true,
            },
            Parameter {
                name: "offset".to_string(),
                type_hint: "number".to_string(),
                description: "Offset in result set".to_string(),
                required: false,
            },
            Parameter {
                name: "rows".to_string(),
                type_hint: "array".to_string(),
                description: "Array of view rows with id, key, value".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "send_view_response",
            "total_rows": 2,
            "offset": 0,
            "rows": [
                {"id": "user1", "key": 25, "value": "Alice"},
                {"id": "user2", "key": 30, "value": "Bob"}
            ]
        }),
    }
}

fn send_changes_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_changes_response".to_string(),
        description: "Send changes feed response (GET /{db}/_changes)".to_string(),
        parameters: vec![
            Parameter {
                name: "results".to_string(),
                type_hint: "array".to_string(),
                description: "Array of change events with seq, id, changes".to_string(),
                required: true,
            },
            Parameter {
                name: "last_seq".to_string(),
                type_hint: "string".to_string(),
                description: "Last sequence number".to_string(),
                required: true,
            },
            Parameter {
                name: "pending".to_string(),
                type_hint: "number".to_string(),
                description: "Number of pending changes".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_changes_response",
            "results": [
                {"seq": "1-abc", "id": "doc1", "changes": [{"rev": "1-xyz"}]},
                {"seq": "2-def", "id": "doc2", "changes": [{"rev": "1-uvw"}]}
            ],
            "last_seq": "2-def",
            "pending": 0
        }),
    }
}

fn send_replication_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_replication_response".to_string(),
        description: "Send replication protocol response".to_string(),
        parameters: vec![
            Parameter {
                name: "history".to_string(),
                type_hint: "array".to_string(),
                description: "Replication history".to_string(),
                required: false,
            },
            Parameter {
                name: "session_id".to_string(),
                type_hint: "string".to_string(),
                description: "Replication session ID".to_string(),
                required: false,
            },
            Parameter {
                name: "source_last_seq".to_string(),
                type_hint: "string".to_string(),
                description: "Source database last sequence".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_replication_response",
            "session_id": "abc123",
            "source_last_seq": "10-xyz"
        }),
    }
}

fn send_auth_required_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_auth_required".to_string(),
        description: "Send 401 Unauthorized response (basic auth required)".to_string(),
        parameters: vec![
            Parameter {
                name: "realm".to_string(),
                type_hint: "string".to_string(),
                description: "Authentication realm".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "send_auth_required",
            "realm": "CouchDB"
        }),
    }
}

pub fn get_couchdb_event_types() -> Vec<EventType> {
    vec![COUCHDB_REQUEST_EVENT.clone()]
}

// Implement Protocol trait (common functionality)
impl Protocol for CouchDbProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "enable_auth".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable HTTP basic authentication (default: false)".to_string(),
                required: false,
                example: serde_json::json!(false),
            },
            crate::llm::actions::ParameterDefinition {
                name: "admin_username".to_string(),
                type_hint: "string".to_string(),
                description: "Admin username (if auth enabled)".to_string(),
                required: false,
                example: serde_json::json!("admin"),
            },
            crate::llm::actions::ParameterDefinition {
                name: "admin_password".to_string(),
                type_hint: "string".to_string(),
                description: "Admin password (if auth enabled)".to_string(),
                required: false,
                example: serde_json::json!("password"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // No async actions for CouchDB currently
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_couchdb_response_action(),
            send_server_info_action(),
            send_db_info_action(),
            send_doc_response_action(),
            send_all_dbs_action(),
            send_all_docs_action(),
            send_bulk_docs_response_action(),
            send_view_response_action(),
            send_changes_response_action(),
            send_replication_response_action(),
            send_auth_required_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "CouchDB"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_couchdb_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>COUCHDB"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["couchdb", "nosql", "document-database"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.5 HTTP server with CouchDB REST API")
            .llm_control("Database CRUD, document CRUD, views, changes, replication")
            .e2e_testing("couch_rs client library")
            .notes("Virtual data (no persistence), LLM-controlled revisions and views")
            .build()
    }

    fn description(&self) -> &'static str {
        "CouchDB document database"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a CouchDB server on port 5984"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for CouchDbProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::couchdb::CouchDbServer;

            let enable_auth = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("enable_auth"))
                .unwrap_or(false);

            let admin_username = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("admin_username"))
                .unwrap_or_else(|| "admin".to_string());

            let admin_password = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("admin_password"))
                .unwrap_or_else(|| "password".to_string());

            CouchDbServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                enable_auth,
                admin_username,
                admin_password,
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
            "send_couchdb_response" => {
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

                let etag = action
                    .get("etag")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": status_code,
                        "body": body,
                        "etag": etag
                    }),
                })
            }
            "send_server_info" => {
                let version = action
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("3.5.1");

                let response = serde_json::json!({
                    "couchdb": "Welcome",
                    "version": version,
                    "git_sha": "netget-llm",
                    "uuid": "netget-couchdb-uuid",
                    "features": [
                        "access-ready",
                        "partitioned",
                        "pluggable-storage-engines",
                        "reshard",
                        "scheduler"
                    ],
                    "vendor": {
                        "name": "NetGet LLM CouchDB"
                    }
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_db_info" => {
                let db_name = action
                    .get("db_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing db_name"))?;

                let doc_count = action
                    .get("doc_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let update_seq = action
                    .get("update_seq")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0");

                let response = serde_json::json!({
                    "db_name": db_name,
                    "update_seq": update_seq,
                    "sizes": {
                        "file": 0,
                        "external": 0,
                        "active": 0
                    },
                    "purge_seq": 0,
                    "doc_del_count": 0,
                    "doc_count": doc_count,
                    "disk_format_version": 8,
                    "compact_running": false,
                    "cluster": {
                        "q": 2,
                        "n": 1,
                        "w": 1,
                        "r": 1
                    },
                    "instance_start_time": "0"
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_doc_response" => {
                let success = action
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let doc_id = action
                    .get("doc_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;

                let rev = action
                    .get("rev")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing rev"))?;

                if success {
                    let document = action.get("document").cloned();

                    let response = if let Some(doc) = document {
                        // GET request - return full document
                        let mut doc_obj = doc.clone();
                        if let Some(obj) = doc_obj.as_object_mut() {
                            obj.insert("_id".to_string(), json!(doc_id));
                            obj.insert("_rev".to_string(), json!(rev));
                        }
                        doc_obj
                    } else {
                        // PUT/POST/DELETE request - return confirmation
                        json!({
                            "ok": true,
                            "id": doc_id,
                            "rev": rev
                        })
                    };

                    Ok(ActionResult::Custom {
                        name: "couchdb_response".to_string(),
                        data: json!({
                            "status": 200,
                            "body": serde_json::to_string_pretty(&response).unwrap(),
                            "etag": format!("\"{}\"", rev)
                        }),
                    })
                } else {
                    // Error response (conflict, not_found, etc.)
                    let error = action
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown_error");

                    let reason = action
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");

                    let status = match error {
                        "not_found" => 404,
                        "conflict" => 409,
                        "unauthorized" => 401,
                        "forbidden" => 403,
                        _ => 400,
                    };

                    let response = json!({
                        "error": error,
                        "reason": reason
                    });

                    Ok(ActionResult::Custom {
                        name: "couchdb_response".to_string(),
                        data: json!({
                            "status": status,
                            "body": serde_json::to_string_pretty(&response).unwrap()
                        }),
                    })
                }
            }
            "send_all_dbs" => {
                let databases = action
                    .get("databases")
                    .ok_or_else(|| anyhow::anyhow!("Missing databases"))?;

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&databases).unwrap()
                    }),
                })
            }
            "send_all_docs" => {
                let total_rows = action
                    .get("total_rows")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let rows = action
                    .get("rows")
                    .ok_or_else(|| anyhow::anyhow!("Missing rows"))?;

                let response = json!({
                    "total_rows": total_rows,
                    "offset": 0,
                    "rows": rows
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_bulk_docs_response" => {
                let results = action
                    .get("results")
                    .ok_or_else(|| anyhow::anyhow!("Missing results"))?;

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 201,
                        "body": serde_json::to_string_pretty(&results).unwrap()
                    }),
                })
            }
            "send_view_response" => {
                let total_rows = action
                    .get("total_rows")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let offset = action
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let rows = action
                    .get("rows")
                    .ok_or_else(|| anyhow::anyhow!("Missing rows"))?;

                let response = json!({
                    "total_rows": total_rows,
                    "offset": offset,
                    "rows": rows
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_changes_response" => {
                let results = action
                    .get("results")
                    .ok_or_else(|| anyhow::anyhow!("Missing results"))?;

                let last_seq = action
                    .get("last_seq")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0");

                let pending = action
                    .get("pending")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let response = json!({
                    "results": results,
                    "last_seq": last_seq,
                    "pending": pending
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_replication_response" => {
                let history = action.get("history").cloned().unwrap_or_else(|| json!([]));
                let session_id = action
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("abc123");
                let source_last_seq = action
                    .get("source_last_seq")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0");

                let response = json!({
                    "ok": true,
                    "session_id": session_id,
                    "source_last_seq": source_last_seq,
                    "history": history
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 200,
                        "body": serde_json::to_string_pretty(&response).unwrap()
                    }),
                })
            }
            "send_auth_required" => {
                let realm = action
                    .get("realm")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CouchDB");

                let response = json!({
                    "error": "unauthorized",
                    "reason": "Authentication required"
                });

                Ok(ActionResult::Custom {
                    name: "couchdb_response".to_string(),
                    data: json!({
                        "status": 401,
                        "body": serde_json::to_string_pretty(&response).unwrap(),
                        "www_authenticate": format!("Basic realm=\"{}\"", realm)
                    }),
                })
            }
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
