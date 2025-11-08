//! DynamoDB protocol actions and event types
//!
//! Defines the actions the LLM can take in response to DynamoDB API requests.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// DynamoDB protocol handler
pub struct DynamoProtocol {
    // Could store connection state here if needed
}

impl DynamoProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

/// DynamoDB request event - triggered when a DynamoDB API request is received
pub static DYNAMO_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "dynamo_request",
        "DynamoDB API request received"
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "DynamoDB operation (GetItem, PutItem, Query, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "table_name".to_string(),
            type_hint: "string".to_string(),
            description: "Target table name (if available)".to_string(),
            required: false,
        },
        Parameter {
            name: "request_body".to_string(),
            type_hint: "string".to_string(),
            description: "JSON request body".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_dynamo_response_action(),
        show_message_action(),
    ])
});

fn send_dynamo_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dynamo_response".to_string(),
        description: "Send DynamoDB JSON response with HTTP status code".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (200, 400, 500, etc.)".to_string(),
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
            "type": "send_dynamo_response",
            "status_code": 200,
            "body": "{\"Item\": {\"id\": {\"S\": \"user-123\"}, \"name\": {\"S\": \"Alice\"}}}"
        }),
    }
}

fn show_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "show_message".to_string(),
        description: "Display a message in the TUI output panel".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Message to display".to_string(),
                required: true,
            },
        ],
        example: serde_json::json!({
            "type": "show_message",
            "message": "Stored item in Users table"
        }),
    }
}

pub fn get_dynamo_event_types() -> Vec<EventType> {
    vec![
        DYNAMO_REQUEST_EVENT.clone(),
    ]
}

// Implement Protocol trait (common functionality)
impl Protocol for DynamoProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            // No async actions for DynamoDB currently
            vec![]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                send_dynamo_response_action(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "DynamoDB"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            get_dynamo_event_types()
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>HTTP>DYNAMODB"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["dynamo"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("hyper v1.5 HTTP with manual DynamoDB API")
                .llm_control("All DynamoDB operations (GetItem, PutItem, Query)")
                .e2e_testing("aws-sdk-dynamodb client")
                .notes("Virtual data (no persistence)")
                .build()
        }
        fn description(&self) -> &'static str {
            "DynamoDB-compatible database server"
        }
        fn example_prompt(&self) -> &'static str {
            "Start a DynamoDB-compatible server on port 8000"
        }
        fn group_name(&self) -> &'static str {
            "Database"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for DynamoProtocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::server::dynamo::DynamoServer;
                DynamoServer::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    false,
                    ctx.server_id,
                ).await
            })
        }
        fn execute_action(&self, action: Value) -> Result<ActionResult> {
            let action_type = action.get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;
    
            match action_type {
                "send_dynamo_response" => {
                    let status_code = action.get("status_code")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| anyhow::anyhow!("Missing or invalid status_code"))? as u16;
    
                    let body = action.get("body")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing body"))?
                        .to_string();
    
                    Ok(ActionResult::Custom {
                        name: "dynamo_response".to_string(),
                        data: json!({
                            "status": status_code,
                            "body": body
                        }),
                    })
                }
                _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type))
            }
        }
}

