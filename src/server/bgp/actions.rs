//! BGP protocol actions implementation

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

    /// Encode a prefix such as "10.0.0.0/24" the way RFC 4271 4.3 wants it: a one-byte
    /// length in bits followed by only the significant bytes of the prefix.
    fn encode_prefix(prefix: &str) -> Result<Vec<u8>> {
        let (addr, bits) = prefix
            .split_once('/')
            .context("BGP prefix must be in CIDR form, e.g. 10.0.0.0/24")?;
        let bits: u8 = bits.parse().context("Invalid prefix length")?;
        if bits > 32 {
            return Err(anyhow::anyhow!("Prefix length {} exceeds 32", bits));
        }
        let octets: Vec<u8> = addr
            .split('.')
            .map(|o| o.parse::<u8>().context("Invalid IPv4 octet"))
            .collect::<Result<_>>()?;
        if octets.len() != 4 {
            return Err(anyhow::anyhow!("Invalid IPv4 address: {}", addr));
        }
        let significant = bits.div_ceil(8) as usize;
        let mut out = vec![bits];
        out.extend_from_slice(&octets[..significant]);
        Ok(out)
    }

    fn execute_send_bgp_update(&self, action: serde_json::Value) -> Result<ActionResult> {
        let withdrawn_routes: Vec<String> = action
            .get("withdrawn_routes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let nlri: Vec<String> = action
            .get("nlri")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        debug!(
            "BGP sending UPDATE: {} withdrawn, {} announced",
            withdrawn_routes.len(),
            nlri.len()
        );

        // Withdrawn routes.
        let mut withdrawn_bytes = Vec::new();
        for prefix in &withdrawn_routes {
            withdrawn_bytes.extend_from_slice(&Self::encode_prefix(prefix)?);
        }

        // NLRI.
        let mut nlri_bytes = Vec::new();
        for prefix in &nlri {
            nlri_bytes.extend_from_slice(&Self::encode_prefix(prefix)?);
        }

        // Path attributes. RFC 4271 9.1 makes ORIGIN, AS_PATH and NEXT_HOP mandatory whenever
        // NLRI is present, so a withdrawal-only UPDATE carries none and an announcement
        // carries all three. Previously this whole section was hardcoded to zero length and
        // the documented withdrawn_routes/nlri parameters were read, logged and then dropped -
        // every UPDATE went out empty.
        let mut attrs = Vec::new();
        if !nlri_bytes.is_empty() {
            let origin = match action.get("origin").and_then(|v| v.as_str()) {
                Some("EGP") => 1u8,
                Some("INCOMPLETE") => 2,
                _ => 0, // IGP
            };
            // ORIGIN: well-known mandatory (flags 0x40), type 1, length 1
            attrs.extend_from_slice(&[0x40, 1, 1, origin]);

            // AS_PATH: well-known mandatory, type 2. One AS_SEQUENCE segment.
            let as_path: Vec<u16> = action
                .get("as_path")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u16))
                        .collect()
                })
                .unwrap_or_default();
            let mut as_path_value = Vec::new();
            if !as_path.is_empty() {
                as_path_value.push(2u8); // AS_SEQUENCE
                as_path_value.push(as_path.len() as u8);
                for asn in &as_path {
                    as_path_value.extend_from_slice(&asn.to_be_bytes());
                }
            }
            attrs.extend_from_slice(&[0x40, 2, as_path_value.len() as u8]);
            attrs.extend_from_slice(&as_path_value);

            // NEXT_HOP: well-known mandatory, type 3, length 4
            let next_hop = action
                .get("next_hop")
                .and_then(|v| v.as_str())
                .context("send_bgp_update with nlri requires next_hop")?;
            let octets: Vec<u8> = next_hop
                .split('.')
                .map(|o| o.parse::<u8>().context("Invalid next_hop octet"))
                .collect::<Result<_>>()?;
            if octets.len() != 4 {
                return Err(anyhow::anyhow!("Invalid next_hop address: {}", next_hop));
            }
            attrs.extend_from_slice(&[0x40, 3, 4]);
            attrs.extend_from_slice(&octets);
        }

        let mut msg = Vec::new();
        msg.extend_from_slice(&[0xff; 16]); // Marker
        msg.extend_from_slice(&[0, 0]); // Length placeholder
        msg.push(2); // Type = UPDATE
        msg.extend_from_slice(&(withdrawn_bytes.len() as u16).to_be_bytes());
        msg.extend_from_slice(&withdrawn_bytes);
        msg.extend_from_slice(&(attrs.len() as u16).to_be_bytes());
        msg.extend_from_slice(&attrs);
        msg.extend_from_slice(&nlri_bytes);

        let msg_len = u16::try_from(msg.len()).context("BGP UPDATE exceeds 65535 bytes")?;
        if msg_len as usize > 4096 {
            return Err(anyhow::anyhow!(
                "BGP UPDATE is {} bytes, over the RFC 4271 4096-byte maximum",
                msg_len
            ));
        }
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

