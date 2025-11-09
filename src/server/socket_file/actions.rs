//! Socket file protocol actions implementation
//!
//! Platform: Unix/Linux only (uses Unix domain sockets)
#![cfg(unix)]

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::server::connection::ConnectionId;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;

/// Connection data for socket file protocol
pub struct ConnectionData {
    pub write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
}

/// Socket file protocol action handler
pub struct SocketFileProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

impl SocketFileProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_connections(
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) -> Self {
        Self { connections }
    }

    /// Add a connection to the protocol handler
    pub async fn add_connection(
        &self,
        connection_id: ConnectionId,
        write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>,
    ) {
        self.connections
            .lock()
            .await
            .insert(connection_id, ConnectionData { write_half });
    }

    /// Remove a connection from the protocol handler
    pub async fn remove_connection(&self, connection_id: &ConnectionId) {
        self.connections.lock().await.remove(connection_id);
    }

    /// Get list of active connection IDs
    pub async fn list_connection_ids(&self) -> Vec<ConnectionId> {
        self.connections.lock().await.keys().copied().collect()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SocketFileProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "socket_path".to_string(),
                type_hint: "string".to_string(),
                description: "Filesystem path for the Unix domain socket file (e.g., ./netget.sock)".to_string(),
                required: true,
                example: serde_json::json!("./netget.sock"),
            },
            crate::llm::actions::ParameterDefinition {
                name: "send_first".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether the server should send the first message after connection (e.g., for greeting banners)".to_string(),
                required: false,
                example: serde_json::json!(false),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            send_to_connection_action(),
            close_connection_action(),
            list_connections_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_socket_data_action(),
            wait_for_more_action(),
            close_this_connection_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "SOCKET_FILE"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_socket_file_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "UNIX_SOCKET"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["socket_file", "unix_socket", "ipc"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual Unix domain socket handling with tokio")
            .llm_control("Full byte stream control - all sent/received data")
            .e2e_testing("tokio::net::UnixStream")
            .notes("Unix domain socket for IPC - uses filesystem socket files instead of IP:port")
            .build()
    }

    fn description(&self) -> &'static str {
        "Unix domain socket server for inter-process communication"
    }

    fn example_prompt(&self) -> &'static str {
        "Create socket file at ./myapp.sock and echo back any data received"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SocketFileProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            // Extract socket_path and send_first from startup_params
            let socket_path = ctx.startup_params
                .as_ref()
                .and_then(|p| Some(p.get_string("socket_path")))
                .ok_or_else(|| anyhow::anyhow!("socket_path parameter is required"))?;

            let send_first = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            use crate::server::socket_file::SocketFileServer;
            let socket_path_buf = std::path::PathBuf::from(socket_path);
            let result_path = SocketFileServer::spawn_with_llm_actions(
                socket_path_buf,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
            ).await?;

            // Return a dummy SocketAddr since Unix sockets don't have IP addresses
            // Store the actual socket path in the server instance
            Ok("127.0.0.1:0".parse().unwrap())
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_to_connection" => {
                // Async action - not fully implemented here, needs to be handled by caller
                let connection_id_str = action
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'connection_id' parameter")?;

                let data = action
                    .get("data")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data' parameter")?;

                let _connection_id = ConnectionId::from_string(connection_id_str)
                    .context("Invalid connection_id format")?;

                // Return the data with connection ID embedded
                Ok(ActionResult::Output(data.as_bytes().to_vec()))
            }
            "close_connection" => {
                // Async action - signal that connection should be closed
                let connection_id_str = action
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'connection_id' parameter")?;

                let _connection_id = ConnectionId::from_string(connection_id_str)
                    .context("Invalid connection_id format")?;

                Ok(ActionResult::CloseConnection)
            }
            "list_connections" => {
                // This needs to be handled specially by the caller
                Ok(ActionResult::NoAction)
            }
            "send_socket_data" => self.execute_send_socket_data(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown socket file action: {action_type}")),
        }
    }
}

impl SocketFileProtocol {
    /// Execute send_socket_data sync action
    fn execute_send_socket_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }
}

/// Action definition for send_to_connection (async)
fn send_to_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_connection".to_string(),
        description: "Send data to a specific socket file connection (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "connection_id".to_string(),
                type_hint: "string".to_string(),
                description: "Connection ID to send to".to_string(),
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
            "type": "send_to_connection",
            "connection_id": "conn_12345",
            "data": "Hello from socket file"
        }),
    }
}

/// Action definition for close_connection (async)
fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close a specific socket file connection (async action)".to_string(),
        parameters: vec![Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID to close".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_connection",
            "connection_id": "conn_12345"
        }),
    }
}

/// Action definition for list_connections (async)
fn list_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_connections".to_string(),
        description: "List all active socket file connections (async action)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_connections"
        }),
    }
}

/// Action definition for send_socket_data (sync)
fn send_socket_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_socket_data".to_string(),
        description: "Send data over the current socket file connection".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_socket_data",
            "data": "ACK\n"
        }),
    }
}

/// Action definition for wait_for_more (sync)
fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding (accumulate incomplete data)"
            .to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}

/// Action definition for close_this_connection (sync)
fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current socket file connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
    }
}

// ============================================================================
// Socket File Action Constants
// ============================================================================

pub static SEND_SOCKET_DATA_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_socket_data_action());
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| wait_for_more_action());
pub static CLOSE_THIS_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| close_this_connection_action());

// ============================================================================
// Socket File Event Type Constants
// ============================================================================

/// Socket file connection opened event - triggered when new connection is established
pub static SOCKET_FILE_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socket_file_connection_opened",
        "New Unix domain socket connection established (send initial greeting/banner if needed)"
    )
    .with_parameters(vec![])
    .with_actions(vec![
        SEND_SOCKET_DATA_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
});

/// Socket file data received event - triggered when data is received on connection
pub static SOCKET_FILE_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socket_file_data_received",
        "Data received on Unix domain socket connection"
    )
    .with_parameters(vec![
        Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "The data received (as hex string or UTF-8 if printable)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        SEND_SOCKET_DATA_ACTION.clone(),
        WAIT_FOR_MORE_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
});

/// Get socket file event types
pub fn get_socket_file_event_types() -> Vec<EventType> {
    vec![
        SOCKET_FILE_CONNECTION_OPENED_EVENT.clone(),
        SOCKET_FILE_DATA_RECEIVED_EVENT.clone(),
    ]
}
