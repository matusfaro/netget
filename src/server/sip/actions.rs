//! SIP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct SipProtocol;

impl SipProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SipProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            send_sip_invite_action(),
            send_sip_bye_action(),
            update_registration_action(),
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            sip_register_action(),
            sip_invite_action(),
            sip_bye_action(),
            sip_ack_action(),
            sip_options_action(),
            sip_cancel_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "SIP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_sip_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>SIP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["sip", "voip", "session initiation"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("rsipstack v0.2.52 - RFC 3261 compliant SIP stack")
            .llm_control("Registration decisions + call routing + SDP generation")
            .e2e_testing("rvoip-sip-client - 1-2 LLM calls with scripting")
            .notes("Perfect scripting candidate, VoIP signaling honeypot")
            .build()
    }
    fn description(&self) -> &'static str {
        "SIP server for VoIP signaling"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a SIP server on port 5060 for VoIP registration and call signaling"
    }
    fn group_name(&self) -> &'static str {
        "Proxy & Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 5060,
                "base_stack": "sip",
                "instruction": "SIP VoIP signaling server. Accept REGISTER requests with 200 OK. For INVITE requests, respond with 200 OK and SDP. Log all call setup attempts."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 5060,
                "base_stack": "sip",
                "event_handlers": [{
                    "event_pattern": "sip_register",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }, {
                    "event_pattern": "sip_invite",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }, {
                    "event_pattern": "sip_options",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 5060,
                "base_stack": "sip",
                "event_handlers": [{
                    "event_pattern": "sip_register",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "sip_register",
                            "status_code": 200,
                            "expires": 3600
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SipProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::sip::SipServer;
            SipServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
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
            "sip_register" => self.execute_sip_register(action),
            "sip_invite" => self.execute_sip_invite(action),
            "sip_bye" => self.execute_sip_bye(action),
            "sip_ack" => Ok(ActionResult::NoAction), // ACK doesn't require response
            "sip_options" => self.execute_sip_options(action),
            "sip_cancel" => self.execute_sip_cancel(action),
            "send_sip_invite" => self.execute_send_invite(action),
            "send_sip_bye" => self.execute_send_bye(action),
            "update_registration" => self.execute_update_registration(action),
            _ => Err(anyhow::anyhow!("Unknown SIP action: {}", action_type)),
        }
    }
}

impl SipProtocol {
    /// Execute SIP REGISTER action (network event)
    /// Just validates the action - actual response data comes from action JSON itself
    fn execute_sip_register(&self, _action: serde_json::Value) -> Result<ActionResult> {
        // Validation happens in mod.rs when parsing the action
        Ok(ActionResult::NoAction)
    }

    /// Execute SIP INVITE action (network event)
    fn execute_sip_invite(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::NoAction)
    }

    /// Execute SIP BYE action (network event)
    fn execute_sip_bye(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::NoAction)
    }

    /// Execute SIP OPTIONS action (network event)
    fn execute_sip_options(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::NoAction)
    }

    /// Execute SIP CANCEL action (network event)
    fn execute_sip_cancel(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::NoAction)
    }

    /// Execute send SIP INVITE action (user-triggered)
    fn execute_send_invite(&self, _action: serde_json::Value) -> Result<ActionResult> {
        // This would trigger an outbound INVITE
        // For now, return NoAction as it requires server-side state
        Ok(ActionResult::NoAction)
    }

    /// Execute send SIP BYE action (user-triggered)
    fn execute_send_bye(&self, _action: serde_json::Value) -> Result<ActionResult> {
        // This would trigger an outbound BYE
        Ok(ActionResult::NoAction)
    }

    /// Execute update registration action (user-triggered)
    fn execute_update_registration(&self, _action: serde_json::Value) -> Result<ActionResult> {
        // This would update the registration database
        Ok(ActionResult::NoAction)
    }
}

/// SIP event types
fn get_sip_event_types() -> Vec<EventType> {
    vec![
        SIP_REGISTER_EVENT.clone(),
        SIP_INVITE_EVENT.clone(),
        SIP_BYE_EVENT.clone(),
        SIP_ACK_EVENT.clone(),
        SIP_OPTIONS_EVENT.clone(),
        SIP_CANCEL_EVENT.clone(),
    ]
}

/// SIP REGISTER event
pub static SIP_REGISTER_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "sip_register",
        "Client sends REGISTER to register user location",
        json!({
            "type": "sip_register",
            "status_code": 200,
            "expires": 3600
        })
    )
});

/// SIP INVITE event
pub static SIP_INVITE_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("sip_invite", "Client initiates session with INVITE request", json!({"type": "placeholder", "event_id": "sip_invite"})));

/// SIP BYE event
pub static SIP_BYE_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("sip_bye", "Client or server terminates session with BYE", json!({"type": "placeholder", "event_id": "sip_bye"})));

/// SIP ACK event
pub static SIP_ACK_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("sip_ack", "Client acknowledges INVITE response", json!({"type": "placeholder", "event_id": "sip_ack"})));

