//! NTP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct NtpProtocol;

impl NtpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for NtpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ntp::NtpServer;
            NtpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_ntp_time_response_action(),
            send_ntp_response_action(),
            ignore_request_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_ntp_time_response" => self.execute_send_ntp_time_response(action),
            "send_ntp_response" => self.execute_send_ntp_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown NTP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "NTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_ntp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>NTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ntp", "time"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(123))
            .implementation("Manual 48-byte NTP packet construction")
            .llm_control("Time responses (stratum, timestamps)")
            .e2e_testing("Manual NTP packet construction")
            .notes("Sub-ms with scripting, simple protocol")
            .build()
    }

    fn description(&self) -> &'static str {
        "Network Time Protocol server for time synchronization"
    }

    fn example_prompt(&self) -> &'static str {
        "pretend to be a ntp server on port 123"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

impl NtpProtocol {
    fn execute_send_ntp_time_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract all NTP fields from the action
        let stratum = action.get("stratum").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

        let reference_id = action
            .get("reference_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let leap_indicator = action
            .get("leap_indicator")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;

        let poll = action.get("poll").and_then(|v| v.as_u64()).unwrap_or(6) as u8;

        let precision = action
            .get("precision")
            .and_then(|v| v.as_i64())
            .unwrap_or(-20) as i8;

        let root_delay = action
            .get("root_delay")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let root_dispersion = action
            .get("root_dispersion")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Timestamps - support "current_time", unix timestamp (seconds), or null
        let reference_timestamp = Self::parse_timestamp(action.get("reference_timestamp"));
        let origin_timestamp = Self::parse_timestamp(action.get("origin_timestamp"));
        let receive_timestamp = Self::parse_timestamp(action.get("receive_timestamp"));
        let transmit_timestamp = Self::parse_timestamp(action.get("transmit_timestamp"));

        // Build NTP response packet
        let packet = Self::build_ntp_packet(
            leap_indicator,
            stratum,
            poll,
            precision,
            root_delay,
            root_dispersion,
            reference_id,
            reference_timestamp,
            origin_timestamp,
            receive_timestamp,
            transmit_timestamp,
        );
        Ok(ActionResult::Output(packet))
    }

    fn parse_timestamp(value: Option<&serde_json::Value>) -> Option<u64> {
        match value {
            Some(serde_json::Value::String(s)) if s == "current_time" => {
                Some(Self::get_current_ntp_time())
            }
            Some(serde_json::Value::Number(n)) => {
                n.as_u64().map(|timestamp| {
                    // If value is > 2^32 (4,294,967,296), it's a full 64-bit NTP timestamp (seconds + fraction)
                    // Otherwise, it's a Unix timestamp (seconds only) that needs conversion
                    if timestamp > 0xFFFFFFFF {
                        // Raw NTP timestamp (64-bit: 32-bit seconds + 32-bit fraction)
                        timestamp
                    } else {
                        // Unix timestamp (seconds since 1970) - convert to NTP timestamp (seconds part only)
                        // Note: This loses fractional precision, but LLM typically provides whole seconds
                        let ntp_seconds = timestamp + 2_208_988_800;
                        ntp_seconds << 32 // Shift to upper 32 bits, fraction = 0
                    }
                })
            }
            Some(serde_json::Value::Null) | None => None,
            _ => None,
        }
    }

