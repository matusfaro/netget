//! BGP protocol actions implementation

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

/// BGP protocol action handler
pub struct BgpProtocol;

impl BgpProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_bgp_open(&self, action: serde_json::Value) -> Result<ActionResult> {
        let my_as = action
            .get("my_as")
            .and_then(|v| v.as_u64())
            .unwrap_or(65000) as u32;

        let hold_time = action
            .get("hold_time")
            .and_then(|v| v.as_u64())
            .unwrap_or(180) as u16;

        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        debug!(
            "BGP sending OPEN: AS={}, hold_time={}, router_id={}",
            my_as, hold_time, router_id
        );

        // Build OPEN message
        let mut msg = Vec::new();

        // Marker (16 bytes of 0xFF)
        msg.extend_from_slice(&[0xff; 16]);

        // Length placeholder
        msg.extend_from_slice(&[0, 0]);

        // Type = OPEN (1)
        msg.push(1);

        // Version
        msg.push(4);

        // My AS (16-bit)
        msg.extend_from_slice(&(my_as as u16).to_be_bytes());

        // Hold Time
        msg.extend_from_slice(&hold_time.to_be_bytes());

        // BGP Identifier (Router ID)
        let router_id_bytes: Vec<u8> = router_id
            .split('.')
            .filter_map(|s| s.parse::<u8>().ok())
            .collect();
        if router_id_bytes.len() == 4 {
            msg.extend_from_slice(&router_id_bytes);
        } else {
            msg.extend_from_slice(&[0, 0, 0, 0]);
        }

        // Optional Parameters Length
        msg.push(0);

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        Ok(ActionResult::Output(msg))
    }

    fn execute_send_bgp_keepalive(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("BGP sending KEEPALIVE");

        // Build KEEPALIVE message
        let mut msg = Vec::new();

        // Marker (16 bytes of 0xFF)
        msg.extend_from_slice(&[0xff; 16]);

        // Length (19 bytes for KEEPALIVE)
        msg.extend_from_slice(&19u16.to_be_bytes());

        // Type = KEEPALIVE (4)
        msg.push(4);

        Ok(ActionResult::Output(msg))
    }

    fn execute_send_bgp_update(&self, action: serde_json::Value) -> Result<ActionResult> {
        let withdrawn_routes = action
            .get("withdrawn_routes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let nlri = action
            .get("nlri")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        debug!(
            "BGP sending UPDATE: {} withdrawn, {} announced",
            withdrawn_routes.len(),
            nlri.len()
        );

        // Build UPDATE message (simplified - no path attributes for now)
        let mut msg = Vec::new();

        // Marker
        msg.extend_from_slice(&[0xff; 16]);

        // Length placeholder
        msg.extend_from_slice(&[0, 0]);

        // Type = UPDATE (2)
        msg.push(2);

        // Withdrawn Routes Length (0 for now - simplified)
        msg.extend_from_slice(&0u16.to_be_bytes());

        // Total Path Attribute Length (0 for now - simplified)
        msg.extend_from_slice(&0u16.to_be_bytes());

        // NLRI (Network Layer Reachability Information)
        // For now, just placeholder - full implementation would parse prefix/length

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        Ok(ActionResult::Output(msg))
    }

    fn execute_send_bgp_notification(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(6) as u8; // 6 = Cease

        let error_subcode = action
            .get("error_subcode")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;

        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .and_then(|s| hex::decode(s).ok())
            .unwrap_or_default();

        debug!(
            "BGP sending NOTIFICATION: code={}, subcode={}",
            error_code, error_subcode
        );

        // Build NOTIFICATION message
        let mut msg = Vec::new();

        // Marker
        msg.extend_from_slice(&[0xff; 16]);

        // Length placeholder
        msg.extend_from_slice(&[0, 0]);

        // Type = NOTIFICATION (3)
        msg.push(3);

        // Error Code
        msg.push(error_code);

        // Error Subcode
        msg.push(error_subcode);

        // Data
        msg.extend_from_slice(&data);

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        Ok(ActionResult::Output(msg))
    }

    fn execute_transition_state(&self, action: serde_json::Value) -> Result<ActionResult> {
        let new_state = action
            .get("new_state")
            .and_then(|v| v.as_str())
            .unwrap_or("Connect");

        debug!("BGP transitioning FSM to state: {}", new_state);

        // This is informational - actual state transition happens in mod.rs
        Ok(ActionResult::NoAction)
    }

    fn execute_announce_route(&self, action: serde_json::Value) -> Result<ActionResult> {
        let prefix = action.get("prefix").and_then(|v| v.as_str()).unwrap_or("");

        let next_hop = action
            .get("next_hop")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        debug!("BGP announcing route: {} via {}", prefix, next_hop);

        // This would generate an UPDATE message with the route
        // For now, return success
        Ok(ActionResult::NoAction)
    }

    fn execute_withdraw_route(&self, action: serde_json::Value) -> Result<ActionResult> {
        let prefix = action.get("prefix").and_then(|v| v.as_str()).unwrap_or("");

        debug!("BGP withdrawing route: {}", prefix);

        // This would generate an UPDATE message with withdrawn routes
        // For now, return success
        Ok(ActionResult::NoAction)
    }

    fn execute_reset_peer(&self, _action: serde_json::Value) -> Result<ActionResult> {
        debug!("BGP resetting peer connection");

        // Send NOTIFICATION (Cease) and close connection
        let error_code = 6; // Cease
        let error_subcode = 0;

        let mut msg = Vec::new();
        msg.extend_from_slice(&[0xff; 16]);
        msg.extend_from_slice(&21u16.to_be_bytes());
        msg.push(3);
        msg.push(error_code);
        msg.push(error_subcode);

        Ok(ActionResult::Output(msg))
    }
}

