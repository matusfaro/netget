//! WireGuard client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult, ConnectContext},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// WireGuard client connected event
pub static WIREGUARD_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "wireguard_connected",
        "WireGuard client successfully connected to VPN server",
    )
    .with_parameters(vec![
        Parameter {
            name: "server_endpoint".to_string(),
            type_hint: "string".to_string(),
            description: "VPN server endpoint (IP:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "client_public_key".to_string(),
            type_hint: "string".to_string(),
            description: "Client's WireGuard public key".to_string(),
            required: true,
        },
        Parameter {
            name: "client_address".to_string(),
            type_hint: "string".to_string(),
            description: "Client's VPN IP address".to_string(),
            required: true,
        },
    ])
});

/// WireGuard client disconnected event
pub static WIREGUARD_CLIENT_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "wireguard_disconnected",
        "WireGuard client disconnected from VPN server",
    )
    .with_parameters(vec![Parameter {
        name: "reason".to_string(),
        type_hint: "string".to_string(),
        description: "Disconnection reason".to_string(),
        required: true,
    }])
});

/// WireGuard client protocol action handler
pub struct WireguardClientProtocol;

impl WireguardClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for WireguardClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            use crate::client::wireguard::{WireguardClient, WireguardClientParams};

            // Parse startup params from instruction or use defaults
            // Expected format: "server_public_key=<key> server_endpoint=<ip:port> client_address=<ip/cidr> allowed_ips=<ips>"
            let params = parse_wireguard_params(&ctx.instruction)?;

            WireguardClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                params,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "get_connection_status".to_string(),
                description: "Get current WireGuard connection status and statistics".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "get_connection_status"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the WireGuard VPN server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
            ActionDefinition {
                name: "get_client_info".to_string(),
                description: "Get WireGuard client configuration information".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "get_client_info"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        // WireGuard doesn't have sync actions (no immediate response to network events)
        vec![]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "get_connection_status" => Ok(ClientActionResult::Custom {
                name: "wireguard_get_status".to_string(),
                data: json!({}),
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "get_client_info" => Ok(ClientActionResult::Custom {
                name: "wireguard_get_info".to_string(),
                data: json!({}),
            }),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            WIREGUARD_CLIENT_CONNECTED_EVENT.clone(),
            WIREGUARD_CLIENT_DISCONNECTED_EVENT.clone(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "wireguard"
    }

    fn stack_name(&self) -> &'static str {
        "VPN"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["wireguard", "wg", "vpn client", "wireguard client"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("defguard_wireguard_rs with kernel/userspace backend")
            .llm_control("VPN connection control, status queries")
            .e2e_testing("WireGuard server for client connections")
            .privilege_requirement(PrivilegeRequirement::Root)
            .notes("Requires root/CAP_NET_ADMIN on Linux, userspace on macOS")
            .build()
    }

    fn description(&self) -> &'static str {
        "WireGuard VPN client for secure tunneling"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to WireGuard VPN at 1.2.3.4:51820 with server public key abc123..."
    }

    fn group_name(&self) -> &'static str {
        "VPN"
    }

    fn get_startup_params(&self) -> Vec<Parameter> {
        vec![
            Parameter {
                name: "server_public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Server's WireGuard public key (base64 encoded)".to_string(),
                required: true,
            },
            Parameter {
                name: "server_endpoint".to_string(),
                type_hint: "string".to_string(),
                description: "Server endpoint (IP:port)".to_string(),
                required: true,
            },
            Parameter {
                name: "client_address".to_string(),
                type_hint: "string".to_string(),
                description: "Client's VPN IP address with CIDR (e.g., 10.20.30.2/32)".to_string(),
                required: true,
            },
            Parameter {
                name: "allowed_ips".to_string(),
                type_hint: "array".to_string(),
                description:
                    "IPs to route through VPN (e.g., [\"0.0.0.0/0\"] for all traffic)".to_string(),
                required: false,
            },
            Parameter {
                name: "keepalive".to_string(),
                type_hint: "number".to_string(),
                description: "Keepalive interval in seconds (0 to disable)".to_string(),
                required: false,
            },
            Parameter {
                name: "private_key".to_string(),
                type_hint: "string".to_string(),
                description: "Client's private key (base64 encoded, generated if not provided)"
                    .to_string(),
                required: false,
            },
        ]
    }
}

/// Parse WireGuard client parameters from instruction or JSON
fn parse_wireguard_params(instruction: &str) -> Result<crate::client::wireguard::WireguardClientParams> {
    // Try to parse as JSON first
    if let Ok(json_params) = serde_json::from_str::<serde_json::Value>(instruction) {
        return Ok(crate::client::wireguard::WireguardClientParams {
            server_public_key: json_params["server_public_key"]
                .as_str()
                .context("Missing server_public_key")?
                .to_string(),
            server_endpoint: json_params["server_endpoint"]
                .as_str()
                .context("Missing server_endpoint")?
                .to_string(),
            client_address: json_params["client_address"]
                .as_str()
                .context("Missing client_address")?
                .to_string(),
            allowed_ips: json_params["allowed_ips"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| vec!["0.0.0.0/0".to_string()]),
            keepalive: json_params["keepalive"]
                .as_u64()
                .map(|k| k as u16),
            private_key: json_params["private_key"]
                .as_str()
                .map(|s| s.to_string()),
        });
    }

    // Otherwise, parse as key=value pairs
    let mut server_public_key = None;
    let mut server_endpoint = None;
    let mut client_address = None;
    let mut allowed_ips = vec![];
    let mut keepalive = None;
    let mut private_key = None;

    for part in instruction.split_whitespace() {
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "server_public_key" => server_public_key = Some(value.to_string()),
                "server_endpoint" => server_endpoint = Some(value.to_string()),
                "client_address" => client_address = Some(value.to_string()),
                "allowed_ips" => {
                    allowed_ips = value.split(',').map(|s| s.to_string()).collect()
                }
                "keepalive" => keepalive = value.parse().ok(),
                "private_key" => private_key = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Ok(crate::client::wireguard::WireguardClientParams {
        server_public_key: server_public_key.context("Missing server_public_key parameter")?,
        server_endpoint: server_endpoint.context("Missing server_endpoint parameter")?,
        client_address: client_address.context("Missing client_address parameter")?,
        allowed_ips: if allowed_ips.is_empty() {
            vec!["0.0.0.0/0".to_string()]
        } else {
            allowed_ips
        },
        keepalive,
        private_key,
    })
}