    fn get_current_ntp_time() -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let ntp_seconds = now + 2_208_988_800; // Unix epoch to NTP epoch offset
        ntp_seconds << 32 // Return 64-bit timestamp: seconds in upper 32 bits, fraction=0 in lower 32 bits
    }

    fn execute_send_ntp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first (for binary NTP packets)
        // If hex decode fails, treat as raw string
        let bytes = if let Ok(decoded) = hex::decode(data) {
            decoded
        } else {
            data.as_bytes().to_vec()
        };

        Ok(ActionResult::Output(bytes))
    }

    /// Build a valid NTP response packet
    fn build_ntp_packet(
        leap_indicator: u8,
        stratum: u8,
        poll: u8,
        precision: i8,
        root_delay: f64,
        root_dispersion: f64,
        reference_id: &str,
        reference_timestamp: Option<u64>,
        origin_timestamp: Option<u64>,
        receive_timestamp: Option<u64>,
        transmit_timestamp: Option<u64>,
    ) -> Vec<u8> {
        let mut packet = vec![0u8; 48];

        // Byte 0: LI (2 bits), Version=4 (3 bits), Mode=4 (3 bits, server)
        let li = (leap_indicator & 0x03) << 6; // LI in bits 7-6
        let version = 0x04 << 3; // Version 4 in bits 5-3
        let mode = 0x04; // Mode 4 (server) in bits 2-0
        packet[0] = li | version | mode;

        // Byte 1: Stratum
        packet[1] = stratum;

        // Byte 2: Poll interval (log2 seconds)
        packet[2] = poll;

        // Byte 3: Precision (log2 seconds, signed 8-bit)
        packet[3] = precision as u8;

        // Bytes 4-7: Root delay (32-bit fixed point: 16 bits integer, 16 bits fraction)
        let root_delay_fixed = (root_delay * 65536.0) as u32;
        packet[4..8].copy_from_slice(&root_delay_fixed.to_be_bytes());

        // Bytes 8-11: Root dispersion (32-bit fixed point: 16 bits integer, 16 bits fraction)
        let root_dispersion_fixed = (root_dispersion * 65536.0) as u32;
        packet[8..12].copy_from_slice(&root_dispersion_fixed.to_be_bytes());

        // Bytes 12-15: Reference ID (4-byte ASCII identifier)
        let ref_id_bytes = if reference_id.is_empty() {
            [0u8; 4] // Zeros if not specified
        } else {
            let mut bytes = [0u8; 4];
            for (i, b) in reference_id.bytes().take(4).enumerate() {
                bytes[i] = b;
            }
            bytes
        };
        packet[12..16].copy_from_slice(&ref_id_bytes);

        // Helper to write timestamp (64-bit: upper 32 bits = seconds, lower 32 bits = fraction)
        let write_timestamp = |packet: &mut [u8], offset: usize, timestamp: Option<u64>| {
            if let Some(ntp_time) = timestamp {
                let seconds = ((ntp_time >> 32) & 0xFFFFFFFF) as u32; // Upper 32 bits
                let fraction = (ntp_time & 0xFFFFFFFF) as u32; // Lower 32 bits
                packet[offset..offset + 4].copy_from_slice(&seconds.to_be_bytes());
                packet[offset + 4..offset + 8].copy_from_slice(&fraction.to_be_bytes());
            }
            // else: leave as zeros
        };

        // Reference timestamp (bytes 16-23) - when the clock was last set
        write_timestamp(
            &mut packet,
            16,
            reference_timestamp.or_else(|| Some(Self::get_current_ntp_time())),
        );

        // Origin timestamp (bytes 24-31) - client's transmit time (should be copied from request)
        // Default to current_time if not provided (not ideal but better than zeros)
        write_timestamp(
            &mut packet,
            24,
            origin_timestamp.or_else(|| Some(Self::get_current_ntp_time())),
        );

        // Receive timestamp (bytes 32-39) - when we received the request
        write_timestamp(
            &mut packet,
            32,
            receive_timestamp.or_else(|| Some(Self::get_current_ntp_time())),
        );

        // Transmit timestamp (bytes 40-47) - when we send the response
        write_timestamp(
            &mut packet,
            40,
            transmit_timestamp.or_else(|| Some(Self::get_current_ntp_time())),
        );

        packet
    }
}

