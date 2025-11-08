//! SOCKS5 protocol actions implementation

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

/// SOCKS5 protocol action handler
pub struct Socks5Protocol {
}

impl Socks5Protocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Socks5Protocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            // Future: Could add actions like:
            // - close_socks5_connection(connection_id)
            // - set_socks5_filter(patterns)
            // - list_socks5_connections()
            Vec::new()
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                allow_socks5_connect_action(),
                deny_socks5_connect_action(),
                allow_socks5_auth_action(),
                deny_socks5_auth_action(),
                forward_socks5_data_action(),
                modify_socks5_data_action(),
                close_connection_action(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "SOCKS5"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            get_socks5_event_types()
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>SOCKS5"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["socks"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("Manual SOCKS5 protocol (RFC 1928)")
                .llm_control("Auth, connection allow/deny, MITM data inspection")
                .e2e_testing("curl --socks5 / SOCKS5 clients")
                .notes("CONNECT only (no BIND/UDP ASSOCIATE)")
                .build()
        }
        fn description(&self) -> &'static str {
            "SOCKS5 proxy server"
        }
        fn example_prompt(&self) -> &'static str {
            "Start a SOCKS5 proxy on port 1080 that asks before connecting"
        }
        fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
            use crate::llm::actions::ParameterDefinition;
            vec![
                ParameterDefinition {
                    name: "auth_methods".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of allowed authentication methods: 'none' (no auth) or 'username_password' (RFC 1929)".to_string(),
                    required: false,
                    example: json!(["none", "username_password"]),
                },
                ParameterDefinition {
                    name: "default_action".to_string(),
                    type_hint: "string".to_string(),
                    description: "Default action when no filter matches: 'allow' or 'deny'".to_string(),
                    required: false,
                    example: json!("allow"),
                },
                ParameterDefinition {
                    name: "filter_mode".to_string(),
                    type_hint: "string".to_string(),
                    description: "Filter mode: 'allow_all', 'deny_all', 'ask_llm', or 'selective'".to_string(),
                    required: false,
                    example: json!("selective"),
                },
                ParameterDefinition {
                    name: "filter".to_string(),
                    type_hint: "object".to_string(),
                    description: "Filter configuration object with 'target_host_patterns' (array of regex) and 'target_port_ranges' (array of [start, end])".to_string(),
                    required: false,
                    example: json!({
                        "target_host_patterns": [".*\\.example\\.com"],
                        "target_port_ranges": [[80, 80], [443, 443]]
                    }),
                },
            ]
        }
        fn group_name(&self) -> &'static str {
            "Proxy & Network"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for Socks5Protocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::server::socks5::Socks5Server;
                Socks5Server::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.server_id,
                    ctx.startup_params,
                ).await
            })
        }
        fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
            let action_type = action
                .get("type")
                .and_then(|v| v.as_str())
                .context("Missing 'type' field in action")?;
    
            match action_type {
                "allow_socks5_connect" => self.execute_allow_connect(action),
                "deny_socks5_connect" => self.execute_deny_connect(action),
                "allow_socks5_auth" => self.execute_allow_auth(action),
                "deny_socks5_auth" => self.execute_deny_auth(action),
                "forward_socks5_data" => self.execute_forward_data(action),
                "modify_socks5_data" => self.execute_modify_data(action),
                "close_connection" => self.execute_close_connection(action),
                _ => Err(anyhow::anyhow!("Unknown SOCKS5 action: {}", action_type)),
            }
        }
}


impl Socks5Protocol {
    fn execute_allow_connect(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _mitm = action
            .get("mitm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        debug!("SOCKS5 allowing connection (MITM: {})", _mitm);

        // Return NoAction to signal connection should proceed
        Ok(ActionResult::NoAction)
    }

    fn execute_deny_connect(&self, action: serde_json::Value) -> Result<ActionResult> {
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Connection denied by policy");

        debug!("SOCKS5 denying connection: {}", reason);

        // Return CloseConnection to deny
        Ok(ActionResult::CloseConnection)
    }

    fn execute_allow_auth(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("SOCKS5 allowing authentication");
        Ok(ActionResult::NoAction)
    }

    fn execute_deny_auth(&self, action: serde_json::Value) -> Result<ActionResult> {
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Authentication failed");

        debug!("SOCKS5 denying authentication: {}", reason);
        Ok(ActionResult::CloseConnection)
    }

    fn execute_forward_data(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("SOCKS5 forwarding data as-is");
        // Return NoAction to signal data should be forwarded unchanged
        Ok(ActionResult::NoAction)
    }

    fn execute_modify_data(&self, action: serde_json::Value) -> Result<ActionResult> {
        let modified_data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' field for modify_socks5_data action")?;

        debug!("SOCKS5 modifying data (new length: {} bytes)", modified_data.len());

        // Return Output with the modified data
        Ok(ActionResult::Output(modified_data.as_bytes().to_vec()))
    }

    fn execute_close_connection(&self, action: serde_json::Value) -> Result<ActionResult> {
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Connection closed by policy");

        debug!("SOCKS5 closing connection: {}", reason);
        Ok(ActionResult::CloseConnection)
    }
}

// ============================================================================
// SOCKS5 Action Definitions
// ============================================================================

fn allow_socks5_connect_action() -> ActionDefinition {
    ActionDefinition {
        name: "allow_socks5_connect".to_string(),
        description: "Allow SOCKS5 CONNECT request to proceed".to_string(),
        parameters: vec![
            Parameter {
                name: "mitm".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable MITM inspection for this connection (default: false)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "allow_socks5_connect",
            "mitm": false
        }),
    }
}

fn deny_socks5_connect_action() -> ActionDefinition {
    ActionDefinition {
        name: "deny_socks5_connect".to_string(),
        description: "Deny SOCKS5 CONNECT request".to_string(),
        parameters: vec![
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for denial (for logging)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "deny_socks5_connect",
            "reason": "Blocked by security policy"
        }),
    }
}

