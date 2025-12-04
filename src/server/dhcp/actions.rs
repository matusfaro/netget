//! DHCP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::net::Ipv4Addr;
use std::sync::LazyLock;

#[cfg(feature = "dhcp")]
use dhcproto::{v4, Encodable, Encoder};

pub struct DhcpProtocol {
    #[cfg(feature = "dhcp")]
    request_context: std::sync::Arc<std::sync::Mutex<Option<DhcpRequestContext>>>,
}

#[cfg(feature = "dhcp")]
#[derive(Clone)]
pub struct DhcpRequestContext {
    pub xid: u32,        // Transaction ID
    pub chaddr: Vec<u8>, // Client MAC address
    pub message_type: v4::MessageType,
    pub ciaddr: Ipv4Addr,               // Client IP address (if set)
    pub requested_ip: Option<Ipv4Addr>, // Requested IP from options
}

impl DhcpProtocol {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "dhcp")]
            request_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[cfg(feature = "dhcp")]
    pub fn set_request_context(&self, context: DhcpRequestContext) {
        *self.request_context.lock().unwrap() = Some(context);
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for DhcpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_dhcp_offer_action(),
            send_dhcp_ack_action(),
            send_dhcp_nak_action(),
            send_dhcp_response_action(),
            ignore_request_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "DHCP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_dhcp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>DHCP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["dhcp"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(67))
            .implementation("dhcproto v0.11 for parsing")
            .llm_control("DISCOVER→OFFER, REQUEST→ACK flow + lease options")
            .e2e_testing("Manual DHCP packet construction - 3 LLM calls")
            .notes("Lenient validation for testing")
            .build()
    }
    fn description(&self) -> &'static str {
        "DHCP server for IP address assignment"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a DHCP server on interface eth0"
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
                "port": 67,
                "base_stack": "dhcp",
                "instruction": "DHCP server assigning IPs from 192.168.1.100-200, subnet 255.255.255.0, gateway 192.168.1.1, DNS 8.8.8.8"
            }),
            // Script-based example
            json!({
                "type": "open_server",
                "port": 67,
                "base_stack": "dhcp",
                "event_handlers": [{
                    "event_pattern": "dhcp_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# Handle DHCP DISCOVER/REQUEST\nif event.get('message_type') == 'DISCOVER':\n    respond([{'type': 'send_dhcp_offer', 'offered_ip': '192.168.1.100', 'subnet_mask': '255.255.255.0', 'router': '192.168.1.1', 'dns_servers': ['8.8.8.8'], 'lease_time': 86400}])\nelif event.get('message_type') == 'REQUEST':\n    respond([{'type': 'send_dhcp_ack', 'assigned_ip': '192.168.1.100', 'subnet_mask': '255.255.255.0', 'router': '192.168.1.1', 'dns_servers': ['8.8.8.8'], 'lease_time': 86400}])"
                    }
                }]
            }),
            // Static handler example
            json!({
                "type": "open_server",
                "port": 67,
                "base_stack": "dhcp",
                "event_handlers": [{
                    "event_pattern": "dhcp_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_dhcp_offer",
                            "offered_ip": "192.168.1.100",
                            "subnet_mask": "255.255.255.0",
                            "router": "192.168.1.1",
                            "dns_servers": ["8.8.8.8"],
                            "lease_time": 86400
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for DhcpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::dhcp::DhcpServer;
            DhcpServer::spawn_with_llm_actions(
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
            "send_dhcp_offer" => self.execute_send_dhcp_offer(action),
            "send_dhcp_ack" => self.execute_send_dhcp_ack(action),
            "send_dhcp_nak" => self.execute_send_dhcp_nak(action),
            "send_dhcp_response" => self.execute_send_dhcp_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown DHCP action: {}", action_type)),
        }
    }
}

impl DhcpProtocol {
    #[cfg(feature = "dhcp")]
    fn execute_send_dhcp_offer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let context = self
            .request_context
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("No DHCP request context available"))?;

        // Extract parameters from action
        let offered_ip = action
            .get("offered_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'offered_ip' parameter")?
            .parse::<Ipv4Addr>()?;

        let server_ip = action
            .get("server_ip")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0")
            .parse::<Ipv4Addr>()?;

        let subnet_mask = action
            .get("subnet_mask")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?;

        let router = action
            .get("router")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?;

        let dns_servers = action
            .get("dns_servers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| s.parse::<Ipv4Addr>().ok())
                    .collect::<Vec<_>>()
            });

        let lease_time = action
            .get("lease_time")
            .and_then(|v| v.as_u64())
            .unwrap_or(86400) as u32;

        // Build DHCP OFFER message
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootReply)
            .set_xid(context.xid)
            .set_flags(v4::Flags::default().set_broadcast())
            .set_yiaddr(offered_ip)
            .set_siaddr(server_ip)
            .set_chaddr(&context.chaddr);

        // Add DHCP options
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(v4::MessageType::Offer));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(server_ip));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(lease_time));

        if let Some(mask) = subnet_mask {
            msg.opts_mut().insert(v4::DhcpOption::SubnetMask(mask));
        }

        if let Some(gw) = router {
            msg.opts_mut().insert(v4::DhcpOption::Router(vec![gw]));
        }

        if let Some(dns) = dns_servers {
            if !dns.is_empty() {
                msg.opts_mut().insert(v4::DhcpOption::DomainNameServer(dns));
            }
        }

        // Encode to bytes
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;

        Ok(ActionResult::Output(buf))
    }

    #[cfg(feature = "dhcp")]
    fn execute_send_dhcp_ack(&self, action: serde_json::Value) -> Result<ActionResult> {
        let context = self
            .request_context
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("No DHCP request context available"))?;

        // Extract parameters
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

        let subnet_mask = action
            .get("subnet_mask")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?;

        let router = action
            .get("router")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?;

        let dns_servers = action
            .get("dns_servers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| s.parse::<Ipv4Addr>().ok())
                    .collect::<Vec<_>>()
            });

        let lease_time = action
            .get("lease_time")
            .and_then(|v| v.as_u64())
            .unwrap_or(86400) as u32;

        // Build DHCP ACK message
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootReply)
            .set_xid(context.xid)
            .set_flags(v4::Flags::default().set_broadcast())
            .set_yiaddr(assigned_ip)
            .set_siaddr(server_ip)
            .set_chaddr(&context.chaddr);

        // Add DHCP options
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(v4::MessageType::Ack));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(server_ip));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(lease_time));

        if let Some(mask) = subnet_mask {
            msg.opts_mut().insert(v4::DhcpOption::SubnetMask(mask));
        }

        if let Some(gw) = router {
            msg.opts_mut().insert(v4::DhcpOption::Router(vec![gw]));
        }

        if let Some(dns) = dns_servers {
            if !dns.is_empty() {
                msg.opts_mut().insert(v4::DhcpOption::DomainNameServer(dns));
            }
        }

        // Encode to bytes
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;

        Ok(ActionResult::Output(buf))
    }

    #[cfg(feature = "dhcp")]
    fn execute_send_dhcp_nak(&self, action: serde_json::Value) -> Result<ActionResult> {
        let context = self
            .request_context
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("No DHCP request context available"))?;

        let server_ip = action
            .get("server_ip")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0")
            .parse::<Ipv4Addr>()?;

        // Build DHCP NAK message
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootReply)
            .set_xid(context.xid)
            .set_flags(v4::Flags::default().set_broadcast())
            .set_siaddr(server_ip)
            .set_chaddr(&context.chaddr);

        // Add DHCP options
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(v4::MessageType::Nak));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(server_ip));

        // Optional message
        if let Some(message) = action.get("message").and_then(|v| v.as_str()) {
            msg.opts_mut()
                .insert(v4::DhcpOption::Message(message.to_string()));
        }

        // Encode to bytes
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;

        Ok(ActionResult::Output(buf))
    }

    #[cfg(not(feature = "dhcp"))]
    fn execute_send_dhcp_offer(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Err(anyhow!("DHCP feature not enabled"))
    }

    #[cfg(not(feature = "dhcp"))]
    fn execute_send_dhcp_ack(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Err(anyhow!("DHCP feature not enabled"))
    }

    #[cfg(not(feature = "dhcp"))]
    fn execute_send_dhcp_nak(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Err(anyhow!("DHCP feature not enabled"))
    }

    fn execute_send_dhcp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first (for binary DHCP packets)
        // If hex decode fails, treat as raw string
        let bytes = if let Ok(decoded) = hex::decode(data) {
            decoded
        } else {
            data.as_bytes().to_vec()
        };

        Ok(ActionResult::Output(bytes))
    }
}

