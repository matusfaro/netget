//! Tor Relay protocol actions implementation

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

/// Tor Relay protocol action handler
pub struct TorRelayProtocol;

impl TorRelayProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_detect_create_cell(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response_type = action
            .get("response_type")
            .and_then(|v| v.as_str())
            .unwrap_or("reject");

        debug!("Tor Relay detected CREATE cell, response: {}", response_type);

        match response_type {
            "accept" => {
                // Send fake CREATED cell (honeypot mode)
                let created_cell = vec![0u8; 64]; // Simplified CREATED cell
                Ok(ActionResult::Output(created_cell))
            }
            "reject" => {
                // Send DESTROY cell
                let destroy_cell = vec![4u8; 5]; // Simplified DESTROY cell
                Ok(ActionResult::Output(destroy_cell))
            }
            _ => Ok(ActionResult::CloseConnection),
        }
    }

    fn execute_detect_relay_cell(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        debug!("Tor Relay: {}", message);

        Ok(ActionResult::Custom {
            name: "tor_relay_log".to_string(),
            data: json!({
                "logged": true,
                "message": message
            })
        })
    }

    fn execute_send_destroy(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("Tor Relay sending DESTROY cell");

        // Simplified DESTROY cell (command 4)
        let destroy_cell = vec![4u8; 5];
        Ok(ActionResult::Output(destroy_cell))
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TorRelayProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                set_relay_type_action(),
                configure_exit_policy_action(),
                list_active_circuits_action(),
                disconnect_circuit_action(),
                list_active_streams_action(),
                close_stream_action(),
                get_relay_statistics_action(),
            ]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                detect_create_cell_action(),
                detect_relay_cell_action(),
                send_destroy_action(),
                close_connection_action(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "Tor Relay"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            get_tor_relay_event_types()
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>TLS>TorRelay"
        }
        fn description(&self) -> &'static str {
            "Tor relay server for anonymous communication"
        }
        fn example_prompt(&self) -> &'static str {
            "Start a Tor exit relay on port 9001 allowing connections to localhost"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["tor_relay", "tor-relay", "onion router", "guard", "exit", "middle", "circuit"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Stable)
                .implementation("Custom Tor OR protocol with ntor handshake - 2,182 LOC")
                .llm_control("Circuit creation logging + unknown relay command responses")
                .e2e_testing("Official Tor client (tor binary)")
                .notes("Full exit relay, cryptographically correct, production-ready")
                .build()
        }
        fn group_name(&self) -> &'static str {
            "Network Services"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for TorRelayProtocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::server::tor_relay::TorRelayServer;
                TorRelayServer::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
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
                "detect_create_cell" => self.execute_detect_create_cell(action),
                "detect_relay_cell" => self.execute_detect_relay_cell(action),
                "send_destroy" => self.execute_send_destroy(action),
                "close_connection" => Ok(ActionResult::CloseConnection),
                // Async actions return custom results
                "set_relay_type" | "configure_exit_policy"
                | "list_active_circuits" | "disconnect_circuit"
                | "list_active_streams" | "close_stream" | "get_relay_statistics" => {
                    Ok(ActionResult::Custom {
                        name: "tor_relay_async".to_string(),
                        data: json!({
                            "action": action_type,
                            "note": "Async action - implementation in server logic"
                        })
                    })
                },
                _ => Err(anyhow::anyhow!("Unknown Tor Relay action: {}", action_type)),
            }
        }
}


// ============================================================================
// Action Definitions - Sync Actions (Network Event Triggered)
// ============================================================================

fn detect_create_cell_action() -> ActionDefinition {
    ActionDefinition {
        name: "detect_create_cell".to_string(),
        description: "Detected Tor CREATE cell (circuit creation request)".to_string(),
        parameters: vec![
            Parameter {
                name: "response_type".to_string(),
                type_hint: "string".to_string(),
                description: "How to respond: 'accept' (honeypot), 'reject' (DESTROY), 'silent' (close)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "detect_create_cell",
            "response_type": "reject"
        }),
    }
}

fn detect_relay_cell_action() -> ActionDefinition {
    ActionDefinition {
        name: "detect_relay_cell".to_string(),
        description: "Detected Tor RELAY cell".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Log message describing the RELAY cell".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "detect_relay_cell",
            "message": "RELAY cell detected from circuit 0x12345"
        }),
    }
}

fn send_destroy_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_destroy".to_string(),
        description: "Send DESTROY cell to tear down circuit".to_string(),
        parameters: vec![],
        example: json!({
            "type": "send_destroy"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the connection immediately".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}

// ============================================================================
// Action Definitions - Async Actions (User Triggered)
// ============================================================================

fn set_relay_type_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_relay_type".to_string(),
        description: "Set relay type (Guard/Middle/Exit) for future implementation".to_string(),
        parameters: vec![
            Parameter {
                name: "relay_type".to_string(),
                type_hint: "string".to_string(),
                description: "Relay type: 'guard', 'middle', or 'exit'".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "set_relay_type",
            "relay_type": "guard"
        }),
    }
}