// ============================================================================
// Action Definitions (shared between get_sync_actions() and the event types).
// ============================================================================

fn send_bgp_open_action() -> ActionDefinition {
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP OPEN AS{my_as} hold={hold_time}s")
                .with_debug(
                    "BGP send_bgp_open: AS={my_as}, hold_time={hold_time}, router_id={router_id}",
                ),
        ),
    }
}

fn send_bgp_keepalive_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bgp_keepalive".to_string(),
        description: "Send BGP KEEPALIVE message".to_string(),
        parameters: vec![],
        example: json!({
            "type": "send_bgp_keepalive"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP KEEPALIVE")
                .with_debug("BGP send_bgp_keepalive"),
        ),
    }
}

fn send_bgp_update_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bgp_update".to_string(),
        description: "Send a BGP UPDATE announcing and/or withdrawing routes".to_string(),
        parameters: vec![
            Parameter {
                name: "withdrawn_routes".to_string(),
                type_hint: "array".to_string(),
                description: "CIDR prefixes to withdraw, e.g. [\"10.0.0.0/24\"]".to_string(),
                required: false,
            },
            Parameter {
                name: "nlri".to_string(),
                type_hint: "array".to_string(),
                description: "CIDR prefixes to announce, e.g. [\"192.168.0.0/16\"]".to_string(),
                required: false,
            },
            Parameter {
                name: "next_hop".to_string(),
                type_hint: "string".to_string(),
                description: "Next-hop IPv4 address. Required whenever nlri is non-empty \
                              (RFC 4271 makes NEXT_HOP mandatory for an announcement)."
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "as_path".to_string(),
                type_hint: "array".to_string(),
                description: "AS numbers forming the AS_SEQUENCE, e.g. [65001]. Empty means an \
                              empty AS_PATH, which is what an iBGP-originated route looks like."
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "origin".to_string(),
                type_hint: "string".to_string(),
                description: "ORIGIN attribute: 'IGP' (default), 'EGP' or 'INCOMPLETE'".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_bgp_update",
            "nlri": ["10.0.0.0/24"],
            "next_hop": "192.168.1.1",
            "as_path": [65001],
            "origin": "IGP"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP UPDATE")
                .with_debug("BGP send_bgp_update: withdrawn={withdrawn_routes} nlri={nlri}"),
        ),
    }
}

fn send_bgp_notification_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bgp_notification".to_string(),
        description: "Send BGP NOTIFICATION message (error) and close connection".to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP NOTIFICATION code={error_code}")
                .with_debug(
                    "BGP send_bgp_notification: code={error_code}, subcode={error_subcode}",
                ),
        ),
    }
}

fn transition_state_action() -> ActionDefinition {
    ActionDefinition {
        name: "transition_state".to_string(),
        description: "Transition BGP FSM to a new state".to_string(),
        parameters: vec![Parameter {
            name: "new_state".to_string(),
            type_hint: "string".to_string(),
            description: "Target FSM state (Idle/Connect/Active/OpenSent/OpenConfirm/Established)"
                .to_string(),
            required: true,
        }],
        example: json!({
            "type": "transition_state",
            "new_state": "Established"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP FSM -> {new_state}")
                .with_debug("BGP transition_state: new_state={new_state}"),
        ),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more BGP messages before responding".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BGP wait for more")
                .with_debug("BGP wait_for_more: awaiting additional messages"),
        ),
    }
}

// ============================================================================
// Event Types
//
// `call_llm` builds the model's tool list from `EventType::actions`, not from
// get_sync_actions(), so every event has to carry its action list. All four
// previously carried none, and all four used a `{"type": "placeholder"}`
// response_example - which is rendered verbatim into the prompt.
// ============================================================================