fn send_dhcp_offer_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_offer".to_string(),
        description:
            "Send DHCP OFFER message in response to DISCOVER. Proposes IP configuration to client."
                .to_string(),
        parameters: vec![
            Parameter {
                name: "offered_ip".to_string(),
                type_hint: "string".to_string(),
                description: "IP address to offer to the client (e.g., '192.168.1.100')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "server_ip".to_string(),
                type_hint: "string".to_string(),
                description: "DHCP server IP address (default: '0.0.0.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "subnet_mask".to_string(),
                type_hint: "string".to_string(),
                description: "Subnet mask (e.g., '255.255.255.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "router".to_string(),
                type_hint: "string".to_string(),
                description: "Default gateway/router IP (e.g., '192.168.1.1')".to_string(),
                required: false,
            },
            Parameter {
                name: "dns_servers".to_string(),
                type_hint: "array of strings".to_string(),
                description: "DNS server IPs (e.g., ['8.8.8.8', '8.8.4.4'])".to_string(),
                required: false,
            },
            Parameter {
                name: "lease_time".to_string(),
                type_hint: "number".to_string(),
                description: "Lease duration in seconds (default: 86400 = 24 hours)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dhcp_offer",
            "offered_ip": "192.168.1.100",
            "subnet_mask": "255.255.255.0",
            "router": "192.168.1.1",
            "dns_servers": ["8.8.8.8", "8.8.4.4"],
            "lease_time": 86400
        }),
        log_template: None,
    }
}