// Event types for BGP
pub static BGP_OPEN_EVENT: LazyLock<EventType> = LazyLock::new(|| EventType::new("bgp_open", "BGP OPEN message received from peer", json!({"type": "placeholder", "event_id": "bgp_open"})));

pub static BGP_UPDATE_EVENT: LazyLock<EventType> = LazyLock::new(|| EventType::new("bgp_update", "BGP UPDATE message received (route announcement or withdrawal)", json!({"type": "placeholder", "event_id": "bgp_update"})));

pub static BGP_KEEPALIVE_EVENT: LazyLock<EventType> = LazyLock::new(|| EventType::new("bgp_keepalive", "BGP KEEPALIVE message received", json!({"type": "placeholder", "event_id": "bgp_keepalive"})));

pub static BGP_NOTIFICATION_EVENT: LazyLock<EventType> = LazyLock::new(|| EventType::new("bgp_notification", "BGP NOTIFICATION message received (error)", json!({"type": "placeholder", "event_id": "bgp_notification"})));

// Implement Protocol trait (common functionality)
impl Protocol for BgpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "announce_route".to_string(),
                description: "Announce a BGP route to peers".to_string(),
                parameters: vec![
                    Parameter {
                        name: "prefix".to_string(),
                        type_hint: "string".to_string(),
                        description: "IP prefix to announce (e.g., \"10.0.0.0/24\")".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "next_hop".to_string(),
                        type_hint: "string".to_string(),
                        description: "Next hop IP address".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "announce_route",
                    "prefix": "10.0.0.0/24",
                    "next_hop": "192.168.1.1"
                }),
            },
            ActionDefinition {
                name: "withdraw_route".to_string(),
                description: "Withdraw a previously announced BGP route".to_string(),
                parameters: vec![Parameter {
                    name: "prefix".to_string(),
                    type_hint: "string".to_string(),
                    description: "IP prefix to withdraw (e.g., \"10.0.0.0/24\")".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "withdraw_route",
                    "prefix": "10.0.0.0/24"
                }),
            },
            ActionDefinition {
                name: "reset_peer".to_string(),
                description: "Reset BGP session with peer (send NOTIFICATION and close)"
                    .to_string(),
                parameters: vec![],
                example: json!({
                    "type": "reset_peer"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_bgp_open".to_string(),
                description: "Send BGP OPEN message to establish session".to_string(),
                parameters: vec![
                    Parameter {
                        name: "my_as".to_string(),
                        type_hint: "number".to_string(),
                        description: "Local AS number".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "hold_time".to_string(),
                        type_hint: "number".to_string(),
                        description: "Hold time in seconds (default 180)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "router_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "BGP router identifier (IPv4 address format)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_bgp_open",
                    "my_as": 65000,
                    "hold_time": 180,
                    "router_id": "192.168.1.100"
                }),
            },
            ActionDefinition {
                name: "send_bgp_keepalive".to_string(),
                description: "Send BGP KEEPALIVE message".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "send_bgp_keepalive"
                }),
            },
            ActionDefinition {
                name: "send_bgp_update".to_string(),
                description: "Send BGP UPDATE message (route announcement/withdrawal)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "withdrawn_routes".to_string(),
                        type_hint: "array".to_string(),
                        description: "List of prefixes to withdraw".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "nlri".to_string(),
                        type_hint: "array".to_string(),
                        description: "Network Layer Reachability Information (announced routes)"
                            .to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_bgp_update",
                    "nlri": ["10.0.0.0/24"]
                }),
            },
            ActionDefinition {
                name: "send_bgp_notification".to_string(),
                description: "Send BGP NOTIFICATION message (error) and close connection"
                    .to_string(),
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
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded error data".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_bgp_notification",
                    "error_code": 6,
                    "error_subcode": 0
                }),
            },
            ActionDefinition {
                name: "transition_state".to_string(),
                description: "Transition BGP FSM to a new state".to_string(),
                parameters: vec![Parameter {
                    name: "new_state".to_string(),
                    type_hint: "string".to_string(),
                    description:
                        "Target FSM state (Idle/Connect/Active/OpenSent/OpenConfirm/Established)"
                            .to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "transition_state",
                    "new_state": "Established"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more BGP messages before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "BGP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            BGP_OPEN_EVENT.clone(),
            BGP_UPDATE_EVENT.clone(),
            BGP_KEEPALIVE_EVENT.clone(),
            BGP_NOTIFICATION_EVENT.clone(),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>BGP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bgp", "border gateway"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("Manual BGP-4 (RFC 4271), 6-state FSM")
            .llm_control("Peering decisions, route advertisements")
            .e2e_testing("Manual BGP client")
            .notes("No RIB, no route propagation, session tracking only. Standard port is 179 but can run on any port.")
            .build()
    }
    fn description(&self) -> &'static str {
        "BGP routing server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a BGP routing server on port 8179"
    }
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        use crate::llm::actions::ParameterDefinition;
        vec![
                ParameterDefinition {
                    name: "as_number".to_string(),
                    type_hint: "integer".to_string(),
                    description: "BGP Autonomous System Number (1-4294967295). Use private ASNs (64512-65534) for testing.".to_string(),
                    required: false,
                    example: json!(65001),
                },
                ParameterDefinition {
                    name: "router_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "BGP router ID in IPv4 address format (e.g., 192.168.1.1)".to_string(),
                    required: false,
                    example: json!("192.168.1.1"),
                },
            ]
    }
    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM controls BGP peering decisions
            json!({
                "type": "open_server",
                "port": 179,
                "base_stack": "bgp",
                "instruction": "Accept BGP peers and respond with KEEPALIVE to maintain sessions",
                "startup_params": {
                    "as_number": 65001,
                    "router_id": "192.168.1.1"
                }
            }),
            // Script mode: Code-based BGP message handling
            json!({
                "type": "open_server",
                "port": 179,
                "base_stack": "bgp",
                "startup_params": {
                    "as_number": 65001,
                    "router_id": "192.168.1.1"
                },
                "event_handlers": [{
                    "event_pattern": "bgp_open",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<bgp_server_handler>"
                    }
                }]
            }),
            // Static mode: Fixed BGP response flow
            json!({
                "type": "open_server",
                "port": 179,
                "base_stack": "bgp",
                "startup_params": {
                    "as_number": 65001,
                    "router_id": "192.168.1.1"
                },
                "event_handlers": [
                    {
                        "event_pattern": "bgp_open",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_bgp_open",
                                "my_as": 65001,
                                "hold_time": 180,
                                "router_id": "192.168.1.1"
                            }]
                        }
                    },
                    {
                        "event_pattern": "bgp_keepalive",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "send_bgp_keepalive"
                            }]
                        }
                    }
                ]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for BgpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::bgp::BgpServer;
            BgpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing action type")?;

        match action_type {
            "send_bgp_open" => self.execute_send_bgp_open(action),
            "send_bgp_keepalive" => self.execute_send_bgp_keepalive(action),
            "send_bgp_update" => self.execute_send_bgp_update(action),
            "send_bgp_notification" => self.execute_send_bgp_notification(action),
            "transition_state" => self.execute_transition_state(action),
            "announce_route" => self.execute_announce_route(action),
            "withdraw_route" => self.execute_withdraw_route(action),
            "reset_peer" => self.execute_reset_peer(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown BGP action type: {}", action_type)),
        }
    }
}