/// Actions any BGP session event can respond with.
fn bgp_response_actions() -> Vec<ActionDefinition> {
    vec![
        send_bgp_open_action(),
        send_bgp_keepalive_action(),
        send_bgp_update_action(),
        send_bgp_notification_action(),
        wait_for_more_action(),
    ]
}

pub static BGP_OPEN_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_open",
        "BGP OPEN message received from peer - decide whether to peer, and on what terms",
        json!({
            "type": "send_bgp_open",
            "my_as": 65001,
            "hold_time": 180,
            "router_id": "192.168.1.1"
        }),
    )
    .with_alternative_example(json!({
        "type": "send_bgp_notification",
        "error_code": 2,
        "error_subcode": 2
    }))
    .with_actions(bgp_response_actions())
    .with_log_template(
        LogTemplate::new()
            .with_info("BGP OPEN from AS{peer_as} router_id={router_id}")
            .with_debug("BGP OPEN: AS={peer_as} hold_time={hold_time} router_id={router_id}")
            .with_trace("BGP OPEN: {json_pretty(.)}"),
    )
});

pub static BGP_UPDATE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_update",
        "BGP UPDATE received. Carries parsed withdrawn_routes, nlri and path_attributes \
         (ORIGIN, AS_PATH, NEXT_HOP, MED, LOCAL_PREF decoded).",
        json!({ "type": "wait_for_more" }),
    )
    .with_actions(bgp_response_actions())
    .with_log_template(
        LogTemplate::new()
            .with_info("BGP UPDATE from AS{peer_as}")
            .with_debug("BGP UPDATE from AS{peer_as} on {connection_id}")
            .with_trace("BGP UPDATE: {json_pretty(.)}"),
    )
});

pub static BGP_KEEPALIVE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_keepalive",
        "BGP KEEPALIVE message received",
        json!({ "type": "send_bgp_keepalive" }),
    )
    .with_actions(bgp_response_actions())
    .with_log_template(
        LogTemplate::new()
            .with_info("BGP KEEPALIVE from AS{peer_as}")
            .with_debug("BGP KEEPALIVE on {connection_id}")
            .with_trace("BGP KEEPALIVE: {json_pretty(.)}"),
    )
});

pub static BGP_NOTIFICATION_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bgp_notification",
        "BGP NOTIFICATION received (error). The session closes regardless of the reply.",
        json!({ "type": "wait_for_more" }),
    )
    .with_actions(bgp_response_actions())
    .with_log_template(
        LogTemplate::new()
            .with_info("BGP NOTIFICATION code={error_code} subcode={error_subcode}")
            .with_debug("BGP NOTIFICATION: {error_name} / {error_subcode_name} from AS{peer_as}")
            .with_trace("BGP NOTIFICATION: {json_pretty(.)}"),
    )
});

// Implement Protocol trait (common functionality)
impl Protocol for BgpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "announce_route".to_string(),
                description: "NOT IMPLEMENTED - logs the prefix and returns NoAction. No UPDATE \
                              is generated and there is no RIB to announce from."
                    .to_string(),
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
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BGP announce {prefix} via {next_hop}")
                        .with_debug("BGP announce_route: prefix={prefix}, next_hop={next_hop}"),
                ),
            },
            ActionDefinition {
                name: "withdraw_route".to_string(),
                description: "NOT IMPLEMENTED - logs the prefix and returns NoAction. Nothing \
                              was ever announced, so there is nothing to withdraw."
                    .to_string(),
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
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BGP withdraw {prefix}")
                        .with_debug("BGP withdraw_route: prefix={prefix}"),
                ),
            },
            ActionDefinition {
                name: "reset_peer".to_string(),
                description: "Reset BGP session with peer (send NOTIFICATION and close)"
                    .to_string(),
                parameters: vec![],
                example: json!({
                    "type": "reset_peer"
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> BGP reset peer")
                        .with_debug("BGP reset_peer: sending NOTIFICATION and closing"),
                ),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_bgp_open_action(),
            send_bgp_keepalive_action(),
            send_bgp_update_action(),
            send_bgp_notification_action(),
            transition_state_action(),
            wait_for_more_action(),
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
            // BGP's assigned port is TCP 179, below 1024.
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(179))
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
                ctx.legacy_listen_addr(),
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