fn configure_exit_policy_action() -> ActionDefinition {
    ActionDefinition {
        name: "configure_exit_policy".to_string(),
        description: "Configure exit policy (allowed destinations/ports)".to_string(),
        parameters: vec![
            Parameter {
                name: "allowed_ports".to_string(),
                type_hint: "array".to_string(),
                description: "List of allowed ports (e.g., [80, 443])".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "configure_exit_policy",
            "allowed_ports": [80, 443, 22]
        }),
    }
}

fn list_active_circuits_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_active_circuits".to_string(),
        description: "List all active circuits".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_active_circuits"
        }),
    }
}

fn disconnect_circuit_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect_circuit".to_string(),
        description: "Disconnect a specific circuit by ID".to_string(),
        parameters: vec![
            Parameter {
                name: "circuit_id".to_string(),
                type_hint: "string".to_string(),
                description: "Circuit ID to disconnect".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "disconnect_circuit",
            "circuit_id": "0x12345678"
        }),
    }
}

fn list_active_streams_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_active_streams".to_string(),
        description: "List all active streams across all circuits".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_active_streams"
        }),
    }
}

fn close_stream_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_stream".to_string(),
        description: "Close a specific stream by circuit ID and stream ID".to_string(),
        parameters: vec![
            Parameter {
                name: "circuit_id".to_string(),
                type_hint: "string".to_string(),
                description: "Circuit ID (hex format)".to_string(),
                required: true,
            },
            Parameter {
                name: "stream_id".to_string(),
                type_hint: "number".to_string(),
                description: "Stream ID within the circuit".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "close_stream",
            "circuit_id": "0x12345678",
            "stream_id": 42
        }),
    }
}

fn get_relay_statistics_action() -> ActionDefinition {
    ActionDefinition {
        name: "get_relay_statistics".to_string(),
        description: "Get relay statistics (circuits, streams, bandwidth)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "get_relay_statistics"
        }),
    }
}

// ============================================================================
// Action Constants
// ============================================================================

pub static DETECT_CREATE_CELL_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| detect_create_cell_action());
pub static DETECT_RELAY_CELL_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| detect_relay_cell_action());
pub static SEND_DESTROY_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_destroy_action());
pub static CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| close_connection_action());

// ============================================================================
// Event Type Constants
// ============================================================================

/// Tor Relay cell detection event - triggered when cells are detected
pub static TOR_RELAY_CELL_DETECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_relay_cell_detected",
        "Tor OR protocol cell detected from client"
    )
    .with_parameters(vec![
        Parameter {
            name: "cell_type".to_string(),
            type_hint: "string".to_string(),
            description: "The type of cell detected (CREATE, RELAY, DESTROY, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "circuit_id".to_string(),
            type_hint: "string".to_string(),
            description: "Circuit ID from the cell (hex format)".to_string(),
            required: true,
        },
        Parameter {
            name: "client_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Client IP address".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        DETECT_CREATE_CELL_ACTION.clone(),
        DETECT_RELAY_CELL_ACTION.clone(),
        SEND_DESTROY_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
});

/// Tor Relay circuit created event - triggered when CREATE2 succeeds
pub static TOR_RELAY_CIRCUIT_CREATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_relay_circuit_created",
        "Tor circuit successfully created via ntor handshake"
    )
    .with_parameters(vec![
        Parameter {
            name: "circuit_id".to_string(),
            type_hint: "string".to_string(),
            description: "Circuit ID (hex format)".to_string(),
            required: true,
        },
        Parameter {
            name: "client_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Client IP address".to_string(),
            required: true,
        },
    ])
});

/// Tor Relay RELAY cell event - triggered when RELAY cells are processed
pub static TOR_RELAY_RELAY_CELL_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_relay_relay_cell",
        "Tor RELAY cell received and decrypted"
    )
    .with_parameters(vec![
        Parameter {
            name: "circuit_id".to_string(),
            type_hint: "string".to_string(),
            description: "Circuit ID (hex format)".to_string(),
            required: true,
        },
        Parameter {
            name: "relay_command".to_string(),
            type_hint: "string".to_string(),
            description: "RELAY command type (BEGIN, DATA, END, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "stream_id".to_string(),
            type_hint: "number".to_string(),
            description: "Stream ID within the circuit".to_string(),
            required: true,
        },
        Parameter {
            name: "length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of RELAY cell data".to_string(),
            required: true,
        },
        Parameter {
            name: "client_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Client IP address".to_string(),
            required: true,
        },
    ])
});

pub fn get_tor_relay_event_types() -> Vec<EventType> {
    vec![
        TOR_RELAY_CELL_DETECTED_EVENT.clone(),
        TOR_RELAY_CIRCUIT_CREATED_EVENT.clone(),
        TOR_RELAY_RELAY_CELL_EVENT.clone(),
    ]
}
