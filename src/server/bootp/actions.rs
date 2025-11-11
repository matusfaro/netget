//! BOOTP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::net::Ipv4Addr;
use std::sync::LazyLock;

#[cfg(feature = "bootp")]
use dhcproto::{v4, Encodable, Encoder};

pub struct BootpProtocol {
    #[cfg(feature = "bootp")]
    request_context: std::sync::Arc<std::sync::Mutex<Option<BootpRequestContext>>>,
}

#[cfg(feature = "bootp")]
#[derive(Clone)]
pub struct BootpRequestContext {
    pub xid: u32,         // Transaction ID
    pub chaddr: Vec<u8>,  // Client MAC address
    pub op: v4::Opcode,   // Operation code (BootRequest/BootReply)
    pub ciaddr: Ipv4Addr, // Client IP address
    pub giaddr: Ipv4Addr, // Gateway IP address (for relay)
    pub sname: String,    // Server host name
    pub file: String,     // Boot file name
}

impl BootpProtocol {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "bootp")]
            request_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[cfg(feature = "bootp")]
    pub fn set_request_context(&self, context: BootpRequestContext) {
        *self.request_context.lock().unwrap() = Some(context);
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for BootpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_bootp_reply_action(),
            send_bootp_response_action(),
            ignore_request_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "BOOTP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_bootp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>BOOTP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bootp", "bootstrap"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(67))
            .implementation("dhcproto v0.12 for parsing (BOOTP format)")
            .llm_control("BOOTREQUEST→BOOTREPLY flow + boot file location")
            .e2e_testing("Manual BOOTP packet construction - 3 LLM calls")
            .notes("Bootstrap Protocol (RFC 951) - DHCP predecessor")
            .build()
    }
    fn description(&self) -> &'static str {
        "BOOTP server for diskless workstation boot configuration"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a BOOTP server on port 67"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for BootpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::bootp::BootpServer;
            BootpServer::spawn_with_llm_actions(
                ctx.listen_addr,
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
            "send_bootp_reply" => self.execute_send_bootp_reply(action),
            "send_bootp_response" => self.execute_send_bootp_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown BOOTP action: {}", action_type)),
        }
    }
}

impl BootpProtocol {
    #[cfg(feature = "bootp")]
    fn execute_send_bootp_reply(&self, action: serde_json::Value) -> Result<ActionResult> {
        let context = self
            .request_context
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("No BOOTP request context available"))?;

        // Extract parameters from action
        let assigned_ip = action
            .get("assigned_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'assigned_ip' parameter")?
            .parse::<Ipv4Addr>()?;

        let server_ip = action
            .get("server_ip")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0")
            .parse::<Ipv4Addr>()?;

        let boot_file = action
            .get("boot_file")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let server_hostname = action
            .get("server_hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let gateway_ip = action
            .get("gateway_ip")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?
            .unwrap_or(Ipv4Addr::UNSPECIFIED);

        // Build BOOTP REPLY message
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootReply)
            .set_xid(context.xid)
            .set_flags(v4::Flags::default())
            .set_yiaddr(assigned_ip)
            .set_siaddr(server_ip)
            .set_giaddr(gateway_ip)
            .set_chaddr(&context.chaddr);

        // Set boot file name if provided
        if !boot_file.is_empty() {
            msg.set_fname_str(boot_file);
        }

        // Set server hostname if provided
        if !server_hostname.is_empty() {
            msg.set_sname_str(server_hostname);
        }

        // Encode to bytes
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;

        Ok(ActionResult::Output(buf))
    }

    #[cfg(not(feature = "bootp"))]
    fn execute_send_bootp_reply(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Err(anyhow!("BOOTP feature not enabled"))
    }

    fn execute_send_bootp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first (for binary BOOTP packets)
        // If hex decode fails, treat as raw string
        let bytes = if let Ok(decoded) = hex::decode(data) {
            decoded
        } else {
            data.as_bytes().to_vec()
        };

        Ok(ActionResult::Output(bytes))
    }
}

fn send_bootp_reply_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bootp_reply".to_string(),
        description:
            "Send BOOTP REPLY message in response to BOOTREQUEST. Provides IP configuration and boot file location to client."
                .to_string(),
        parameters: vec![
            Parameter {
                name: "assigned_ip".to_string(),
                type_hint: "string".to_string(),
                description: "IP address to assign to the client (e.g., '192.168.1.100')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "server_ip".to_string(),
                type_hint: "string".to_string(),
                description: "BOOTP server IP address (default: '0.0.0.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "boot_file".to_string(),
                type_hint: "string".to_string(),
                description: "Boot file name/path on server (e.g., 'boot/pxeboot.n12')".to_string(),
                required: false,
            },
            Parameter {
                name: "server_hostname".to_string(),
                type_hint: "string".to_string(),
                description: "Server hostname (e.g., 'bootserver.local')".to_string(),
                required: false,
            },
            Parameter {
                name: "gateway_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Gateway/relay IP address (default: '0.0.0.0')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_bootp_reply",
            "assigned_ip": "192.168.1.100",
            "server_ip": "192.168.1.1",
            "boot_file": "boot/pxeboot.n12",
            "server_hostname": "bootserver.local"
        }),
    }
}

fn send_bootp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bootp_response".to_string(),
        description: "Send custom BOOTP response packet (advanced, for raw hex data)".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "BOOTP response packet as hex-encoded string".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_bootp_response",
            "data": "020106006395a3e3000080000000000000000000c0a8016400000000..."
        }),
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this BOOTP request without sending a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
    }
}

// ============================================================================
// BOOTP Event Type Constants
// ============================================================================

pub static BOOTP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bootp_request",
        "BOOTP client sent a BOOTREQUEST (requesting IP and boot configuration)",
    )
    .with_parameters(vec![
        Parameter {
            name: "op_code".to_string(),
            type_hint: "string".to_string(),
            description: "BOOTP operation code (BootRequest or BootReply)".to_string(),
            required: true,
        },
        Parameter {
            name: "client_mac".to_string(),
            type_hint: "string".to_string(),
            description: "Client MAC address".to_string(),
            required: true,
        },
        Parameter {
            name: "client_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Client IP address (if set)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_bootp_reply_action(),
        send_bootp_response_action(),
        ignore_request_action(),
    ])
});

pub fn get_bootp_event_types() -> Vec<EventType> {
    vec![BOOTP_REQUEST_EVENT.clone()]
}
