//! TLS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// TLS protocol action handler
pub struct TlsProtocol {}

impl TlsProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TlsProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
                crate::llm::actions::ParameterDefinition {
                    name: "send_first".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Whether the server should send the first message after TLS handshake (e.g., for greeting banners)".to_string(),
                    required: false,
                    example: serde_json::json!(false),
                },
                crate::llm::actions::ParameterDefinition {
                    name: "cert_path".to_string(),
                    type_hint: "string".to_string(),
                    description: "Path to TLS certificate file (PEM format). If not provided, a self-signed certificate will be generated.".to_string(),
                    required: false,
                    example: serde_json::json!("/path/to/cert.pem"),
                },
                crate::llm::actions::ParameterDefinition {
                    name: "key_path".to_string(),
                    type_hint: "string".to_string(),
                    description: "Path to TLS private key file (PEM format). Required if cert_path is provided.".to_string(),
                    required: false,
                    example: serde_json::json!("/path/to/key.pem"),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_tls_data_action(),
            wait_for_more_action(),
            close_this_connection_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "TLS"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_tls_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>TLS"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["tls", "ssl", "secure", "encrypted"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("TLS transport layer using tokio-rustls with self-signed certificates")
            .llm_control("Full control over application protocol on top of TLS")
            .e2e_testing("Native TLS client for testing")
            .notes("Generic TLS server for custom protocols - LLM implements application layer")
            .build()
    }
    fn description(&self) -> &'static str {
        "Generic TLS server for implementing custom encrypted protocols"
    }
    fn example_prompt(&self) -> &'static str {
        "Listen on port 8443 via TLS; implement a simple chat protocol over encrypted connection"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            json!({
                "type": "open_server",
                "port": 8443,
                "base_stack": "tls",
                "instruction": "Secure TLS server for custom encrypted protocols"
            }),
            json!({
                "type": "open_server",
                "port": 8443,
                "base_stack": "tls",
                "event_handlers": [{
                    "event_pattern": "tls_data_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<tls_handler>"
                    }
                }]
            }),
            json!({
                "type": "open_server",
                "port": 8443,
                "base_stack": "tls",
                "event_handlers": [
                    {
                        "event_pattern": "tls_connection_opened",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_tls_data",
                                "data": "220 Welcome to secure server\r\n"
                            }]
                        }
                    },
                    {
                        "event_pattern": "tls_data_received",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_tls_data",
                                "data": "OK\r\n"
                            }]
                        }
                    }
                ]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for TlsProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            // Extract send_first from startup_params
            let send_first = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            // Extract custom TLS config if provided
            let tls_config = if let Some(ref params) = ctx.startup_params {
                let cert_path = params.get_optional_string("cert_path");
                let key_path = params.get_optional_string("key_path");

                match (cert_path, key_path) {
                    (Some(cert), Some(key)) => {
                        // Load custom certificates from files
                        Some(crate::server::tls_cert_manager::load_tls_config_from_files(
                            &cert,
                            &key,
                        )?)
                    }
                    (Some(_), None) | (None, Some(_)) => {
                        return Err(anyhow::anyhow!(
                            "Both cert_path and key_path must be provided together"
                        ));
                    }
                    (None, None) => None, // Use default self-signed certificate
                }
            } else {
                None
            };

            use crate::server::tls::TlsServer;
            TlsServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
                tls_config,
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
            "send_tls_data" => self.execute_send_tls_data(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown TLS action: {action_type}")),
        }
    }
}

impl TlsProtocol {
    /// Execute send_tls_data sync action
    fn execute_send_tls_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }
}

/// Action definition for send_tls_data (sync)
fn send_tls_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_tls_data".to_string(),
        description: "Send data over the current TLS connection".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send (text or hex for binary)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_tls_data",
            "data": "Hello over TLS\r\n"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> TLS {data_len}B")
                .with_debug("TLS send: {data_len} bytes"),
        ),
    }
}

/// Action definition for wait_for_more (sync)
fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding (accumulate incomplete protocol data)"
            .to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
        log_template: Some(LogTemplate::new().with_debug("TLS waiting for more data")),
    }
}

/// Action definition for close_this_connection (sync)
fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current TLS connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
        log_template: Some(LogTemplate::new().with_info("-> TLS connection closed")),
    }
}

// ============================================================================
// TLS Action Constants
// ============================================================================

pub static SEND_TLS_DATA_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_tls_data_action());
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| wait_for_more_action());
pub static CLOSE_THIS_CONNECTION_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| close_this_connection_action());

// ============================================================================
// TLS Event Type Constants
// ============================================================================

/// TLS connection opened event - triggered when TLS handshake completes
pub static TLS_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tls_connection_opened",
        "TLS handshake complete, connection established (send initial greeting/banner if needed)",
        json!({
            "type": "send_tls_data",
            "data": "220 Welcome to secure server\r\n"
        }),
    )
    // No parameters - just connection opened notification
    .with_actions(vec![
        SEND_TLS_DATA_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} TLS connected")
            .with_debug("TLS handshake complete from {client_ip}"),
    )
});

/// TLS data received event - triggered when data is received on encrypted connection
pub static TLS_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tls_data_received",
        "Data received on TLS connection (implement your application protocol here)",
        json!({
            "type": "send_tls_data",
            "data": "OK Data received\r\n"
        }),
    )
    .with_parameters(vec![Parameter {
        name: "data".to_string(),
        type_hint: "string".to_string(),
        description: "The data received (as hex string if binary, UTF-8 if printable)".to_string(),
        required: true,
    }])
    .with_actions(vec![
        SEND_TLS_DATA_ACTION.clone(),
        WAIT_FOR_MORE_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} TLS <- {data_len}B ({duration_ms}ms)")
            .with_debug("TLS data from {client_ip}: {data_len} bytes")
            .with_trace("TLS data: {data}"),
    )
});

/// Get TLS event types
pub fn get_tls_event_types() -> Vec<EventType> {
    vec![
        TLS_CONNECTION_OPENED_EVENT.clone(),
        TLS_DATA_RECEIVED_EVENT.clone(),
    ]
}