/// SIP OPTIONS event
pub static SIP_OPTIONS_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("sip_options", "Client queries server capabilities", json!({"type": "placeholder", "event_id": "sip_options"})));

/// SIP CANCEL event
pub static SIP_CANCEL_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("sip_cancel", "Client cancels pending INVITE request", json!({"type": "placeholder", "event_id": "sip_cancel"})));

// Action definitions
fn sip_register_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_register".to_string(),
        description: "Respond to SIP REGISTER request".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "SIP status code (200=OK, 403=Forbidden, 401=Unauthorized)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "reason_phrase".to_string(),
                type_hint: "string".to_string(),
                description: "Optional reason phrase (default based on status code)".to_string(),
                required: false,
            },
            Parameter {
                name: "expires".to_string(),
                type_hint: "number".to_string(),
                description: "Registration expiration in seconds (default 3600)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "sip_register",
            "status_code": 200,
            "expires": 3600
        }),
        log_template: None,
    }
}

fn sip_invite_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_invite".to_string(),
        description: "Respond to SIP INVITE request".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "SIP status code (200=OK, 486=Busy, 603=Decline, 180=Ringing)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "reason_phrase".to_string(),
                type_hint: "string".to_string(),
                description: "Optional reason phrase".to_string(),
                required: false,
            },
            Parameter {
                name: "sdp".to_string(),
                type_hint: "string".to_string(),
                description: "Session Description Protocol body (required for 200 OK)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "sip_invite",
            "status_code": 200,
            "sdp": "v=0\no=- 0 0 IN IP4 127.0.0.1\ns=Call\nc=IN IP4 127.0.0.1\nt=0 0\nm=audio 8000 RTP/AVP 0\n"
        }),
        log_template: None,
    }
}

fn sip_bye_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_bye".to_string(),
        description: "Respond to SIP BYE request".to_string(),
        parameters: vec![Parameter {
            name: "status_code".to_string(),
            type_hint: "number".to_string(),
            description: "SIP status code (default 200)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "sip_bye",
            "status_code": 200
        }),
        log_template: None,
    }
}

fn sip_ack_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_ack".to_string(),
        description: "Process SIP ACK (no response needed)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "sip_ack"
        }),
        log_template: None,
    }
}

fn sip_options_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_options".to_string(),
        description: "Respond to SIP OPTIONS request".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "SIP status code (default 200)".to_string(),
                required: false,
            },
            Parameter {
                name: "allow_methods".to_string(),
                type_hint: "array".to_string(),
                description: "Array of supported SIP methods".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "sip_options",
            "status_code": 200,
            "allow_methods": ["INVITE", "ACK", "BYE", "REGISTER", "OPTIONS"]
        }),
        log_template: None,
    }
}

fn sip_cancel_action() -> ActionDefinition {
    ActionDefinition {
        name: "sip_cancel".to_string(),
        description: "Respond to SIP CANCEL request".to_string(),
        parameters: vec![Parameter {
            name: "status_code".to_string(),
            type_hint: "number".to_string(),
            description: "SIP status code (default 200)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "sip_cancel",
            "status_code": 200
        }),
        log_template: None,
    }
}

// User-triggered actions
fn send_sip_invite_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_sip_invite".to_string(),
        description: "Send SIP INVITE to initiate outbound call".to_string(),
        parameters: vec![
            Parameter {
                name: "to".to_string(),
                type_hint: "string".to_string(),
                description: "SIP URI of recipient (e.g., sip:bob@example.com)".to_string(),
                required: true,
            },
            Parameter {
                name: "from".to_string(),
                type_hint: "string".to_string(),
                description: "SIP URI of caller".to_string(),
                required: true,
            },
            Parameter {
                name: "sdp".to_string(),
                type_hint: "string".to_string(),
                description: "Session Description Protocol body".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_sip_invite",
            "to": "sip:bob@example.com",
            "from": "sip:alice@example.com",
            "sdp": "v=0\no=- 0 0 IN IP4 127.0.0.1\n..."
        }),
        log_template: None,
    }
}

fn send_sip_bye_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_sip_bye".to_string(),
        description: "Send SIP BYE to terminate active session".to_string(),
        parameters: vec![Parameter {
            name: "call_id".to_string(),
            type_hint: "string".to_string(),
            description: "Call-ID of session to terminate".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_sip_bye",
            "call_id": "call-123@example.com"
        }),
        log_template: None,
    }
}

fn update_registration_action() -> ActionDefinition {
    ActionDefinition {
        name: "update_registration".to_string(),
        description: "Update registration database bindings".to_string(),
        parameters: vec![Parameter {
            name: "bindings".to_string(),
            type_hint: "object".to_string(),
            description: "Object mapping username to contact URI".to_string(),
            required: true,
        }],
        example: json!({
            "type": "update_registration",
            "bindings": {
                "alice@localhost": "sip:alice@192.168.1.10:5060",
                "bob@localhost": "sip:bob@192.168.1.11:5060"
            }
        }),
        log_template: None,
    }
}
