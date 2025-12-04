//! IPP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// IPP protocol action handler
pub struct IppProtocol {}

impl IppProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IppProtocol {
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
        vec![list_print_jobs_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ipp_response_action(),
            ipp_printer_attributes_action(),
            ipp_job_attributes_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "IPP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_ipp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>IPP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["ipp", "printer", "print"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual IPP binary parsing, hyper HTTP")
            .llm_control("Printer attributes, job handling, IPP responses")
            .e2e_testing("ipp-client / curl")
            .notes("IPP/1.1 and 2.0, no CUPS extensions")
            .build()
    }
    fn description(&self) -> &'static str {
        "Internet Printing Protocol server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an IPP server on port 631"
    }
    fn group_name(&self) -> &'static str {
        "Web & File"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles all IPP responses intelligently
            json!({
                "type": "open_server",
                "port": 631,
                "base_stack": "ipp",
                "instruction": "IPP printer server handling print jobs"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 631,
                "base_stack": "ipp",
                "event_handlers": [{
                    "event_pattern": "ipp_request_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<ipp_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 631,
                "base_stack": "ipp",
                "event_handlers": [{
                    "event_pattern": "ipp_request_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "ipp_response",
                            "status": 200,
                            "body": ""
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for IppProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ipp::IppServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            IppServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "ipp_response" => self.execute_ipp_response(action),
            "ipp_printer_attributes" => self.execute_ipp_printer_attributes(action),
            "ipp_job_attributes" => self.execute_ipp_job_attributes(action),
            "list_print_jobs" => self.execute_list_print_jobs(action),
            _ => Err(anyhow::anyhow!("Unknown IPP action: {}", action_type)),
        }
    }
}

impl IppProtocol {
    fn execute_ipp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;

        let body = action.get("body").and_then(|v| v.as_str()).unwrap_or("");

        debug!("IPP response: status={}", status);

        Ok(ActionResult::Custom {
            name: "ipp_response".to_string(),
            data: json!({
                "status": status,
                "body": hex::encode(body.as_bytes())
            }),
        })
    }

    fn execute_ipp_printer_attributes(&self, action: serde_json::Value) -> Result<ActionResult> {
        let attributes = action
            .get("attributes")
            .and_then(|v| v.as_object())
            .context("Missing 'attributes' object")?;

        debug!("IPP printer attributes: {} attrs", attributes.len());

        // Build IPP response with printer attributes
        let body = build_ipp_printer_attributes_response(attributes);

        Ok(ActionResult::Custom {
            name: "ipp_response".to_string(),
            data: json!({
                "status": 200,
                "body": hex::encode(&body)
            }),
        })
    }

    fn execute_ipp_job_attributes(&self, action: serde_json::Value) -> Result<ActionResult> {
        let attributes = action
            .get("attributes")
            .and_then(|v| v.as_object())
            .context("Missing 'attributes' object")?;

        debug!("IPP job attributes: {} attrs", attributes.len());

        // Build IPP response with job attributes
        let body = build_ipp_job_attributes_response(attributes);

        Ok(ActionResult::Custom {
            name: "ipp_response".to_string(),
            data: json!({
                "status": 200,
                "body": hex::encode(&body)
            }),
        })
    }

    fn execute_list_print_jobs(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("IPP list print jobs");

        // This is a placeholder - in a real implementation, we'd track jobs
        Ok(ActionResult::Custom {
            name: "list_print_jobs".to_string(),
            data: json!({"jobs": []}),
        })
    }
}

/// Action definition: Send IPP response
pub fn ipp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ipp_response".to_string(),
        description: "Send a raw IPP response".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (200, 404, etc.)".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Raw IPP response body (base64 or string)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ipp_response",
            "status": 200,
            "body": ""
        }),
        log_template: None,
    }
}

/// Action definition: Send printer attributes response
pub fn ipp_printer_attributes_action() -> ActionDefinition {
    ActionDefinition {
        name: "ipp_printer_attributes".to_string(),
        description: "Respond to Get-Printer-Attributes with printer info".to_string(),
        parameters: vec![
            Parameter {
                name: "attributes".to_string(),
                type_hint: "object".to_string(),
                description: "Object with printer attributes like {\"printer-name\": \"My Printer\", \"printer-state\": \"idle\", \"printer-uri-supported\": [\"ipp://localhost:631/printers/my-printer\"]}".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "ipp_printer_attributes",
            "attributes": {
                "printer-name": "My Printer",
                "printer-state": "idle",
                "printer-uri-supported": ["ipp://localhost:631/printers/my-printer"]
            }
        }),
        log_template: None,
    }
}

/// Action definition: Send job attributes response
pub fn ipp_job_attributes_action() -> ActionDefinition {
    ActionDefinition {
        name: "ipp_job_attributes".to_string(),
        description: "Respond to Get-Job-Attributes with job info".to_string(),
        parameters: vec![
            Parameter {
                name: "attributes".to_string(),
                type_hint: "object".to_string(),
                description: "Object with job attributes like {\"job-id\": 123, \"job-state\": \"completed\", \"job-name\": \"document.pdf\"}".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "ipp_job_attributes",
            "attributes": {
                "job-id": 123,
                "job-state": "completed",
                "job-name": "document.pdf"
            }
        }),
        log_template: None,
    }
}

