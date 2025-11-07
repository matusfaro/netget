//! S3 client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// S3 client connected event
pub static S3_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "s3_connected",
        "S3 client initialized and ready to access buckets"
    )
    .with_parameters(vec![
        Parameter {
            name: "endpoint".to_string(),
            type_hint: "string".to_string(),
            description: "S3 endpoint URL".to_string(),
            required: true,
        },
        Parameter {
            name: "region".to_string(),
            type_hint: "string".to_string(),
            description: "AWS region".to_string(),
            required: true,
        },
    ])
});

/// S3 client response received event
pub static S3_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "s3_response_received",
        "S3 operation completed with response"
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "S3 operation performed (put_object, get_object, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "success".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether the operation succeeded".to_string(),
            required: true,
        },
        Parameter {
            name: "result".to_string(),
            type_hint: "object".to_string(),
            description: "Operation result data".to_string(),
            required: false,
        },
        Parameter {
            name: "error".to_string(),
            type_hint: "string".to_string(),
            description: "Error message if operation failed".to_string(),
            required: false,
        },
    ])
});

/// S3 client protocol action handler
pub struct S3ClientProtocol;

impl S3ClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for S3ClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::s3::S3Client;
            S3Client::connect_with_llm_actions(
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
                name: "access_key_id".to_string(),
                description: "AWS access key ID for authentication".to_string(),
                type_hint: "string".to_string(),
                required: true,
                example: json!("AKIAIOSFODNN7EXAMPLE"),
            },
            ParameterDefinition {
                name: "secret_access_key".to_string(),
                description: "AWS secret access key for authentication".to_string(),
                type_hint: "string".to_string(),
                required: true,
                example: json!("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            },
            ParameterDefinition {
                name: "region".to_string(),
                description: "AWS region (e.g., us-east-1, eu-west-1)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("us-east-1"),
            },
            ParameterDefinition {
                name: "endpoint_url".to_string(),
                description: "Custom S3 endpoint URL (for S3-compatible services like MinIO)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("http://localhost:9000"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "put_object".to_string(),
                description: "Upload an object to an S3 bucket".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key (path/filename)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object content (text or base64 for binary)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "content_type".to_string(),
                        type_hint: "string".to_string(),
                        description: "Content type (e.g., text/plain, application/json)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "put_object",
                    "bucket": "my-bucket",
                    "key": "data/file.txt",
                    "body": "Hello, S3!",
                    "content_type": "text/plain"
                }),
            },
            ActionDefinition {
                name: "get_object".to_string(),
                description: "Download an object from an S3 bucket".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key (path/filename)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "get_object",
                    "bucket": "my-bucket",
                    "key": "data/file.txt"
                }),
            },
            ActionDefinition {
                name: "list_buckets".to_string(),
                description: "List all S3 buckets".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_buckets"
                }),
            },
            ActionDefinition {
                name: "list_objects".to_string(),
                description: "List objects in an S3 bucket".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "prefix".to_string(),
                        type_hint: "string".to_string(),
                        description: "Filter objects by prefix (folder path)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "max_keys".to_string(),
                        type_hint: "number".to_string(),
                        description: "Maximum number of keys to return".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "list_objects",
                    "bucket": "my-bucket",
                    "prefix": "data/",
                    "max_keys": 100
                }),
            },
            ActionDefinition {
                name: "delete_object".to_string(),
                description: "Delete an object from an S3 bucket".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key (path/filename)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "delete_object",
                    "bucket": "my-bucket",
                    "key": "data/file.txt"
                }),
            },
            ActionDefinition {
                name: "head_object".to_string(),
                description: "Get metadata for an object without downloading it".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key (path/filename)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "head_object",
                    "bucket": "my-bucket",
                    "key": "data/file.txt"
                }),
            },
            ActionDefinition {
                name: "create_bucket".to_string(),
                description: "Create a new S3 bucket".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "create_bucket",
                    "bucket": "my-new-bucket"
                }),
            },
            ActionDefinition {
                name: "delete_bucket".to_string(),
                description: "Delete an S3 bucket (must be empty)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "delete_bucket",
                    "bucket": "my-old-bucket"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the S3 service".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        // Same actions available in sync context (after receiving responses)
        vec![
            ActionDefinition {
                name: "put_object".to_string(),
                description: "Upload another object in response to previous operation".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object content".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "put_object",
                    "bucket": "my-bucket",
                    "key": "result.json",
                    "body": "{\"status\": \"processed\"}"
                }),
            },
            ActionDefinition {
                name: "get_object".to_string(),
                description: "Download another object in response to previous operation".to_string(),
                parameters: vec![
                    Parameter {
                        name: "bucket".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bucket name".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "key".to_string(),
                        type_hint: "string".to_string(),
                        description: "Object key".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "get_object",
                    "bucket": "my-bucket",
                    "key": "next-file.txt"
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
            "put_object" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                let key = action
                    .get("key")
                    .and_then(|v| v.as_str())
                    .context("Missing 'key' field")?
                    .to_string();

                let body = action
                    .get("body")
                    .and_then(|v| v.as_str())
                    .context("Missing 'body' field")?
                    .to_string();

                let content_type = action
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                Ok(ClientActionResult::Custom {
                    name: "s3_put_object".to_string(),
                    data: json!({
                        "bucket": bucket,
                        "key": key,
                        "body": body,
                        "content_type": content_type,
                    }),
                })
            }
            "get_object" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                let key = action
                    .get("key")
                    .and_then(|v| v.as_str())
                    .context("Missing 'key' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "s3_get_object".to_string(),
                    data: json!({
                        "bucket": bucket,
                        "key": key,
                    }),
                })
            }
            "list_buckets" => {
                Ok(ClientActionResult::Custom {
                    name: "s3_list_buckets".to_string(),
                    data: json!({}),
                })
            }
            "list_objects" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                let prefix = action
                    .get("prefix")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let max_keys = action
                    .get("max_keys")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as i32);

                Ok(ClientActionResult::Custom {
                    name: "s3_list_objects".to_string(),
                    data: json!({
                        "bucket": bucket,
                        "prefix": prefix,
                        "max_keys": max_keys,
                    }),
                })
            }
            "delete_object" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                let key = action
                    .get("key")
                    .and_then(|v| v.as_str())
                    .context("Missing 'key' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "s3_delete_object".to_string(),
                    data: json!({
                        "bucket": bucket,
                        "key": key,
                    }),
                })
            }
            "head_object" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                let key = action
                    .get("key")
                    .and_then(|v| v.as_str())
                    .context("Missing 'key' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "s3_head_object".to_string(),
                    data: json!({
                        "bucket": bucket,
                        "key": key,
                    }),
                })
            }
            "create_bucket" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "s3_create_bucket".to_string(),
                    data: json!({
                        "bucket": bucket,
                    }),
                })
            }
            "delete_bucket" => {
                let bucket = action
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .context("Missing 'bucket' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "s3_delete_bucket".to_string(),
                    data: json!({
                        "bucket": bucket,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown S3 client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "S3"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "s3_connected".to_string(),
                description: "Triggered when S3 client is initialized".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "s3_response_received".to_string(),
                description: "Triggered when S3 client receives a response".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>S3"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["s3", "s3 client", "aws s3", "object storage", "minio"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("AWS SDK for Rust (aws-sdk-s3)")
            .llm_control("Full control over S3 operations (put, get, list, delete)")
            .e2e_testing("LocalStack, MinIO, or real AWS S3")
            .build()
    }

    fn description(&self) -> &'static str {
        "S3 client for object storage operations (AWS S3 and S3-compatible services)"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to S3 at s3.amazonaws.com and list all buckets"
    }

    fn group_name(&self) -> &'static str {
        "Cloud"
    }
}
