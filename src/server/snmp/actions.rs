//! SNMP protocol actions implementation

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

/// SNMP protocol action handler
pub struct SnmpProtocol;

impl SnmpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SnmpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // SNMP has async action for sending traps
        vec![send_trap_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_snmp_response_action(),
            send_snmp_error_action(),
            ignore_request_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "SNMP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_snmp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>SNMP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["snmp", "snmp agent"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .implementation("rasn-snmp v0.18 for parsing + manual BER encoding")
            .llm_control("OID responses (sysDescr, ifTable, custom MIBs)")
            .e2e_testing("net-snmp tools (snmpget)")
            .notes("SNMPv1/v2c only, manual BER encoding")
            .build()
    }
    fn description(&self) -> &'static str {
        "SNMP agent for network monitoring"
    }
    fn example_prompt(&self) -> &'static str {
        "SNMP Port 8161 serve OID 1.3.6.1.2.1.1.1.0 (sysDescr) return 'NetGet SNMP Server v0.1'"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        StartupExamples::new(
            // LLM-driven example
            json!({
                "type": "open_server",
                "port": 161,
                "base_stack": "snmp",
                "instruction": "SNMP agent serving OID 1.3.6.1.2.1.1.1.0 (sysDescr) as 'NetGet SNMP Server v1.0'"
            }),
            // Script-based example
            json!({
                "type": "open_server",
                "port": 161,
                "base_stack": "snmp",
                "event_handlers": [{
                    "event_pattern": "snmp_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# Handle SNMP GET request\noids = event.get('oids', [])\nvariables = [{'oid': oid, 'type': 'string', 'value': 'NetGet SNMP Server'} for oid in oids]\nrespond([{'type': 'send_snmp_response', 'variables': variables}])"
                    }
                }]
            }),
            // Static handler example
            json!({
                "type": "open_server",
                "port": 161,
                "base_stack": "snmp",
                "event_handlers": [{
                    "event_pattern": "snmp_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_snmp_response",
                            "variables": [{
                                "oid": "1.3.6.1.2.1.1.1.0",
                                "type": "string",
                                "value": "NetGet SNMP Server v1.0"
                            }]
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SnmpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::snmp::SnmpServer;
            SnmpServer::spawn_with_llm_actions(
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
            "send_trap" => self.execute_send_trap(action),
            "send_snmp_response" => self.execute_send_snmp_response(action),
            "send_snmp_error" => self.execute_send_snmp_error(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown SNMP action: {}", action_type)),
        }
    }
}

impl SnmpProtocol {
    /// Execute send_trap async action
    fn execute_send_trap(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let variables = action
            .get("variables")
            .and_then(|v| v.as_array())
            .context("Missing 'variables' parameter")?;

        // Encode trap data as JSON for now
        // The caller will need to convert this to actual SNMP trap format
        let trap_data = json!({
            "variables": variables
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&trap_data).context("Failed to serialize trap data")?,
        ))
    }

    /// Execute send_snmp_response sync action
    fn execute_send_snmp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let variables = action
            .get("variables")
            .and_then(|v| v.as_array())
            .context("Missing 'variables' parameter")?;

        // Encode response data as JSON
        // The caller will convert this to actual SNMP response format
        let response_data = json!({
            "variables": variables,
            "error": false
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize SNMP response")?,
        ))
    }

    /// Execute send_snmp_error sync action
    fn execute_send_snmp_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_message = action
            .get("error_message")
            .and_then(|v| v.as_str())
            .context("Missing 'error_message' parameter")?;

        // Encode error response as JSON
        let response_data = json!({
            "error": true,
            "error_message": error_message
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize SNMP error")?,
        ))
    }
}

/// Action definition for send_trap (async)
fn send_trap_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_trap".to_string(),
        description: "Send SNMP trap to a target address (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target address in format 'IP:port'".to_string(),
                required: true,
            },
            Parameter {
                name: "variables".to_string(),
                type_hint: "array".to_string(),
                description: "Array of variable bindings with oid, type, and value".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_trap",
            "target": "127.0.0.1:162",
            "variables": [
                {"oid": "1.3.6.1.2.1.1.3.0", "type": "timeticks", "value": 12345}
            ]
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> SNMP trap to {target}")
                .with_debug("SNMP send_trap: target={target}, {variables_len} vars"),
        ),
    }
}

/// Action definition for send_snmp_response (sync)
fn send_snmp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_snmp_response".to_string(),
        description: "Send SNMP response with variable bindings".to_string(),
        parameters: vec![Parameter {
            name: "variables".to_string(),
            type_hint: "array".to_string(),
            description: "Array of variable bindings with oid, type, and value".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_snmp_response",
            "variables": [
                {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "System Description"},
                {"oid": "1.3.6.1.2.1.1.5.0", "type": "string", "value": "hostname"}
            ]
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> SNMP response {variables_len} vars")
                .with_debug("SNMP send_snmp_response: {variables_len} variables"),
        ),
    }
}

/// Action definition for send_snmp_error (sync)
fn send_snmp_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_snmp_error".to_string(),
        description: "Send SNMP error response".to_string(),
        parameters: vec![Parameter {
            name: "error_message".to_string(),
            type_hint: "string".to_string(),
            description: "Error message".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_snmp_error",
            "error_message": "No such object"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> SNMP error: {error_message}")
                .with_debug("SNMP send_snmp_error: {error_message}"),
        ),
    }
}

/// Action definition for ignore_request (sync)
fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this SNMP request and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("SNMP request ignored")
                .with_debug("SNMP ignore_request"),
        ),
    }
}

// ============================================================================
// SNMP Event Type Constants
// ============================================================================

pub static SNMP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "snmp_request",
        "SNMP client sent a GET/GETNEXT/GETBULK request",
        json!({
            "type": "send_snmp_response",
            "variables": [
                {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "System Description"}
            ]
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "request_type".to_string(),
            type_hint: "string".to_string(),
            description: "SNMP request type (GET, GETNEXT, GETBULK, SET)".to_string(),
            required: true,
        },
        Parameter {
            name: "oids".to_string(),
            type_hint: "array".to_string(),
            description: "Requested OIDs".to_string(),
            required: true,
        },
        Parameter {
            name: "community".to_string(),
            type_hint: "string".to_string(),
            description: "SNMP community string".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_snmp_response_action(),
        send_snmp_error_action(),
        ignore_request_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("SNMP {client_ip} {oid}")
            .with_debug("SNMP request from {client_ip}:{client_port}, OID={oid}")
            .with_trace("SNMP: {json_pretty(.)}"),
    )
});

pub fn get_snmp_event_types() -> Vec<EventType> {
    vec![SNMP_REQUEST_EVENT.clone()]
}
