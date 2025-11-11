//! PyPI protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::LazyLock;

/// PyPI protocol action handler
pub struct PypiProtocol;

impl PypiProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for PypiProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // PyPI has no async actions - it's purely request-response like HTTP
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_pypi_response_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "PyPI"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_pypi_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>PyPI"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "pypi",
            "python repository",
            "python package index",
            "pip server",
            "via pypi",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("PEP 503 Simple Repository API on hyper HTTP server")
            .llm_control("Package availability, version lists, and file serving")
            .e2e_testing("pip install command - target < 10 LLM calls")
            .build()
    }
    fn description(&self) -> &'static str {
        "Python Package Index (PyPI) repository server implementing PEP 503"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as a PyPI server on port 8080. Serve a package called 'hello-world' with version 1.0.0 containing a simple wheel file with setup metadata."
    }
    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for PypiProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::pypi::PypiServer;
            PypiServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
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
            "send_pypi_response" => self.execute_send_pypi_response(action),
            _ => Err(anyhow::anyhow!("Unknown PyPI action: {action_type}")),
        }
    }
}

impl PypiProtocol {
    /// Execute send_pypi_response sync action
    fn execute_send_pypi_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action
            .get("status")
            .and_then(|v| v.as_u64())
            .context("Missing or invalid 'status' parameter")? as u16;

        let headers = action
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .context("Missing 'body' parameter")?;

        // Return structured data for PyPI response
        let response_data = json!({
            "status": status,
            "headers": headers,
            "body": body
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize PyPI response")?,
        ))
    }
}

/// Action definition for send_pypi_response (sync)
fn send_pypi_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_pypi_response".to_string(),
        description: "Send a PyPI HTTP response to the current request".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (e.g., 200, 404, 500)".to_string(),
                required: true,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Response headers as key-value pairs (must include Content-Type for HTML: text/html)".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Response body (HTML for /simple/ endpoints, binary data for package files)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_pypi_response",
            "status": 200,
            "headers": {
                "Content-Type": "text/html"
            },
            "body": "<!DOCTYPE html>\n<html>\n<body>\n<a href=\"hello-world/\">hello-world</a>\n</body>\n</html>"
        }),
    }
}

// ============================================================================
// PyPI Action Constants
// ============================================================================

pub static SEND_PYPI_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_pypi_response_action());

// ============================================================================
// PyPI Event Type Constants
// ============================================================================

/// PyPI request event - triggered when client sends a PyPI HTTP request
pub static PYPI_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "pypi_request",
        "PyPI HTTP request received from client (pip, twine, etc.)"
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (usually GET for pip)".to_string(),
            required: true,
        },
        Parameter {
            name: "uri".to_string(),
            type_hint: "string".to_string(),
            description: "Request URI".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path (e.g., /simple/, /simple/package-name/, /packages/...)".to_string(),
            required: true,
        },
        Parameter {
            name: "headers".to_string(),
            type_hint: "object".to_string(),
            description: "Request headers as key-value pairs".to_string(),
            required: true,
        },
        Parameter {
            name: "body".to_string(),
            type_hint: "string".to_string(),
            description: "Request body (usually empty for GET requests)".to_string(),
            required: false,
        },
        Parameter {
            name: "request_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of PyPI request: 'list_packages' (/simple/), 'list_files' (/simple/package/), 'download_file' (/packages/...), or 'unknown'".to_string(),
            required: true,
        },
        Parameter {
            name: "package_name".to_string(),
            type_hint: "string".to_string(),
            description: "Package name if request_type is 'list_files' or 'download_file'".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        SEND_PYPI_RESPONSE_ACTION.clone(),
    ])
});

/// Get PyPI event types
pub fn get_pypi_event_types() -> Vec<EventType> {
    vec![PYPI_REQUEST_EVENT.clone()]
}
