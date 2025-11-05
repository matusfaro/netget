//! RIP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct RipProtocol;

impl RipProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for RipProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::rip::RipServer;
            RipServer::spawn_with_llm_actions(
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
            send_rip_response_action(),
            send_rip_request_action(),
            ignore_request_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_rip_response" => self.execute_send_rip_response(action),
            "send_rip_request" => self.execute_send_rip_request(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown RIP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "RIP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_rip_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>RIP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["rip", "routing"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(520))
            .implementation("Manual RIPv2 packet construction (RFC 2453)")
            .llm_control("Route advertisements, routing decisions")
            .e2e_testing("Manual RIP packet construction with route entries")
            .notes("Distance-vector routing protocol, 15 hop limit")
            .build()
    }

    fn description(&self) -> &'static str {
        "Routing Information Protocol v2 for dynamic routing"
    }

    fn example_prompt(&self) -> &'static str {
        "listen on port 520 via rip. Advertise routes for 192.168.1.0/24 (metric 1) and 10.0.0.0/8 (metric 5)"
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }
}

impl RipProtocol {
    fn execute_send_rip_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract routes from action
        let routes = action
            .get("routes")
            .and_then(|v| v.as_array())
            .context("Missing or invalid 'routes' field")?;

        if routes.len() > 25 {
            return Err(anyhow::anyhow!("Too many routes (max 25 per packet)"));
        }

        // Build RIP response packet
        let mut packet = Vec::new();

        // Header (4 bytes)
        packet.push(2); // Command: Response
        packet.push(2); // Version: RIPv2
        packet.push(0); // Unused
        packet.push(0); // Unused

        // Route entries (20 bytes each)
        for route in routes {
            let afi = route.get("afi").and_then(|v| v.as_u64()).unwrap_or(2) as u16; // IPv4 = 2
            let route_tag = route.get("route_tag").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let ip_str = route
                .get("ip_address")
                .and_then(|v| v.as_str())
                .context("Missing 'ip_address' in route")?;
            let subnet_mask_str = route
                .get("subnet_mask")
                .and_then(|v| v.as_str())
                .unwrap_or("255.255.255.0");
            let next_hop_str = route
                .get("next_hop")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0.0");
            let metric = route.get("metric").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

            // Parse IP addresses
            let ip_parts = Self::parse_ipv4(ip_str)?;
            let subnet_parts = Self::parse_ipv4(subnet_mask_str)?;
            let next_hop_parts = Self::parse_ipv4(next_hop_str)?;

            // Build route entry
            packet.extend_from_slice(&afi.to_be_bytes());
            packet.extend_from_slice(&route_tag.to_be_bytes());
            packet.extend_from_slice(&ip_parts);
            packet.extend_from_slice(&subnet_parts);
            packet.extend_from_slice(&next_hop_parts);
            packet.extend_from_slice(&metric.to_be_bytes());
        }

        Ok(ActionResult::Output(packet))
    }

    fn execute_send_rip_request(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract optional specific routes to request
        let routes = action.get("routes").and_then(|v| v.as_array());

        let mut packet = Vec::new();

        // Header (4 bytes)
        packet.push(1); // Command: Request
        packet.push(2); // Version: RIPv2
        packet.push(0); // Unused
        packet.push(0); // Unused

        if let Some(routes_array) = routes {
            // Request specific routes
            for route in routes_array {
                let afi = route.get("afi").and_then(|v| v.as_u64()).unwrap_or(2) as u16;
                let ip_str = route
                    .get("ip_address")
                    .and_then(|v| v.as_str())
                    .context("Missing 'ip_address' in route")?;

                let ip_parts = Self::parse_ipv4(ip_str)?;

                // Build route entry (metric 16 = infinity for requests)
                packet.extend_from_slice(&afi.to_be_bytes());
                packet.extend_from_slice(&[0, 0]); // Route tag
                packet.extend_from_slice(&ip_parts);
                packet.extend_from_slice(&[0, 0, 0, 0]); // Subnet mask
                packet.extend_from_slice(&[0, 0, 0, 0]); // Next hop
                packet.extend_from_slice(&16u32.to_be_bytes()); // Metric = 16
            }
        } else {
            // Request entire routing table (AFI=0, metric=16)
            packet.extend_from_slice(&[0, 0]); // AFI = 0 (special)
            packet.extend_from_slice(&[0, 0]); // Route tag
            packet.extend_from_slice(&[0, 0, 0, 0]); // IP
            packet.extend_from_slice(&[0, 0, 0, 0]); // Subnet mask
            packet.extend_from_slice(&[0, 0, 0, 0]); // Next hop
            packet.extend_from_slice(&16u32.to_be_bytes()); // Metric = 16
        }

        Ok(ActionResult::Output(packet))
    }

    fn parse_ipv4(ip_str: &str) -> Result<[u8; 4]> {
        let parts: Vec<&str> = ip_str.split('.').collect();
        if parts.len() != 4 {
            return Err(anyhow::anyhow!("Invalid IPv4 address: {}", ip_str));
        }

        Ok([
            parts[0].parse().context("Invalid IP octet")?,
            parts[1].parse().context("Invalid IP octet")?,
            parts[2].parse().context("Invalid IP octet")?,
            parts[3].parse().context("Invalid IP octet")?,
        ])
    }
}

// Event types
pub static RIP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "rip_request",
        "Triggered when a RIP message (request or response) is received"
    )
    .with_parameters(vec![
        Parameter {
            name: "command".to_string(),
            type_hint: "number".to_string(),
            description: "RIP command type (1=request, 2=response)".to_string(),
            required: true,
        },
        Parameter {
            name: "version".to_string(),
            type_hint: "number".to_string(),
            description: "RIP version (typically 2 for RIPv2)".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Message type: 'request' or 'response'".to_string(),
            required: true,
        },
        Parameter {
            name: "routes".to_string(),
            type_hint: "array".to_string(),
            description: "Array of route entries (ip_address, subnet_mask, next_hop, metric)".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_address".to_string(),
            type_hint: "string".to_string(),
            description: "Address of the RIP peer".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_received".to_string(),
            type_hint: "number".to_string(),
            description: "Size of the received packet".to_string(),
            required: true,
        },
    ])
});

fn get_rip_event_types() -> Vec<EventType> {
    vec![RIP_REQUEST_EVENT.clone()]
}

// Action definitions
fn send_rip_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_rip_response".to_string(),
        description: "Send RIP response with routing table entries".to_string(),
        parameters: vec![
            Parameter {
                name: "routes".to_string(),
                type_hint: "array".to_string(),
                description: "Array of route entries to advertise. Each route must have: ip_address (string), subnet_mask (string, default 255.255.255.0), next_hop (string, default 0.0.0.0), metric (number 1-15, default 1), afi (number, default 2 for IPv4), route_tag (number, default 0)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_rip_response",
            "routes": [
                {
                    "ip_address": "192.168.1.0",
                    "subnet_mask": "255.255.255.0",
                    "next_hop": "0.0.0.0",
                    "metric": 1
                },
                {
                    "ip_address": "10.0.0.0",
                    "subnet_mask": "255.0.0.0",
                    "next_hop": "192.168.1.1",
                    "metric": 5
                }
            ]
        }),
    }
}

fn send_rip_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_rip_request".to_string(),
        description: "Send RIP request to query routing table. If routes array is omitted, requests entire routing table.".to_string(),
        parameters: vec![
            Parameter {
                name: "routes".to_string(),
                type_hint: "array".to_string(),
                description: "Optional array of specific routes to request. Omit to request entire table.".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_rip_request"
        }),
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore the RIP message and send no response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
    }
}