/// Action definition: List print jobs
pub fn list_print_jobs_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_print_jobs".to_string(),
        description: "List all active and completed print jobs".to_string(),
        parameters: vec![],
        example: json!({"type": "list_print_jobs"}),
        log_template: None,
    }
}

/// Build IPP printer attributes response
fn build_ipp_printer_attributes_response(
    attributes: &serde_json::Map<String, serde_json::Value>,
) -> Vec<u8> {
    // Build a minimal IPP response
    // IPP format: version(2) + status-code(2) + request-id(4) + attributes + end-tag
    let mut response = Vec::new();

    // Version 2.0 (0x0200)
    response.extend_from_slice(&[0x02, 0x00]);

    // Status code: successful-ok (0x0000)
    response.extend_from_slice(&[0x00, 0x00]);

    // Request ID (placeholder)
    response.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

    // Operation attributes tag (0x01)
    response.push(0x01);

    // charset (0x47 = charset type, 0x00 0x12 = name length)
    response.extend_from_slice(&[0x47, 0x00, 0x12]);
    response.extend_from_slice(b"attributes-charset");
    response.extend_from_slice(&[0x00, 0x05]); // value length
    response.extend_from_slice(b"utf-8");

    // natural-language (0x48 = natural-language type)
    response.extend_from_slice(&[0x48, 0x00, 0x1b]);
    response.extend_from_slice(b"attributes-natural-language");
    response.extend_from_slice(&[0x00, 0x05]); // value length
    response.extend_from_slice(b"en-us");

    // Printer attributes tag (0x04)
    response.push(0x04);

    // Add printer attributes (simplified - real implementation would use proper IPP encoding)
    for (key, value) in attributes {
        // Add as nameWithoutLanguage (0x42)
        response.push(0x42);
        let key_bytes = key.as_bytes();
        response.extend_from_slice(&[0x00, key_bytes.len() as u8]);
        response.extend_from_slice(key_bytes);

        let val_string = value.to_string();
        let val_str = value.as_str().unwrap_or(&val_string);
        let val_bytes = val_str.as_bytes();
        response.extend_from_slice(&[(val_bytes.len() >> 8) as u8, val_bytes.len() as u8]);
        response.extend_from_slice(val_bytes);
    }

    // End-of-attributes tag (0x03)
    response.push(0x03);

    response
}

/// Build IPP job attributes response
fn build_ipp_job_attributes_response(
    attributes: &serde_json::Map<String, serde_json::Value>,
) -> Vec<u8> {
    // Similar to printer attributes but with job-specific tags
    let mut response = Vec::new();

    // Version 2.0
    response.extend_from_slice(&[0x02, 0x00]);

    // Status code: successful-ok
    response.extend_from_slice(&[0x00, 0x00]);

    // Request ID
    response.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

    // Operation attributes tag
    response.push(0x01);

    // charset
    response.extend_from_slice(&[0x47, 0x00, 0x12]);
    response.extend_from_slice(b"attributes-charset");
    response.extend_from_slice(&[0x00, 0x05]);
    response.extend_from_slice(b"utf-8");

    // natural-language
    response.extend_from_slice(&[0x48, 0x00, 0x1b]);
    response.extend_from_slice(b"attributes-natural-language");
    response.extend_from_slice(&[0x00, 0x05]);
    response.extend_from_slice(b"en-us");

    // Job attributes tag (0x02)
    response.push(0x02);

    // Add job attributes
    for (key, value) in attributes {
        response.push(0x42); // nameWithoutLanguage
        let key_bytes = key.as_bytes();
        response.extend_from_slice(&[0x00, key_bytes.len() as u8]);
        response.extend_from_slice(key_bytes);

        let val_string = value.to_string();
        let val_str = value.as_str().unwrap_or(&val_string);
        let val_bytes = val_str.as_bytes();
        response.extend_from_slice(&[(val_bytes.len() >> 8) as u8, val_bytes.len() as u8]);
        response.extend_from_slice(val_bytes);
    }

    // End-of-attributes tag
    response.push(0x03);

    response
}

// ============================================================================
// IPP Action Constants
// ============================================================================

pub static IPP_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| ipp_response_action());
pub static IPP_PRINTER_ATTRIBUTES_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| ipp_printer_attributes_action());
pub static IPP_JOB_ATTRIBUTES_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| ipp_job_attributes_action());

// ============================================================================
// IPP Event Type Constants
// ============================================================================

/// IPP request event - triggered when client sends an IPP request
pub static IPP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ipp_request_received", "IPP request received from client", json!({"type": "placeholder", "event_id": "ipp_request_received"}))
        .with_parameters(vec![
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "HTTP method (usually POST)".to_string(),
                required: true,
            },
            Parameter {
                name: "uri".to_string(),
                type_hint: "string".to_string(),
                description: "Request URI".to_string(),
                required: true,
            },
            Parameter {
                name: "operation".to_string(),
                type_hint: "string".to_string(),
                description: "IPP operation name (e.g., Print-Job, Get-Printer-Attributes)"
                    .to_string(),
                required: true,
            },
        ])
        .with_actions(vec![
            IPP_RESPONSE_ACTION.clone(),
            IPP_PRINTER_ATTRIBUTES_ACTION.clone(),
            IPP_JOB_ATTRIBUTES_ACTION.clone(),
        ])
});

/// Get IPP event types
pub fn get_ipp_event_types() -> Vec<EventType> {
    vec![IPP_REQUEST_EVENT.clone()]
}
