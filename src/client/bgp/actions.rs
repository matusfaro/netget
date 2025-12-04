//! BGP client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::{ConnectContext, EventType};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// BGP client connected event
pub static BGP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_connected",
        "BGP client successfully connected to BGP peer and completed OPEN handshake",
        json!({"type": "wait_for_more"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote BGP peer address".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_as".to_string(),
            type_hint: "number".to_string(),
            description: "Peer AS number".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_router_id".to_string(),
            type_hint: "string".to_string(),
            description: "Peer BGP router ID".to_string(),
            required: true,
        },
        Parameter {
            name: "hold_time".to_string(),
            type_hint: "number".to_string(),
            description: "Negotiated hold time in seconds".to_string(),
            required: true,
        },
    ])
});

/// BGP UPDATE message received event
pub static BGP_CLIENT_UPDATE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_update_received",
        "BGP UPDATE message received from peer (route announcement or withdrawal)",
        json!({"type": "send_keepalive"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "update_data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Raw UPDATE message data (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "update_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of UPDATE message in bytes".to_string(),
            required: true,
        },
    ])
});

/// BGP NOTIFICATION message received event
pub static BGP_CLIENT_NOTIFICATION_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_notification_received",
        "BGP NOTIFICATION message received from peer (error, connection will close)",
        json!({"type": "wait_for_more"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "error_code".to_string(),
            type_hint: "number".to_string(),
            description: "BGP error code".to_string(),
            required: true,
        },
        Parameter {
            name: "error_subcode".to_string(),
            type_hint: "number".to_string(),
            description: "BGP error subcode".to_string(),
            required: true,
        },
        Parameter {
            name: "error_data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Error data (as hex string)".to_string(),
            required: false,
        },
    ])
});

/// BGP client protocol action handler
pub struct BgpClientProtocol;

impl BgpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for BgpClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_keepalive".to_string(),
                description: "Send BGP KEEPALIVE message to peer".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "send_keepalive"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "send_notification".to_string(),
                description: "Send BGP NOTIFICATION message and close connection".to_string(),
                parameters: vec![
                    Parameter {
                        name: "error_code".to_string(),
                        type_hint: "number".to_string(),
                        description: "BGP error code (6 = Cease)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "error_subcode".to_string(),
                        type_hint: "number".to_string(),
                        description: "BGP error subcode".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_notification",
                    "error_code": 6,
                    "error_subcode": 0
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from BGP peer (sends NOTIFICATION with Cease)".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            log_template: None,
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_keepalive".to_string(),
                description: "Send BGP KEEPALIVE message in response to received message"
                    .to_string(),
                parameters: vec![],
                example: json!({
                    "type": "send_keepalive"
                }),
            log_template: None,
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more BGP messages before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            log_template: None,
            },
        ]
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            BGP_CLIENT_CONNECTED_EVENT.clone(),
            BGP_CLIENT_UPDATE_RECEIVED_EVENT.clone(),
            BGP_CLIENT_NOTIFICATION_RECEIVED_EVENT.clone(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "BGP"
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>BGP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bgp", "border gateway"]
    }
    fn description(&self) -> &'static str {
        "BGP routing client (query mode)"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to BGP peer at 192.168.1.1:179 and query routing table"
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "local_as".to_string(),
                type_hint: "integer".to_string(),
                description: "Local BGP AS number (can be fake for monitoring)".to_string(),
                required: false,
                example: json!(65000),
            },
            ParameterDefinition {
                name: "router_id".to_string(),
                type_hint: "string".to_string(),
                description: "BGP router ID in IPv4 format".to_string(),
                required: false,
                example: json!("192.168.1.100"),
            },
            ParameterDefinition {
                name: "hold_time".to_string(),
                type_hint: "integer".to_string(),
                description: "BGP hold time in seconds (default 180)".to_string(),
                required: false,
                example: json!(180),
            },
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("Manual BGP-4 query client (RFC 4271)")
            .llm_control("Session establishment, route monitoring")
            .e2e_testing("NetGet BGP server")
            .notes("Query mode only, no active route announcement, no RIB")
            .build()
    }
    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM controls BGP session and route monitoring
            json!({
                "type": "open_client",
                "remote_addr": "192.168.1.1:179",
                "base_stack": "bgp",
                "instruction": "Establish BGP session and monitor routing updates",
                "startup_params": {
                    "local_as": 65000,
                    "router_id": "192.168.1.100"
                }
            }),
            // Script mode: Code-based BGP update handling
            json!({
                "type": "open_client",
                "remote_addr": "192.168.1.1:179",
                "base_stack": "bgp",
                "startup_params": {
                    "local_as": 65000,
                    "router_id": "192.168.1.100"
                },
                "event_handlers": [{
                    "event_pattern": "bgp_update_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<bgp_client_handler>"
                    }
                }]
            }),
            // Static mode: Fixed BGP keepalive response
            json!({
                "type": "open_client",
                "remote_addr": "192.168.1.1:179",
                "base_stack": "bgp",
                "startup_params": {
                    "local_as": 65000,
                    "router_id": "192.168.1.100",
                    "hold_time": 180
                },
                "event_handlers": [
                    {
                        "event_pattern": "bgp_connected",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_keepalive"
                            }]
                        }
                    },
                    {
                        "event_pattern": "bgp_update_received",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_keepalive"
                            }]
                        }
                    }
                ]
            }),
        )
    }
}

// Implement Client trait (client-specific functionality)
impl Client for BgpClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            use crate::client::bgp::BgpClient;
            BgpClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_keepalive" => {
                // Build BGP KEEPALIVE message
                let mut msg = Vec::new();

                // Marker (16 bytes of 0xFF)
                msg.extend_from_slice(&[0xff; 16]);

                // Length (19 bytes for KEEPALIVE)
                msg.extend_from_slice(&19u16.to_be_bytes());

                // Type = KEEPALIVE (4)
                msg.push(4);

                Ok(ClientActionResult::SendData(msg))
            }
            "send_notification" => {
                let error_code = action
                    .get("error_code")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(6) as u8; // 6 = Cease

                let error_subcode = action
                    .get("error_subcode")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u8;

                // Build BGP NOTIFICATION message
                let mut msg = Vec::new();

                // Marker
                msg.extend_from_slice(&[0xff; 16]);

                // Length placeholder
                let msg_len = 19 + 2; // header + error_code + error_subcode
                msg.extend_from_slice(&(msg_len as u16).to_be_bytes());

                // Type = NOTIFICATION (3)
                msg.push(3);

                // Error Code
                msg.push(error_code);

                // Error Subcode
                msg.push(error_subcode);

                Ok(ClientActionResult::SendData(msg))
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown BGP client action: {}",
                action_type
            )),
        }
    }
}
