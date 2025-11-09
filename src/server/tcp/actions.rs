//! TCP protocol actions implementation

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

/// Connection data for TCP protocol
pub struct ConnectionData {
    pub write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
}

/// TCP protocol action handler
pub struct TcpProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

impl Default for TcpProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpProtocol {
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
        write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
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
impl Protocol for TcpProtocol {
        fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
            vec![
                crate::llm::actions::ParameterDefinition {
                    name: "send_first".to_string(),
                    type_hint: "boolean".to_string(),
                    description: "Whether the server should send the first message after connection (e.g., for FTP/SMTP greeting banners)".to_string(),
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
                send_tcp_data_action(),
                wait_for_more_action(),
                close_this_connection_action(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "TCP"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            get_tcp_event_types()
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["tcp", "raw", "ftp", "custom"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Beta)
                .implementation("Manual TCP socket handling with tokio")
                .llm_control("Full byte stream control - all sent/received data")
                .e2e_testing("tokio::net::TcpStream")
                .notes("Basis for FTP, SMTP, custom protocols")
                .build()
        }
        fn description(&self) -> &'static str {
            "Raw TCP socket server for custom protocols"
        }
        fn example_prompt(&self) -> &'static str {
            "Pretend to be FTP server on port 2121; serve file accounts.csv with 'balance,0'"
        }
        fn group_name(&self) -> &'static str {
            "Core"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for TcpProtocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                // Extract send_first from startup_params
                let send_first = ctx.startup_params
                    .as_ref()
                    .and_then(|p| p.get_optional_bool("send_first"))
                    .unwrap_or(false);
    
                use crate::server::tcp::TcpServer;
                TcpServer::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    send_first,
                    ctx.server_id,
                ).await
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
                    // because we need async context to send data
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
                    // The caller will need to handle actually sending it
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
                "send_tcp_data" => self.execute_send_tcp_data(action),
                "wait_for_more" => Ok(ActionResult::WaitForMore),
                "close_this_connection" => Ok(ActionResult::CloseConnection),
                _ => Err(anyhow::anyhow!("Unknown TCP action: {action_type}")),
            }
        }
}


impl TcpProtocol {
    /// Execute send_tcp_data sync action
    fn execute_send_tcp_data(&self, action: serde_json::Value) -> Result<ActionResult> {
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
        description: "Send data to a specific TCP connection (async action)".to_string(),
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
            "data": "Hello from TCP"
        }),
    }
}

/// Action definition for close_connection (async)
fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close a specific TCP connection (async action)".to_string(),
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
        description: "List all active TCP connections (async action)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_connections"
        }),
    }
}

/// Action definition for send_tcp_data (sync)
fn send_tcp_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_tcp_data".to_string(),
        description: "Send data over the current TCP connection".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_tcp_data",
            "data": "220 Welcome\r\n"
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

/// Action definition for close_this_connection (sync)
fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current TCP connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
    }
}

// ============================================================================
// TCP Action Constants
// ============================================================================

pub static SEND_TCP_DATA_ACTION: LazyLock<ActionDefinition> = LazyLock::new(send_tcp_data_action);
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(wait_for_more_action);
pub static CLOSE_THIS_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(close_this_connection_action);

// ============================================================================
// TCP Event Type Constants
// ============================================================================

/// TCP connection opened event - triggered when new connection is established
pub static TCP_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tcp_connection_opened",
        "New TCP connection established (send initial greeting/banner if needed)"
    )
    // No parameters - just connection opened notification
    .with_actions(vec![
        SEND_TCP_DATA_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
});

/// TCP data received event - triggered when data is received on connection
pub static TCP_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tcp_data_received",
        "Data received on TCP connection"
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
        SEND_TCP_DATA_ACTION.clone(),
        WAIT_FOR_MORE_ACTION.clone(),
        CLOSE_THIS_CONNECTION_ACTION.clone(),
    ])
});

/// Get TCP event types
pub fn get_tcp_event_types() -> Vec<EventType> {
    vec![
        TCP_CONNECTION_OPENED_EVENT.clone(),
        TCP_DATA_RECEIVED_EVENT.clone(),
    ]
}
