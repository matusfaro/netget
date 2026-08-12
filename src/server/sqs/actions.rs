//! AWS SQS protocol actions and event types
//!
//! Defines the actions the LLM can take in response to SQS API requests.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::LazyLock;

/// SQS protocol handler
pub struct SqsProtocol {
    // Could store connection state here if needed
}

impl SqsProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

/// SQS request event - triggered when an SQS API request is received
pub static SQS_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "sqs_request",
        "SQS API request received",
        json!({"type": "placeholder", "event_id": "sqs_request"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "SQS operation (SendMessage, ReceiveMessage, CreateQueue, etc.)"
                .to_string(),
            required: true,
        },
        Parameter {
            name: "queue_url".to_string(),
            type_hint: "string".to_string(),
            description: "Target queue URL (if available)".to_string(),
            required: false,
        },
        Parameter {
            name: "request_body".to_string(),
            type_hint: "string".to_string(),
            description: "JSON request body".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![send_sqs_response_action()])
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} SQS {operation} {queue_url} -> {status} ({duration_ms}ms)")
            .with_debug("SQS {operation} queue={queue_url} from {client_ip}")
            .with_trace("SQS request: {json_pretty(.)}"),
    )
});

fn send_sqs_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_sqs_response".to_string(),
        description: "Send SQS JSON response with HTTP status code".to_string(),
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
            "type": "send_sqs_response",
            "status_code": 200,
            "body": "{\"MessageId\": \"msg-123\", \"MD5OfMessageBody\": \"d41d8cd98f00b204e9800998ecf8427e\"}"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> {status_code} ({body_len}B)")
                .with_debug("SQS response: status={status_code}")
                .with_trace("SQS response body: {body}"),
        ),
    }
}

/// Read and validate the model-supplied `status_code`.
///
/// The old `as u64 as u16` cast wrapped silently, so a status of 65736 became 200 and a
/// status of 99 or 1000 reached `Response::builder().status()`, which rejects it - and the
/// `.unwrap()` on the other side of that turned a typo in model output into a panic that
/// killed the connection task. Reject it here instead, where the message reaches the model.
pub(crate) fn parse_status_code(action: &Value) -> Result<u16> {
    let raw = action
        .get("status_code")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid status_code: expected a number"))?;

    if !(100..=599).contains(&raw) {
        return Err(anyhow::anyhow!(
            "Invalid status_code {raw}: must be an HTTP status between 100 and 599. \
             SQS uses 200 for success and 400 with an {{\"__type\": ..., \"message\": ...}} \
             body for client errors."
        ));
    }

    Ok(raw as u16)
}

pub fn get_sqs_event_types() -> Vec<EventType> {
    vec![SQS_REQUEST_EVENT.clone()]
}

// Implement Protocol trait (common functionality)
impl Protocol for SqsProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        // Deliberately empty. `default_visibility_timeout`, `default_message_retention` and
        // `max_receive_count` used to be declared here, but the server holds no queue state
        // to apply them to and `spawn()` never read them - the LLM owns visibility and
        // retention behaviour entirely, through the instruction. Declaring them promised
        // enforcement that did not exist.
        vec![]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // No async actions for SQS currently (all operations are request-driven)
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_sqs_response_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "SQS"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_sqs_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>SQS"
    }
    fn keywords(&self) -> Vec<&'static str> {
        // "queue" alone is intentionally absent: it collided with AMQP. SQS is chosen by
        // its AWS-specific names and the two-word "message queue" phrase.
        vec!["sqs", "message queue", "sqs queue", "aws sqs"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.5 HTTP with AWS JSON protocol")
            .llm_control("All SQS operations (SendMessage, ReceiveMessage, DeleteMessage)")
            .e2e_testing("aws-sdk-sqs client")
            .notes("Virtual queues (no persistence); no auth; visibility timeouts are the LLM's job, the server tracks nothing")
            .build()
    }
    fn description(&self) -> &'static str {
        "AWS SQS-compatible message queue server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an AWS SQS-compatible queue server on port 9324"
    }
    fn group_name(&self) -> &'static str {
        "Database"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles all SQS responses intelligently
            json!({
                "type": "open_server",
                "port": 9324,
                "base_stack": "sqs",
                "instruction": "AWS SQS-compatible message queue handling queue operations"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 9324,
                "base_stack": "sqs",
                "event_handlers": [{
                    "event_pattern": "sqs_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<sqs_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 9324,
                "base_stack": "sqs",
                "event_handlers": [{
                    "event_pattern": "sqs_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_sqs_response",
                            "status_code": 200,
                            "body": "{}"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SqsProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::sqs::SqsServer;
            SqsServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                false,
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
            "send_sqs_response" => {
                let status_code = parse_status_code(&action)?;

                let body = action
                    .get("body")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing body"))?
                    .to_string();

                Ok(ActionResult::Custom {
                    name: "sqs_response".to_string(),
                    data: json!({
                        "status": status_code,
                        "body": body
                    }),
                })
            }
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