fn send_ntp_time_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ntp_time_response".to_string(),
        description: "Send NTP time synchronization response. Most fields have sensible defaults, override only if instructed.".to_string(),
        parameters: vec![
            Parameter {
                name: "leap_indicator".to_string(),
                type_hint: "number".to_string(),
                description: "Leap second warning: 0=no warning, 1=last minute has 61s, 2=last minute has 59s, 3=alarm/unsync. Default: 0".to_string(),
                required: false,
            },
            Parameter {
                name: "stratum".to_string(),
                type_hint: "number".to_string(),
                description: "Stratum level: 0=unspec, 1=primary (GPS/atomic), 2-15=secondary. Can be inferred from server instructions. Default: 2".to_string(),
                required: false,
            },
            Parameter {
                name: "poll".to_string(),
                type_hint: "number".to_string(),
                description: "Poll interval (log2 seconds): 4=16s, 6=64s, 10=1024s. Default: 6".to_string(),
                required: false,
            },
            Parameter {
                name: "precision".to_string(),
                type_hint: "number".to_string(),
                description: "Clock precision (log2 seconds): -6=~15ms, -20=~1us. Negative values. Default: -20".to_string(),
                required: false,
            },
            Parameter {
                name: "root_delay".to_string(),
                type_hint: "number".to_string(),
                description: "Total round-trip delay to primary reference (seconds, float). Default: 0.0".to_string(),
                required: false,
            },
            Parameter {
                name: "root_dispersion".to_string(),
                type_hint: "number".to_string(),
                description: "Max error relative to primary reference (seconds, float). Default: 0.0".to_string(),
                required: false,
            },
            Parameter {
                name: "reference_id".to_string(),
                type_hint: "string".to_string(),
                description: "4-char clock identifier: 'LOCL'=local, 'GPS.'=GPS, 'PPS.'=PPS, 'ATOM'=atomic, or IP address. Default: empty".to_string(),
                required: false,
            },
            Parameter {
                name: "reference_timestamp".to_string(),
                type_hint: "string or number".to_string(),
                description: "When clock was last set: 'current_time', Unix timestamp (seconds), or null. Default: current_time".to_string(),
                required: false,
            },
            Parameter {
                name: "origin_timestamp".to_string(),
                type_hint: "string or number".to_string(),
                description: "Client's transmit timestamp. Unix timestamp, or null. Default: extracted from client request. Leave null unless you need to test specific behavior.".to_string(),
                required: false,
            },
            Parameter {
                name: "receive_timestamp".to_string(),
                type_hint: "string or number".to_string(),
                description: "When server received request: 'current_time', Unix timestamp, or null. Default: current_time".to_string(),
                required: false,
            },
            Parameter {
                name: "transmit_timestamp".to_string(),
                type_hint: "string or number".to_string(),
                description: "When server sends response: 'current_time', Unix timestamp, or null. Default: current_time".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_ntp_time_response",
            "stratum": 2
        }),
    }
}

fn send_ntp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ntp_response".to_string(),
        description: "Send custom NTP response packet (advanced, for raw hex data)".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "NTP response packet as hex-encoded string (48 bytes = 96 hex chars)"
                .to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_ntp_response",
            "data": "240201e900000000000000000000000000000000000000000000000000000000eca56dd14ae94680eca56dd14ae94680eca56dd14ae94680"
        }),
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this NTP request".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
    }
}

// ============================================================================
// NTP Event Type Constants
// ============================================================================

/// NTP request event - triggered when NTP client sends a time request
pub static NTP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ntp_request",
        "NTP client sent a time synchronization request"
    )
    .with_parameters(vec![
        Parameter {
            name: "current_time".to_string(),
            type_hint: "number".to_string(),
            description: "Current server time as Unix timestamp".to_string(),
            required: true,
        },
        Parameter {
            name: "client_transmit_timestamp".to_string(),
            type_hint: "number".to_string(),
            description: "Client's transmit timestamp (Unix or NTP format) - must be echoed back as origin_timestamp".to_string(),
            required: false,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Size of received NTP packet in bytes".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_ntp_time_response_action(),
        send_ntp_response_action(),
        ignore_request_action(),
    ])
});

/// Get NTP event types
pub fn get_ntp_event_types() -> Vec<EventType> {
    vec![
        NTP_REQUEST_EVENT.clone(),
    ]
}