fn send_dhcp_ack_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_ack".to_string(),
        description:
            "Send DHCP ACK message in response to REQUEST. Confirms IP assignment to client."
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
                description: "DHCP server IP address (default: '0.0.0.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "subnet_mask".to_string(),
                type_hint: "string".to_string(),
                description: "Subnet mask (e.g., '255.255.255.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "router".to_string(),
                type_hint: "string".to_string(),
                description: "Default gateway/router IP (e.g., '192.168.1.1')".to_string(),
                required: false,
            },
            Parameter {
                name: "dns_servers".to_string(),
                type_hint: "array of strings".to_string(),
                description: "DNS server IPs (e.g., ['8.8.8.8', '8.8.4.4'])".to_string(),
                required: false,
            },
            Parameter {
                name: "lease_time".to_string(),
                type_hint: "number".to_string(),
                description: "Lease duration in seconds (default: 86400 = 24 hours)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dhcp_ack",
            "assigned_ip": "192.168.1.100",
            "subnet_mask": "255.255.255.0",
            "router": "192.168.1.1",
            "dns_servers": ["8.8.8.8"],
            "lease_time": 86400
        }),
        log_template: None,
    }
}

fn send_dhcp_nak_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_nak".to_string(),
        description: "Send DHCP NAK message to reject a REQUEST. Informs client that the requested configuration is not valid.".to_string(),
        parameters: vec![
            Parameter {
                name: "server_ip".to_string(),
                type_hint: "string".to_string(),
                description: "DHCP server IP address (default: '0.0.0.0')".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional error message to include in NAK".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dhcp_nak",
            "message": "Requested IP address not available"
        }),
        log_template: None,
    }
}

fn send_dhcp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_response".to_string(),
        description: "Send custom DHCP response packet (advanced, for raw hex data)".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "DHCP response packet as hex-encoded string".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_dhcp_response",
            "data": "020106006395a3e3000080000000000000000000c0a8016400000000..."
        }),
        log_template: None,
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this DHCP request without sending a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
        log_template: None,
    }
}

// ============================================================================
// DHCP Event Type Constants
// ============================================================================

pub static DHCP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "dhcp_request",
        "DHCP client sent a request (DISCOVER, REQUEST, INFORM, etc.)",
        json!({
            "type": "send_dhcp_offer",
            "offered_ip": "192.168.1.100",
            "subnet_mask": "255.255.255.0",
            "router": "192.168.1.1",
            "dns_servers": ["8.8.8.8", "8.8.4.4"],
            "lease_time": 86400
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "DHCP message type (DISCOVER, REQUEST, INFORM, RELEASE, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "client_mac".to_string(),
            type_hint: "string".to_string(),
            description: "Client MAC address".to_string(),
            required: true,
        },
        Parameter {
            name: "requested_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Requested IP address (if any)".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_dhcp_offer_action(),
        send_dhcp_ack_action(),
        send_dhcp_nak_action(),
        send_dhcp_response_action(),
        ignore_request_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("DHCP {message_type} from {client_mac}")
            .with_debug("DHCP {message_type}: MAC={client_mac}, requested_ip={requested_ip}")
            .with_trace("DHCP request: {json_pretty(.)}"),
    )
});

pub fn get_dhcp_event_types() -> Vec<EventType> {
    vec![DHCP_REQUEST_EVENT.clone()]
}
