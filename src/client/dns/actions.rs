//! DNS client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// DNS client connected event
pub static DNS_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "dns_connected",
        "DNS client successfully connected to DNS server"
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "DNS server address".to_string(),
            required: true,
        },
    ])
});

/// DNS client response received event
pub static DNS_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "dns_response_received",
        "DNS response received from server"
    )
    .with_parameters(vec![
        Parameter {
            name: "query_id".to_string(),
            type_hint: "number".to_string(),
            description: "DNS query transaction ID".to_string(),
            required: true,
        },
        Parameter {
            name: "domain".to_string(),
            type_hint: "string".to_string(),
            description: "Domain name queried".to_string(),
            required: true,
        },
        Parameter {
            name: "query_type".to_string(),
            type_hint: "string".to_string(),
            description: "DNS query type (A, AAAA, MX, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "answers".to_string(),
            type_hint: "array".to_string(),
            description: "Array of answer records".to_string(),
            required: true,
        },
        Parameter {
            name: "response_code".to_string(),
            type_hint: "string".to_string(),
            description: "DNS response code (NOERROR, NXDOMAIN, SERVFAIL, etc.)".to_string(),
            required: true,
        },
    ])
});

/// DNS client protocol action handler
pub struct DnsClientProtocol;

impl DnsClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for DnsClientProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "send_dns_query".to_string(),
                    description: "Send a DNS query to the server".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "domain".to_string(),
                            type_hint: "string".to_string(),
                            description: "Domain name to query (e.g., example.com)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "query_type".to_string(),
                            type_hint: "string".to_string(),
                            description: "DNS query type (A, AAAA, MX, TXT, CNAME, NS, SOA, PTR, SRV, etc.)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "recursion_desired".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Request recursive resolution (default: true)".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_dns_query",
                        "domain": "example.com",
                        "query_type": "A",
                        "recursion_desired": true
                    }),
                },
                ActionDefinition {
                    name: "disconnect".to_string(),
                    description: "Disconnect from the DNS server".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "disconnect"
                    }),
                },
            ]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "send_dns_query".to_string(),
                    description: "Send another DNS query in response to received data".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "domain".to_string(),
                            type_hint: "string".to_string(),
                            description: "Domain name to query".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "query_type".to_string(),
                            type_hint: "string".to_string(),
                            description: "DNS query type".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "recursion_desired".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Request recursive resolution".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_dns_query",
                        "domain": "mail.example.com",
                        "query_type": "A"
                    }),
                },
                ActionDefinition {
                    name: "wait_for_more".to_string(),
                    description: "Wait for more data before responding".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "wait_for_more"
                    }),
                },
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "DNS"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                EventType {
                    id: "dns_connected".to_string(),
                    description: "Triggered when DNS client connects to server".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
                EventType {
                    id: "dns_response_received".to_string(),
                    description: "Triggered when DNS client receives a response".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
            ]
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>UDP>DNS"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["dns", "dns client", "connect to dns", "domain name system"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("hickory-dns (formerly trust-dns) for DNS queries")
                .llm_control("Full control over DNS queries (type, domain, recursion)")
                .e2e_testing("Public DNS servers (8.8.8.8, 1.1.1.1) or local DNS")
                .build()
        }
        fn description(&self) -> &'static str {
            "DNS client for domain name resolution"
        }
        fn example_prompt(&self) -> &'static str {
            "Connect to DNS at 8.8.8.8:53 and query A records for example.com"
        }
        fn group_name(&self) -> &'static str {
            "DNS"
        }
}

// Implement Client trait (client-specific functionality)
impl Client for DnsClientProtocol {
        fn connect(
            &self,
            ctx: crate::protocol::ConnectContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::client::dns::DnsClient;
                DnsClient::connect_with_llm_actions(
                    ctx.remote_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.client_id,
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
                "send_dns_query" => {
                    let domain = action
                        .get("domain")
                        .and_then(|v| v.as_str())
                        .context("Missing 'domain' field")?
                        .to_string();
    
                    let query_type = action
                        .get("query_type")
                        .and_then(|v| v.as_str())
                        .context("Missing 'query_type' field")?
                        .to_string();
    
                    let recursion_desired = action
                        .get("recursion_desired")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
    
                    // Return custom result with query data
                    Ok(ClientActionResult::Custom {
                        name: "dns_query".to_string(),
                        data: json!({
                            "domain": domain,
                            "query_type": query_type,
                            "recursion_desired": recursion_desired,
                        }),
                    })
                }
                "disconnect" => Ok(ClientActionResult::Disconnect),
                "wait_for_more" => Ok(ClientActionResult::WaitForMore),
                _ => Err(anyhow::anyhow!("Unknown DNS client action: {}", action_type)),
            }
        }
}

