//! HTTP3 protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use quinn::SendStream;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;

/// Stream data for HTTP3 protocol
pub struct StreamData {
    pub send_stream: Arc<Mutex<SendStream>>,
}

/// HTTP3 protocol action handler
pub struct Http3Protocol {
    /// Map of active streams (for async actions)
    streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>,
}

impl Default for Http3Protocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Http3Protocol {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_streams(streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>) -> Self {
        Self { streams }
    }

    /// Add a stream to the protocol handler
    pub async fn add_stream(&self, stream_id: ConnectionId, send_stream: Arc<Mutex<SendStream>>) {
        self.streams
            .lock()
            .await
            .insert(stream_id, StreamData { send_stream });
    }

    /// Remove a stream from the protocol handler
    pub async fn remove_stream(&self, stream_id: &ConnectionId) {
        self.streams.lock().await.remove(stream_id);
    }

    /// Get list of active stream IDs
    pub async fn list_stream_ids(&self) -> Vec<ConnectionId> {
        self.streams.lock().await.keys().copied().collect()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Http3Protocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        crate::server::tls_cert_manager::get_tls_startup_parameters()
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            send_to_stream_action(),
            close_stream_action(),
            list_streams_action(),
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_http3_data_action(),
            wait_for_more_action(),
            close_this_stream_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "HTTP3"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_http3_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>HTTP3"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["http3"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Quinn async HTTP3 with built-in TLS 1.3")
            .llm_control("Full stream control - all sent/received data on bidirectional streams")
            .e2e_testing("quinn::Endpoint client")
            .notes("UDP-based, encrypted, multiplexed transport - basis for HTTP/3")
            .build()
    }
    fn description(&self) -> &'static str {
        "HTTP3 protocol server with multiplexed streams"
    }
    fn example_prompt(&self) -> &'static str {
        "HTTP3 echo server on port 4433; echo back all data received on each stream"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for Http3Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::http3::Http3Server;

            // Parse TLS configuration from startup_params (HTTP/3 always uses TLS)
            let tls_config = if let Some(ref params) = ctx.startup_params {
                // For HTTP/3, extract TLS config or use default
                match crate::server::tls_cert_manager::extract_tls_config_from_params(params) {
                    Ok(Some(config)) => Some(config),
                    Ok(None) => {
                        // If tls_enabled is false or not set, use default for HTTP/3
                        match crate::server::tls_cert_manager::generate_default_tls_config() {
                            Ok(config) => Some(config),
                            Err(e) => {
                                return Err(anyhow::anyhow!(
                                    "Failed to generate default TLS config: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to create TLS config: {}", e));
                    }
                }
            } else {
                // Use default self-signed certificate
                match crate::server::tls_cert_manager::generate_default_tls_config() {
                    Ok(config) => Some(config),
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "Failed to generate default TLS config: {}",
                            e
                        ));
                    }
                }
            };

            Http3Server::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
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
            "send_to_stream" => {
                // Async action - not fully implemented here, needs to be handled by caller
                // because we need async context to send data
                let stream_id_str = action
                    .get("stream_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'stream_id' parameter")?;

                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data' parameter")?;

                let _stream_id =
                    ConnectionId::from_string(stream_id_str).context("Invalid stream_id format")?;

                // Return the data with stream ID embedded
                // The caller will need to handle actually sending it
                Ok(ActionResult::Output(data.as_bytes().to_vec()))
            }
            "close_stream" => {
                // Async action - signal that stream should be closed
                let stream_id_str = action
                    .get("stream_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'stream_id' parameter")?;

                let _stream_id =
                    ConnectionId::from_string(stream_id_str).context("Invalid stream_id format")?;

                Ok(ActionResult::CloseConnection)
            }
            "list_streams" => {
                // This needs to be handled specially by the caller
                Ok(ActionResult::NoAction)
            }
            "send_http3_data" => self.execute_send_http3_data(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_this_stream" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown HTTP3 action: {action_type}")),
        }
    }
}

impl Http3Protocol {
    /// Execute send_http3_data sync action
    fn execute_send_http3_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }
}

/// Action definition for send_to_stream (async)
fn send_to_stream_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_stream".to_string(),
        description: "Send data to a specific HTTP3 stream (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "stream_id".to_string(),
                type_hint: "string".to_string(),
                description: "Stream ID to send to".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Data to send".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_to_stream",
            "stream_id": "conn_12345",
            "data": "Hello from HTTP3"
        }),
    }
}

/// Action definition for close_stream (async)
fn close_stream_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_stream".to_string(),
        description: "Close a specific HTTP3 stream (async action)".to_string(),
        parameters: vec![Parameter {
            name: "stream_id".to_string(),
            type_hint: "string".to_string(),
            description: "Stream ID to close".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_stream",
            "stream_id": "conn_12345"
        }),
    }
}

/// Action definition for list_streams (async)
fn list_streams_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_streams".to_string(),
        description: "List all active HTTP3 streams (async action)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_streams"
        }),
    }
}

/// Action definition for send_http3_data (sync)
fn send_http3_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_http3_data".to_string(),
        description: "Send data over the current HTTP3 stream".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_http3_data",
            "data": "Hello from HTTP3\n"
        }),
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
    }
}

/// Action definition for close_this_stream (sync)
fn close_this_stream_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_stream".to_string(),
        description: "Close the current HTTP3 stream".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_stream"
        }),
    }
}

// ============================================================================
// HTTP3 Action Constants
// ============================================================================

pub static SEND_HTTP3_DATA_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(send_http3_data_action);
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(wait_for_more_action);
pub static CLOSE_THIS_STREAM_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(close_this_stream_action);

// ============================================================================
// HTTP3 Event Type Constants
// ============================================================================

/// HTTP3 connection opened event - triggered when new connection is established
pub static HTTP3_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "http3_connection_opened",
        "New HTTP3 connection established with TLS 1.3 encryption",
    )
    // No parameters - just connection opened notification
    .with_actions(vec![])
});

/// HTTP3 stream opened event - triggered when client opens a new stream
pub static HTTP3_STREAM_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "http3_stream_opened",
        "New bidirectional stream opened by client",
    )
    .with_parameters(vec![Parameter {
        name: "stream_id".to_string(),
        type_hint: "string".to_string(),
        description: "The stream ID (HTTP3 uses per-connection stream numbering)".to_string(),
        required: true,
    }])
    .with_actions(vec![
        SEND_HTTP3_DATA_ACTION.clone(),
        CLOSE_THIS_STREAM_ACTION.clone(),
    ])
});

/// HTTP3 data received event - triggered when data is received on a stream
pub static HTTP3_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("http3_data_received", "Data received on HTTP3 stream")
        .with_parameters(vec![
            Parameter {
                name: "stream_id".to_string(),
                type_hint: "string".to_string(),
                description: "The stream ID this data was received on".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "The data received (as hex string or UTF-8 if printable)".to_string(),
                required: true,
            },
        ])
        .with_actions(vec![
            SEND_HTTP3_DATA_ACTION.clone(),
            WAIT_FOR_MORE_ACTION.clone(),
            CLOSE_THIS_STREAM_ACTION.clone(),
        ])
});

/// Get HTTP3 event types
pub fn get_http3_event_types() -> Vec<EventType> {
    vec![
        HTTP3_CONNECTION_OPENED_EVENT.clone(),
        HTTP3_STREAM_OPENED_EVENT.clone(),
        HTTP3_DATA_RECEIVED_EVENT.clone(),
    ]
}