fn allow_socks5_auth_action() -> ActionDefinition {
    ActionDefinition {
        name: "allow_socks5_auth".to_string(),
        description: "Allow SOCKS5 username/password authentication".to_string(),
        parameters: vec![],
        example: json!({
            "type": "allow_socks5_auth"
        }),
    }
}

fn deny_socks5_auth_action() -> ActionDefinition {
    ActionDefinition {
        name: "deny_socks5_auth".to_string(),
        description: "Deny SOCKS5 authentication".to_string(),
        parameters: vec![
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for denial (for logging)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "deny_socks5_auth",
            "reason": "Invalid credentials"
        }),
    }
}

fn forward_socks5_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "forward_socks5_data".to_string(),
        description: "Forward SOCKS5 data without modification (MITM mode)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "forward_socks5_data"
        }),
    }
}

fn modify_socks5_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "modify_socks5_data".to_string(),
        description: "Modify SOCKS5 data before forwarding (MITM mode)".to_string(),
        parameters: vec![
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Modified data to send (base64 or UTF-8)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "modify_socks5_data",
            "data": "Modified payload"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the SOCKS5 connection (MITM mode)".to_string(),
        parameters: vec![
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for closing (for logging)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "close_connection",
            "reason": "Suspicious data detected"
        }),
    }
}

// ============================================================================
// SOCKS5 Event Type Constants
// ============================================================================

pub static SOCKS5_AUTH_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_auth_request",
        "SOCKS5 client authentication request (username/password)"
    )
    .with_parameters(vec![
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Username provided by client".to_string(),
            required: true,
        },
        Parameter {
            name: "password".to_string(),
            type_hint: "string".to_string(),
            description: "Password provided by client".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        allow_socks5_auth_action(),
        deny_socks5_auth_action(),
    ])
});

pub static SOCKS5_CONNECT_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_connect_request",
        "SOCKS5 CONNECT request to target address"
    )
    .with_parameters(vec![
        Parameter {
            name: "target".to_string(),
            type_hint: "string".to_string(),
            description: "Target address (IP or domain:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Authenticated username (if any)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        allow_socks5_connect_action(),
        deny_socks5_connect_action(),
    ])
});

pub static SOCKS5_DATA_TO_TARGET_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_data_to_target",
        "Data from client to target (MITM inspection mode)"
    )
    .with_parameters(vec![
        Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data being sent from client to target (UTF-8 or hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "target".to_string(),
            type_hint: "string".to_string(),
            description: "Target address (IP or domain:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Authenticated username (if any)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        forward_socks5_data_action(),
        modify_socks5_data_action(),
        close_connection_action(),
    ])
});

pub static SOCKS5_DATA_FROM_TARGET_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_data_from_target",
        "Data from target to client (MITM inspection mode)"
    )
    .with_parameters(vec![
        Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data being sent from target to client (UTF-8 or hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "target".to_string(),
            type_hint: "string".to_string(),
            description: "Target address (IP or domain:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Authenticated username (if any)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        forward_socks5_data_action(),
        modify_socks5_data_action(),
        close_connection_action(),
    ])
});

pub fn get_socks5_event_types() -> Vec<EventType> {
    vec![
        SOCKS5_AUTH_REQUEST_EVENT.clone(),
        SOCKS5_CONNECT_REQUEST_EVENT.clone(),
        SOCKS5_DATA_TO_TARGET_EVENT.clone(),
        SOCKS5_DATA_FROM_TARGET_EVENT.clone(),
    ]
}
