//! S3 protocol actions and event types
//!
//! Defines the actions the LLM can take in response to S3 API requests.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// S3 protocol handler
pub struct S3Protocol {
    // Could store connection state here if needed
}

impl S3Protocol {
    pub fn new() -> Self {
        Self {}
    }
}

/// S3 request event - triggered when an S3 API request is received
pub static S3_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("s3_request", "S3 API request received", json!({"type": "placeholder", "event_id": "s3_request"}))
        .with_parameters(vec![
            Parameter {
                name: "operation".to_string(),
                type_hint: "string".to_string(),
                description: "S3 operation (GetObject, PutObject, ListBuckets, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "bucket".to_string(),
                type_hint: "string".to_string(),
                description: "Bucket name (if applicable)".to_string(),
                required: false,
            },
            Parameter {
                name: "key".to_string(),
                type_hint: "string".to_string(),
                description: "Object key/path (if applicable)".to_string(),
                required: false,
            },
            Parameter {
                name: "request_details".to_string(),
                type_hint: "object".to_string(),
                description: "Additional request details (headers, query params, etc.)".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            send_s3_object_action(),
            send_s3_object_list_action(),
            send_s3_bucket_list_action(),
            send_s3_error_action(),
            show_message_action(),
        ])
});

fn send_s3_object_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_s3_object".to_string(),
        description: "Send S3 object content in response to GetObject request".to_string(),
        parameters: vec![
            Parameter {
                name: "content".to_string(),
                type_hint: "string".to_string(),
                description: "Object content (will be base64-decoded if needed)".to_string(),
                required: true,
            },
            Parameter {
                name: "content_type".to_string(),
                type_hint: "string".to_string(),
                description: "Content-Type header (e.g., 'text/plain', 'application/json')"
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "etag".to_string(),
                type_hint: "string".to_string(),
                description: "ETag for the object".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_s3_object",
            "content": "Hello, World!",
            "content_type": "text/plain",
            "etag": "\"d41d8cd98f00b204e9800998ecf8427e\""
        }),
    }
}

fn send_s3_object_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_s3_object_list".to_string(),
        description: "Send list of objects in bucket (ListObjects response)".to_string(),
        parameters: vec![
            Parameter {
                name: "objects".to_string(),
                type_hint: "array".to_string(),
                description: "Array of objects with 'key', 'size', 'last_modified' fields"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "is_truncated".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether there are more objects to list".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_s3_object_list",
            "objects": [
                {"key": "file1.txt", "size": 1024, "last_modified": "2024-01-01T00:00:00Z"},
                {"key": "file2.jpg", "size": 2048, "last_modified": "2024-01-02T00:00:00Z"}
            ],
            "is_truncated": false
        }),
    }
}

fn send_s3_bucket_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_s3_bucket_list".to_string(),
        description: "Send list of buckets (ListBuckets response)".to_string(),
        parameters: vec![Parameter {
            name: "buckets".to_string(),
            type_hint: "array".to_string(),
            description: "Array of buckets with 'name', 'creation_date' fields".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_s3_bucket_list",
            "buckets": [
                {"name": "my-bucket", "creation_date": "2024-01-01T00:00:00Z"},
                {"name": "test-bucket", "creation_date": "2024-01-02T00:00:00Z"}
            ]
        }),
    }
}

fn send_s3_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_s3_error".to_string(),
        description: "Send S3 error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "string".to_string(),
                description: "S3 error code (NoSuchBucket, NoSuchKey, AccessDenied, etc.)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (404, 403, 500, etc.)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_s3_error",
            "error_code": "NoSuchKey",
            "message": "The specified key does not exist",
            "status_code": 404
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
        example: json!({
            "type": "show_message",
            "message": "Stored object in bucket"
        }),
    }
}

pub fn get_s3_event_types() -> Vec<EventType> {
    vec![S3_REQUEST_EVENT.clone()]
}

impl crate::llm::actions::protocol_trait::Protocol for S3Protocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "port".to_string(),
                type_hint: "number".to_string(),
                description: "Port to listen on (default: 9000)".to_string(),
                required: false,
                example: json!(9000),
            },
            ParameterDefinition {
                name: "require_authentication".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether to require AWS Signature V4 authentication".to_string(),
                required: false,
                example: json!(false),
            },
            ParameterDefinition {
                name: "access_key".to_string(),
                type_hint: "string".to_string(),
                description: "AWS access key for authentication (if enabled)".to_string(),
                required: false,
                example: json!("minioadmin"),
            },
            ParameterDefinition {
                name: "secret_key".to_string(),
                type_hint: "string".to_string(),
                description: "AWS secret key for authentication (if enabled)".to_string(),
                required: false,
                example: json!("minioadmin"),
            },
            ParameterDefinition {
                name: "region".to_string(),
                type_hint: "string".to_string(),
                description: "S3 region name (default: us-east-1)".to_string(),
                required: false,
                example: json!("us-east-1"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // No async actions for S3 currently
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_s3_object_action(),
            send_s3_object_list_action(),
            send_s3_bucket_list_action(),
            send_s3_error_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "S3"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_s3_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>S3"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["s3", "object storage", "minio"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.5 HTTP with manual S3 REST API")
            .llm_control("All S3 operations (GetObject, PutObject, ListBuckets)")
            .e2e_testing("aws-sdk-s3 / rust-s3 client")
            .notes("Virtual objects (no persistence)")
            .build()
    }

    fn description(&self) -> &'static str {
        "S3-compatible object storage server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an S3-compatible server on port 9000 with a test-bucket containing hello.txt"
    }

    fn group_name(&self) -> &'static str {
        "Web & File"
    }
}

impl Server for S3Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::s3::S3Server;
            S3Server::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
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
            "send_s3_object" => {
                let content = action
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing content"))?
                    .to_string();

                let content_type = action
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let etag = action
                    .get("etag")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ActionResult::Custom {
                    name: "s3_object".to_string(),
                    data: json!({
                        "content": content,
                        "content_type": content_type,
                        "etag": etag
                    }),
                })
            }
            "send_s3_object_list" => {
                let objects = action
                    .get("objects")
                    .ok_or_else(|| anyhow::anyhow!("Missing objects"))?
                    .clone();

                let is_truncated = action
                    .get("is_truncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                Ok(ActionResult::Custom {
                    name: "s3_object_list".to_string(),
                    data: json!({
                        "objects": objects,
                        "is_truncated": is_truncated
                    }),
                })
            }
            "send_s3_bucket_list" => {
                let buckets = action
                    .get("buckets")
                    .ok_or_else(|| anyhow::anyhow!("Missing buckets"))?
                    .clone();

                Ok(ActionResult::Custom {
                    name: "s3_bucket_list".to_string(),
                    data: json!({
                        "buckets": buckets
                    }),
                })
            }
            "send_s3_error" => {
                let error_code = action
                    .get("error_code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing error_code"))?
                    .to_string();

                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing message"))?
                    .to_string();

                let status_code = action
                    .get("status_code")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid status_code"))?
                    as u16;

                Ok(ActionResult::Custom {
                    name: "s3_error".to_string(),
                    data: json!({
                        "error_code": error_code,
                        "message": message,
                        "status_code": status_code
                    }),
                })
            }
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
